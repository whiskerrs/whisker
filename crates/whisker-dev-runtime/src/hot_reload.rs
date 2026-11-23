//! WebSocket-based hot-reload patch receiver.
//!
//! Connection direction is **device → host**: a Whisker app running on
//! a device / emulator / simulator opens a WebSocket to the host
//! running `whisker run`. Before anything else, both peers prove they
//! hold the session secret `whisker run` provisioned for this launch
//! (see [`whisker_dev_protocol`]); until the server has proven it, the
//! receiver reads at most a few KiB. Every later message is an
//! authenticated envelope. Patch envelopes carry:
//!
//! ```text
//! [8 bytes: u64 BE — JSON header length]
//! [N bytes:        JSON header { "kind": "patch", "table": {...} } ]
//! [rest:           raw patch dylib bytes (no encoding) ]
//! ```
//!
//! The receiver writes the dylib bytes to a local cache file, rewrites
//! `table.lib` to that path, and publishes the JumpTable to a
//! process-wide coordinator. Each registered runtime is woken and
//! consumes every patch generation at the top of its own Host-driven
//! transaction. The coordinator serialises `subsecond::apply_patch`
//! process-wide while every runtime independently remounts the affected
//! component sites (or its root application when required).
//!
//! The receiver retries on disconnect with a small backoff so a
//! `whisker run` restart on the host doesn't require restarting the
//! app on the device. Authentication and protocol failures are written
//! to stderr once per distinct reason; an unreachable server is not.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use subsecond::JumpTable;
use whisker_runtime::view::Element;
use whisker_runtime::{FrameTiming, RuntimeInstance, RuntimeWakeHandle};

const MAX_PATCH_HISTORY: usize = 64;

#[derive(Clone)]
struct AppliedPatch {
    generation: u64,
    patched_functions: Vec<usize>,
}

struct Subscriber {
    wake: RuntimeWakeHandle,
    seen_generation: u64,
}

#[derive(Default)]
struct Coordinator {
    pending: Option<JumpTable>,
    generation: u64,
    history: VecDeque<AppliedPatch>,
    subscribers: HashMap<u64, Subscriber>,
}

static COORDINATOR: LazyLock<Mutex<Coordinator>> =
    LazyLock::new(|| Mutex::new(Coordinator::default()));
static APPLY_LOCK: Mutex<()> = Mutex::new(());
static NEXT_SUBSCRIBER: AtomicU64 = AtomicU64::new(1);

/// One process-wide native code update as observed by a runtime instance.
///
/// Applying the dynamic-library patch is process-wide, while rebuilding the
/// retained component tree is instance-local. A registration therefore sees
/// every applied generation exactly once even when several Whisker surfaces
/// share the same application process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCodeUpdate {
    /// Monotonic process-wide patch generation.
    pub generation: u64,
    /// Host function addresses rewritten since this runtime last polled.
    pub patched_functions: Vec<usize>,
    /// Whether this runtime fell behind the bounded patch history and must
    /// conservatively rebuild its application root.
    pub requires_root_remount: bool,
}

/// Per-runtime subscription to the process-wide native patch coordinator.
pub struct NativeHotReloadRegistration {
    id: u64,
}

/// Platform-neutral native hot-reload adapter for one runtime instance.
///
/// Android, iOS, and Desktop all call [`Self::apply`] at the beginning of a
/// Host-driven UI transaction. Transport, process-wide patching, targeted
/// component reflection, and root fallback therefore have one implementation.
pub struct NativeHotReload {
    registration: NativeHotReloadRegistration,
    application: fn() -> Element,
    application_hash: fn() -> u64,
    mounted_hash: u64,
}

impl NativeHotReload {
    /// Registers a mounted runtime with the process coordinator.
    pub fn new(
        wake: RuntimeWakeHandle,
        application: fn() -> Element,
        application_hash: fn() -> u64,
    ) -> Self {
        Self {
            registration: register_native_runtime(wake),
            application,
            application_hash,
            mounted_hash: application_hash(),
        }
    }

    /// Applies and reflects any patch not yet observed by `runtime`.
    pub fn apply(&mut self, runtime: &mut RuntimeInstance) -> Result<bool, String> {
        runtime.set_frame_timing(crate::trace::enabled());
        let Some(update) = self.registration.poll()? else {
            return Ok(false);
        };
        let current_hash = (self.application_hash)();
        let mut remount_root = update.requires_root_remount || current_hash != self.mounted_hash;
        if !remount_root {
            let stats = runtime
                .remount_changed_components()
                .map_err(|error| error.to_string())?;
            remount_root = stats.remounted == 0 || stats.layout_changed > 0;
        }
        if remount_root {
            runtime
                .remount_root(self.application)
                .map_err(|error| error.to_string())?;
        }
        self.mounted_hash = current_hash;
        Ok(true)
    }

    /// Queues one frame's timing for `whisker run --trace`.
    pub fn record_frame(&self, timing: Option<FrameTiming>) {
        if let Some(timing) = timing {
            crate::trace::record(timing);
        }
    }
}

impl NativeHotReloadRegistration {
    /// Applies a queued process patch at this Host transaction's safe point,
    /// then returns the code updates this runtime has not reflected yet.
    pub fn poll(&mut self) -> Result<Option<NativeCodeUpdate>, String> {
        apply_pending_patch()?;
        let mut coordinator = COORDINATOR
            .lock()
            .map_err(|_| "native hot-reload coordinator lock was poisoned".to_string())?;
        let Some(subscriber) = coordinator.subscribers.get(&self.id) else {
            return Ok(None);
        };
        let seen = subscriber.seen_generation;
        if seen == coordinator.generation {
            return Ok(None);
        }

        let requires_root_remount = coordinator
            .history
            .front()
            .is_some_and(|patch| patch.generation > seen.saturating_add(1));
        let mut patched_functions = Vec::new();
        for patch in coordinator
            .history
            .iter()
            .filter(|patch| patch.generation > seen)
        {
            for function in &patch.patched_functions {
                if !patched_functions.contains(function) {
                    patched_functions.push(*function);
                }
            }
        }
        let generation = coordinator.generation;
        coordinator
            .subscribers
            .get_mut(&self.id)
            .expect("subscriber remains registered while polling")
            .seen_generation = generation;
        discard_observed_history(&mut coordinator);
        Ok(Some(NativeCodeUpdate {
            generation,
            patched_functions,
            requires_root_remount,
        }))
    }
}

impl Drop for NativeHotReloadRegistration {
    fn drop(&mut self) {
        if let Ok(mut coordinator) = COORDINATOR.lock() {
            coordinator.subscribers.remove(&self.id);
            discard_observed_history(&mut coordinator);
        }
    }
}

/// Registers one native runtime and starts the process receiver on first use.
pub fn register_native_runtime(wake: RuntimeWakeHandle) -> NativeHotReloadRegistration {
    crate::log_capture::start_log_capture();
    start_receiver();
    let id = NEXT_SUBSCRIBER.fetch_add(1, Ordering::Relaxed);
    let mut coordinator = COORDINATOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let seen_generation = coordinator.generation;
    coordinator.subscribers.insert(
        id,
        Subscriber {
            wake,
            seen_generation,
        },
    );
    NativeHotReloadRegistration { id }
}

fn apply_pending_patch() -> Result<(), String> {
    let _apply = APPLY_LOCK
        .lock()
        .map_err(|_| "native hot-reload apply lock was poisoned".to_string())?;
    let pending = COORDINATOR
        .lock()
        .map_err(|_| "native hot-reload coordinator lock was poisoned".to_string())?
        .pending
        .take();
    let Some(table) = pending else {
        return Ok(());
    };
    let patched = unsafe { subsecond::apply_patch(table) }
        .map_err(|error| format!("apply native hot-reload patch: {error}"))?;
    let mut coordinator = COORDINATOR
        .lock()
        .map_err(|_| "native hot-reload coordinator lock was poisoned".to_string())?;
    coordinator.generation = coordinator.generation.saturating_add(1);
    let generation = coordinator.generation;
    coordinator.history.push_back(AppliedPatch {
        generation,
        patched_functions: patched
            .into_iter()
            .map(|function| function as usize)
            .collect(),
    });
    while coordinator.history.len() > MAX_PATCH_HISTORY {
        coordinator.history.pop_front();
    }
    Ok(())
}

fn discard_observed_history(coordinator: &mut Coordinator) {
    let minimum_seen = coordinator
        .subscribers
        .values()
        .map(|subscriber| subscriber.seen_generation)
        .min()
        .unwrap_or(coordinator.generation);
    while coordinator
        .history
        .front()
        .is_some_and(|patch| patch.generation <= minimum_seen)
    {
        coordinator.history.pop_front();
    }
}

fn wake_registered_runtimes() {
    let wakes = COORDINATOR
        .lock()
        .map(|coordinator| {
            coordinator
                .subscribers
                .values()
                .map(|subscriber| subscriber.wake.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for wake in wakes {
        wake.wake();
    }
}

/// Spawn the receiver thread. Reads `WHISKER_DEV_ADDR` from the env;
/// if unset, falls back to `127.0.0.1:9876` (the dev-server's
/// default), which works on Android once `adb reverse` is in place.
/// Safe to call unconditionally from app bootstrap — the loop retries
/// on connection failure so a dev server starting later still gets
/// picked up.
pub fn start_receiver() {
    // Once per process: the receiver thread + its WebSocket outlive any
    // single bootstrap, so a re-`run()` (Android Activity recreation)
    // must not spawn a second thread racing the same dev server.
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let addr = std::env::var("WHISKER_DEV_ADDR")
        .ok()
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "127.0.0.1:9876".to_string());
    std::thread::Builder::new()
        .name("whisker-hot-reload".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => return,
            };
            rt.block_on(client_loop(addr));
        })
        .expect("spawn whisker-hot-reload thread");
}

/// Bytes the receiver reads before the server has proven the session secret.
///
/// Covers the HTTP upgrade response plus one server-proof message. The
/// WebSocket's own [`whisker_dev_protocol::MAX_MESSAGE`] limit only applies
/// once the server is authenticated, so an unauthenticated peer can't make
/// the device buffer a large message.
const HANDSHAKE_READ_BUDGET: usize = 2 * whisker_dev_protocol::MAX_HANDSHAKE;

/// How a connection attempt ended, deciding what (if anything) is logged.
#[derive(Debug)]
enum SessionEnd {
    /// Nothing accepted the TCP connection: `whisker run` isn't running.
    Unreachable,
    /// An authenticated session ended because the connection closed.
    Closed,
    /// The peer failed authentication or broke the protocol.
    Rejected(String),
}

/// Turns session outcomes into stderr lines, once per distinct reason.
///
/// A stale app retries every 2 s; repeating the same reason would flood
/// logcat / syslog, which is where stderr ends up via `log_capture`.
#[derive(Default)]
struct FailureLog {
    last: Option<String>,
}

impl FailureLog {
    fn observe(&mut self, end: SessionEnd) -> Option<String> {
        match end {
            SessionEnd::Unreachable => None,
            SessionEnd::Closed => {
                self.last = None;
                None
            }
            SessionEnd::Rejected(reason) => {
                if self.last.as_deref() == Some(reason.as_str()) {
                    return None;
                }
                let line = format!("whisker hot-reload: {reason}");
                self.last = Some(reason);
                Some(line)
            }
        }
    }
}

async fn client_loop(addr: String) {
    let mut failures = FailureLog::default();
    loop {
        let end = match dev_token() {
            Some(secret) => connect_and_serve(&addr, &secret, handle_patch_frame).await,
            None => SessionEnd::Rejected(
                "no development session secret was provisioned; launch the app with \
                 `whisker run` to enable native hot reload"
                    .into(),
            ),
        };
        if let Some(line) = failures.observe(end) {
            eprintln!("{line}");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn connect_and_serve(addr: &str, secret: &str, on_patch: impl FnMut(&[u8])) -> SessionEnd {
    use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

    let Ok(stream) = tokio::net::TcpStream::connect(addr).await else {
        return SessionEnd::Unreachable;
    };
    let budget = ReadBudget::new(HANDSHAKE_READ_BUDGET);
    let config = WebSocketConfig {
        max_frame_size: Some(whisker_dev_protocol::MAX_MESSAGE),
        max_message_size: Some(whisker_dev_protocol::MAX_MESSAGE),
        ..Default::default()
    };
    let ws = match tokio_tungstenite::client_async_with_config(
        format!("ws://{addr}/whisker-dev"),
        BudgetedStream::new(stream, budget.clone()),
        Some(config),
    )
    .await
    {
        Ok((ws, _)) => ws,
        Err(error) => {
            return SessionEnd::Rejected(format!(
                "could not open the development WebSocket at {addr}: {error}"
            ));
        }
    };
    handle_session(
        ws,
        &budget,
        secret,
        whisker_dev_protocol::HANDSHAKE_TIMEOUT,
        on_patch,
    )
    .await
}

/// Shared cap on how many more bytes a [`BudgetedStream`] may read.
#[derive(Clone)]
struct ReadBudget(std::sync::Arc<AtomicUsize>);

impl ReadBudget {
    const UNLIMITED: usize = usize::MAX;

    fn new(limit: usize) -> Self {
        Self(std::sync::Arc::new(AtomicUsize::new(limit)))
    }

    fn lift(&self) {
        self.0.store(Self::UNLIMITED, Ordering::Release);
    }
}

/// Transport adapter that fails reads beyond a [`ReadBudget`].
///
/// tokio-tungstenite 0.21 fixes its message limits when the WebSocket is
/// created, so the pre-authentication limit is enforced one layer below it.
struct BudgetedStream<S> {
    inner: S,
    budget: ReadBudget,
}

impl<S> BudgetedStream<S> {
    fn new(inner: S, budget: ReadBudget) -> Self {
        Self { inner, budget }
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for BudgetedStream<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let remaining = this.budget.0.load(Ordering::Acquire);
        if remaining == ReadBudget::UNLIMITED {
            return std::pin::Pin::new(&mut this.inner).poll_read(cx, buf);
        }
        if remaining == 0 {
            return std::task::Poll::Ready(Err(std::io::Error::other(format!(
                "development server sent more than {HANDSHAKE_READ_BUDGET} bytes before \
                 authenticating"
            ))));
        }
        let limit = remaining.min(buf.remaining());
        let read = {
            let mut limited = tokio::io::ReadBuf::new(buf.initialize_unfilled_to(limit));
            std::task::ready!(std::pin::Pin::new(&mut this.inner).poll_read(cx, &mut limited))?;
            limited.filled().len()
        };
        buf.advance(read);
        this.budget.0.store(remaining - read, Ordering::Release);
        std::task::Poll::Ready(Ok(()))
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for BudgetedStream<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Runtime address of `whisker_aslr_anchor` on the device, handed to
/// the dev server on connect so it can bake the ASLR slide into the
/// patches it builds.
fn device_aslr_reference() -> u64 {
    subsecond::aslr_reference() as u64
}

/// The shared dev-session token, if `whisker run` provisioned one.
///
/// Used for mutual proofs, never sent to the WebSocket peer. iOS and desktop
/// read WHISKER_DEV_TOKEN; Android reads debug.whisker_dev_token. Missing secrets
/// disable native patch reception.
fn dev_token() -> Option<String> {
    if let Ok(t) = std::env::var("WHISKER_DEV_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    #[cfg(target_os = "android")]
    {
        if let Some(t) = android_system_property("debug.whisker_dev_token") {
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// Read an Android system property by name via bionic's
/// `__system_property_get`.
#[cfg(target_os = "android")]
fn android_system_property(name: &str) -> Option<String> {
    let cname = std::ffi::CString::new(name).ok()?;
    // PROP_VALUE_MAX = 92. `c_char` (unsigned on Android) so the
    // buffer pointer matches bionic's signature.
    let mut buf = [0 as libc::c_char; 92];
    // SAFETY: `cname` is a valid NUL-terminated C string; `buf` is a
    // 92-byte buffer matching PROP_VALUE_MAX, which is the size bionic
    // writes into. The return value is the length written (excluding
    // NUL), or <= 0 when the property is unset.
    let len = unsafe { libc::__system_property_get(cname.as_ptr(), buf.as_mut_ptr()) };
    if len <= 0 {
        return None;
    }
    let bytes: Vec<u8> = buf[..len as usize].iter().map(|&b| b as u8).collect();
    String::from_utf8(bytes).ok()
}

/// Next text message during the handshake, skipping control frames.
async fn next_text<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>) -> Result<String, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => return Ok(text),
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
            Some(Ok(Message::Binary(_))) => {
                return Err("the server sent a binary message during authentication".into());
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err("the server closed the connection during authentication".into());
            }
            Some(Err(error)) => return Err(error.to_string()),
        }
    }
}

async fn authenticate<S>(
    ws: &mut tokio_tungstenite::WebSocketStream<S>,
    secret: &str,
) -> Result<whisker_dev_protocol::Session, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let (handshake, challenge) =
        whisker_dev_protocol::ClientHandshake::start(secret).map_err(|error| error.to_string())?;
    ws.send(Message::Text(challenge))
        .await
        .map_err(|error| error.to_string())?;
    let server_proof = next_text(ws).await?;
    let (session, client_proof) = handshake
        .finish(&server_proof)
        .map_err(|error| error.to_string())?;
    ws.send(Message::Text(client_proof))
        .await
        .map_err(|error| error.to_string())?;
    Ok(session)
}

/// Why an authenticated session stopped.
enum Failure {
    /// The connection dropped; expected when `whisker run` exits.
    Transport,
    /// The server broke the authenticated protocol.
    Protocol(String),
}

impl From<tokio_tungstenite::tungstenite::Error> for Failure {
    fn from(_: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Transport
    }
}

impl From<whisker_dev_protocol::Error> for Failure {
    fn from(error: whisker_dev_protocol::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}

async fn handle_session<S>(
    mut ws: tokio_tungstenite::WebSocketStream<BudgetedStream<S>>,
    budget: &ReadBudget,
    secret: &str,
    handshake_timeout: Duration,
    mut on_patch: impl FnMut(&[u8]),
) -> SessionEnd
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut session = match tokio::time::timeout(handshake_timeout, authenticate(&mut ws, secret))
        .await
    {
        Ok(Ok(session)) => session,
        Ok(Err(reason)) => return SessionEnd::Rejected(format!("authentication failed: {reason}")),
        Err(_) => {
            return SessionEnd::Rejected(format!(
                "authentication did not finish within {handshake_timeout:?}"
            ));
        }
    };
    // The server sends nothing after its proof until it has verified ours,
    // so every byte read so far belonged to the handshake and nothing is
    // buffered past it when the cap is lifted.
    budget.lift();
    match serve_session(&mut ws, &mut session, &mut on_patch).await {
        Ok(()) | Err(Failure::Transport) => SessionEnd::Closed,
        Err(Failure::Protocol(reason)) => SessionEnd::Rejected(reason),
    }
}

async fn serve_session<S>(
    ws: &mut tokio_tungstenite::WebSocketStream<S>,
    session: &mut whisker_dev_protocol::Session,
    on_patch: &mut impl FnMut(&[u8]),
) -> Result<(), Failure>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use whisker_dev_protocol::FrameKind;

    let hello = serde_json::json!({ "kind": "hello", "aslr_reference": device_aslr_reference() })
        .to_string();
    ws.send(Message::Binary(
        session.seal(FrameKind::Text, hello.as_bytes())?,
    ))
    .await?;

    let mut trace_flush = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = trace_flush.tick() => {
                let frames = crate::trace::take();
                if !frames.is_empty() {
                    ws.send(Message::Binary(session.seal(FrameKind::Text, crate::trace::encode(&frames).as_bytes())?)).await?;
                }
            }
            // device → host: forward captured stdout/stderr lines,
            // batched so a burst of `println!`s is one frame.
            lines = crate::log_capture::drain_pending_logs() => {
                for line in lines {
                    let frame = serde_json::json!({
                        "kind": "log",
                        "stream": line.stream.as_wire(),
                        "line": line.text,
                        "ts_micros": line.ts_micros.to_string(),
                    })
                    .to_string();
                    ws.send(Message::Binary(session.seal(FrameKind::Text, frame.as_bytes())?)).await?;
                }
            }
            // host → device: patches + close.
            msg = ws.next() => {
                let Some(msg) = msg else { return Ok(()); };
                match msg? {
                    Message::Binary(bytes) => match session.open(&bytes)? {
                        (FrameKind::Patch, payload) => on_patch(payload),
                        (FrameKind::Text, payload) => handle_control_frame(
                            std::str::from_utf8(payload).map_err(|_| {
                                Failure::Protocol("development control message is not UTF-8".into())
                            })?,
                        ),
                    },
                    Message::Text(_) => {
                        return Err(Failure::Protocol(
                            "the server sent an unauthenticated text message".into(),
                        ));
                    }
                    Message::Close(_) => return Ok(()),
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        }
    }
}

fn handle_control_frame(text: &str) {
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Control {
        Trace { enabled: bool },
    }
    if let Ok(Control::Trace { enabled }) = serde_json::from_str(text) {
        crate::trace::set_enabled(enabled);
        wake_registered_runtimes();
    }
}

/// Decode one patch frame from the dev-server and park it in the process
/// coordinator for a Host transaction to apply at its next safe point.
fn handle_patch_frame(bytes: &[u8]) {
    let (mut table, dylib_bytes) = match parse_patch_frame(bytes) {
        Ok(parsed) => parsed,
        Err(_) => return,
    };
    let local = match materialise_patch_dylib(dylib_bytes) {
        Ok(p) => p,
        Err(_) => return,
    };
    table.lib = local;
    if let Ok(mut coordinator) = COORDINATOR.lock() {
        coordinator.pending = Some(table);
    }
    // Every retained surface must observe the new process generation. Its
    // wake endpoint posts onto the owning Host UI lane.
    wake_registered_runtimes();
}

/// Write the patch dylib payload to a file under the app's cache dir
/// and return the local path — what `table.lib` gets overwritten
/// with, so `subsecond::apply_patch`'s `dlopen` sees a real on-device
/// file.
///
/// Each patch uses a fresh owner-only directory and an exclusively created file;
/// old files remain for the OS to reclaim with the cache directory.
fn materialise_patch_dylib(
    bytes: &[u8],
) -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Write;
    let base = patch_cache_dir().ok_or("could not resolve a writable patch cache")?;
    std::fs::create_dir_all(&base)?;
    let suffix: String = whisker_dev_protocol::random_bytes::<16>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let dir = base.join(format!("whisker-patch-{suffix}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    }
    #[cfg(not(unix))]
    std::fs::DirBuilder::new().create(&dir)?;
    let extension = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(any(target_os = "macos", target_os = "ios")) {
        "dylib"
    } else {
        "so"
    };
    let path = dir.join(format!("patch.{extension}"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&path)?.write_all(bytes)?;
    Ok(path)
}

/// Resolve a writable, dlopen-able base directory for patch dylibs: on
/// Android `/data/data/<package>/cache/` (package name from
/// `/proc/self/cmdline`), elsewhere `$TMPDIR`. Each patch gets its own
/// random `whisker-patch-<hex>/` directory below it.
fn patch_cache_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    {
        let cmdline = std::fs::read_to_string("/proc/self/cmdline").ok()?;
        let pkg = cmdline.split('\0').next().unwrap_or("").trim().to_string();
        if !pkg.is_empty() {
            return Some(std::path::PathBuf::from(format!("/data/data/{pkg}/cache")));
        }
        None
    }
    #[cfg(not(target_os = "android"))]
    {
        Some(std::env::temp_dir())
    }
}

// ----- Wire format ----------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Header {
    Patch {
        #[serde(deserialize_with = "deserialize_jump_table")]
        table: JumpTable,
    },
}

/// Counterpart of `whisker-dev-server::server::wire_jump_table::serialize`.
/// Reads the address map as a JSON array of `[old, new]` pairs and
/// reconstructs the `subsecond_types::JumpTable`. See the server side
/// for the JSON-object-vs-array rationale.
fn deserialize_jump_table<'de, D>(d: D) -> Result<JumpTable, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    use std::path::PathBuf;
    use subsecond_types::AddressMap;

    #[derive(Deserialize)]
    struct Wire {
        lib: PathBuf,
        map: Vec<(u64, u64)>,
        aslr_reference: u64,
        new_base_address: u64,
        ifunc_count: u64,
    }
    let w = Wire::deserialize(d)?;
    let mut map = AddressMap::default();
    map.reserve(w.map.len());
    for (k, v) in w.map {
        map.insert(k, v);
    }
    Ok(JumpTable {
        lib: w.lib,
        map,
        aslr_reference: w.aslr_reference,
        new_base_address: w.new_base_address,
        ifunc_count: w.ifunc_count,
    })
}

/// Parse a binary patch frame into `(JumpTable, dylib_bytes_slice)`.
/// See the module docstring for the on-the-wire layout.
fn parse_patch_frame(
    bytes: &[u8],
) -> Result<(JumpTable, &[u8]), Box<dyn std::error::Error + Send + Sync>> {
    if bytes.len() < 8 {
        return Err(format!("frame too short ({} bytes, need ≥8)", bytes.len()).into());
    }
    let json_len = usize::try_from(u64::from_be_bytes(bytes[..8].try_into().unwrap()))
        .map_err(|_| "patch header length overflow")?;
    if json_len > whisker_dev_protocol::MAX_CONTROL {
        return Err("patch metadata exceeds size limit".into());
    }
    let header_end = 8usize.checked_add(json_len).ok_or("json_len overflow")?;
    if bytes.len() < header_end {
        return Err(format!(
            "frame truncated: header claims {} json bytes but only {} available",
            json_len,
            bytes.len() - 8,
        )
        .into());
    }
    let header: Header = serde_json::from_slice(&bytes[8..header_end]).map_err(
        |e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("parse json header: {e}").into()
        },
    )?;
    let Header::Patch { table } = header;
    Ok((table, &bytes[header_end..]))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_runtime_observes_a_process_patch_once() {
        let mut coordinator = COORDINATOR
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *coordinator = Coordinator::default();
        coordinator.generation = 1;
        coordinator.history.push_back(AppliedPatch {
            generation: 1,
            patched_functions: vec![0x1234, 0x5678],
        });
        for id in [41, 42] {
            coordinator.subscribers.insert(
                id,
                Subscriber {
                    wake: RuntimeWakeHandle::new(|| {}),
                    seen_generation: 0,
                },
            );
        }
        drop(coordinator);

        let mut first = NativeHotReloadRegistration { id: 41 };
        let mut second = NativeHotReloadRegistration { id: 42 };
        let expected = NativeCodeUpdate {
            generation: 1,
            patched_functions: vec![0x1234, 0x5678],
            requires_root_remount: false,
        };
        assert_eq!(first.poll().unwrap(), Some(expected.clone()));
        assert_eq!(first.poll().unwrap(), None);
        assert_eq!(second.poll().unwrap(), Some(expected));
        assert_eq!(second.poll().unwrap(), None);
    }

    /// Pack a JSON header + raw dylib bytes into the on-the-wire
    /// binary frame, matching what the server emits.
    fn make_frame(json: &str, dylib: &[u8]) -> Vec<u8> {
        let json_bytes = json.as_bytes();
        let mut frame = Vec::with_capacity(8 + json_bytes.len() + dylib.len());
        frame.extend_from_slice(&(json_bytes.len() as u64).to_be_bytes());
        frame.extend_from_slice(json_bytes);
        frame.extend_from_slice(dylib);
        frame
    }

    #[test]
    fn parses_a_minimal_patch_frame() {
        let json = r#"{
            "kind": "patch",
            "table": {
                "lib": "/tmp/some-patch.dylib",
                "map": [],
                "aslr_reference": 4294967296,
                "new_base_address": 8589934592,
                "ifunc_count": 0
            }
        }"#;
        let frame = make_frame(json, b"");
        let (table, dylib) = parse_patch_frame(&frame).expect("should parse");
        assert_eq!(table.lib.to_string_lossy(), "/tmp/some-patch.dylib",);
        assert_eq!(table.aslr_reference, 0x1_0000_0000);
        assert_eq!(table.new_base_address, 0x2_0000_0000);
        assert_eq!(table.ifunc_count, 0);
        assert!(table.map.is_empty());
        assert!(dylib.is_empty());
    }

    #[test]
    fn parses_a_frame_with_a_non_empty_address_map_and_dylib_bytes() {
        let json = r#"{
            "kind": "patch",
            "table": {
                "lib": "/tmp/p.so",
                "map": [[100, 200], [300, 400]],
                "aslr_reference": 0,
                "new_base_address": 0,
                "ifunc_count": 0
            }
        }"#;
        let dylib_bytes = b"\x00\x01\x02\x03";
        let frame = make_frame(json, dylib_bytes);
        let (table, dylib) = parse_patch_frame(&frame).expect("should parse");
        assert_eq!(table.map.len(), 2);
        assert_eq!(table.map.get(&100), Some(&200));
        assert_eq!(table.map.get(&300), Some(&400));
        assert_eq!(dylib, dylib_bytes);
    }

    #[test]
    fn materialise_patch_dylib_writes_bytes_to_cache_and_returns_path() {
        let payload = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00";
        let path = materialise_patch_dylib(payload).expect("write");
        let read_back = std::fs::read(&path).unwrap();
        assert_eq!(read_back, payload);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_unknown_envelope_kind() {
        let frame = make_frame(r#"{ "kind": "frobnicate" }"#, b"");
        assert!(parse_patch_frame(&frame).is_err());
    }

    #[test]
    fn rejects_truncated_frame() {
        // Five bytes can't hold the 8-byte length prefix.
        assert!(parse_patch_frame(&[0u8; 5]).is_err());
    }

    #[test]
    fn rejects_frame_whose_header_length_overruns_the_payload() {
        // Claim 100 bytes of JSON, supply zero.
        let mut frame = Vec::new();
        frame.extend_from_slice(&100u64.to_be_bytes());
        assert!(parse_patch_frame(&frame).is_err());
    }

    mod session {
        use super::super::*;
        use futures_util::{SinkExt, StreamExt};
        use tokio::io::DuplexStream;
        use tokio_tungstenite::WebSocketStream;
        use tokio_tungstenite::tungstenite::{Message, protocol::Role};
        use whisker_dev_protocol::{FrameKind, ServerHandshake, Session};

        const SECRET: &str = "test-secret";

        type Client = WebSocketStream<BudgetedStream<DuplexStream>>;
        type Server = WebSocketStream<DuplexStream>;

        async fn pair() -> (Client, ReadBudget, Server) {
            let (left, right) = tokio::io::duplex(256 * 1024);
            let budget = ReadBudget::new(HANDSHAKE_READ_BUDGET);
            let client = WebSocketStream::from_raw_socket(
                BudgetedStream::new(left, budget.clone()),
                Role::Client,
                None,
            )
            .await;
            let server = WebSocketStream::from_raw_socket(right, Role::Server, None).await;
            (client, budget, server)
        }

        async fn text(server: &mut Server) -> String {
            loop {
                match server.next().await.unwrap().unwrap() {
                    Message::Text(text) => return text,
                    Message::Ping(_) | Message::Pong(_) => {}
                    other => panic!("expected a text message, got {other:?}"),
                }
            }
        }

        /// Answer the receiver's handshake like the dev server and consume its hello.
        async fn accept(server: &mut Server, secret: &str) -> Session {
            let (handshake, proof) = ServerHandshake::respond(secret, &text(server).await).unwrap();
            server.send(Message::Text(proof)).await.unwrap();
            let mut session = handshake.finish(&text(server).await).unwrap();
            let Message::Binary(hello) = server.next().await.unwrap().unwrap() else {
                panic!("missing hello");
            };
            let (kind, payload) = session.open(&hello).unwrap();
            assert_eq!(kind, FrameKind::Text);
            assert!(
                std::str::from_utf8(payload)
                    .unwrap()
                    .contains("aslr_reference")
            );
            session
        }

        /// Wait for the receiver to hang up, failing if it sends any data.
        async fn expect_hangup(server: &mut Server) {
            while let Some(Ok(message)) = server.next().await {
                assert!(
                    matches!(message, Message::Close(_)),
                    "receiver sent {message:?} after rejecting the server"
                );
            }
        }

        async fn run(
            client: Client,
            budget: &ReadBudget,
            patches: &mut Vec<Vec<u8>>,
        ) -> SessionEnd {
            handle_session(client, budget, SECRET, Duration::from_secs(5), |patch| {
                patches.push(patch.to_vec())
            })
            .await
        }

        fn rejected(end: SessionEnd) -> String {
            match end {
                SessionEnd::Rejected(reason) => reason,
                other => panic!("expected a rejection, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn substituted_server_learns_nothing() {
            let (client, budget, mut server) = pair().await;
            let mut patches = Vec::new();
            let spoof = async {
                let challenge = text(&mut server).await;
                assert!(!challenge.contains(SECRET));
                let forged = format!(r#"{{"nonce":{:?},"mac":{:?}}}"#, [0u8; 32], [0u8; 32]);
                server.send(Message::Text(forged)).await.unwrap();
            };
            let (end, ()) = tokio::join!(run(client, &budget, &mut patches), spoof);
            assert!(rejected(end).contains("different `whisker run` session"));
            expect_hangup(&mut server).await;
        }

        #[tokio::test]
        async fn server_with_another_secret_is_rejected() {
            let (client, budget, mut server) = pair().await;
            let mut patches = Vec::new();
            let other = async {
                let (_, proof) =
                    ServerHandshake::respond("other-secret", &text(&mut server).await).unwrap();
                server.send(Message::Text(proof)).await.unwrap();
            };
            let (end, ()) = tokio::join!(run(client, &budget, &mut patches), other);
            assert!(rejected(end).contains("does not hold this app's session secret"));
            expect_hangup(&mut server).await;
        }

        #[tokio::test]
        async fn ping_during_the_handshake_is_skipped_and_large_patches_arrive_after_it() {
            let (client, budget, mut server) = pair().await;
            let mut patches = Vec::new();
            // Larger than the pre-authentication budget, so this also proves
            // the budget is lifted once the server is authenticated.
            let patch = vec![0xA5; 4 * HANDSHAKE_READ_BUDGET];
            let host = async {
                let challenge = text(&mut server).await;
                server.send(Message::Ping(vec![1, 2, 3])).await.unwrap();
                let (handshake, proof) = ServerHandshake::respond(SECRET, &challenge).unwrap();
                server.send(Message::Text(proof)).await.unwrap();
                let mut session = handshake.finish(&text(&mut server).await).unwrap();
                server.next().await.unwrap().unwrap();
                let frame = session.seal(FrameKind::Patch, &patch).unwrap();
                server.send(Message::Binary(frame)).await.unwrap();
                server.close(None).await.unwrap();
            };
            let (end, ()) = tokio::join!(run(client, &budget, &mut patches), host);
            assert!(matches!(end, SessionEnd::Closed), "{end:?}");
            assert_eq!(patches, [patch]);
        }

        #[tokio::test]
        async fn modified_frame_is_rejected_before_it_reaches_the_patch_handler() {
            let (client, budget, mut server) = pair().await;
            let mut patches = Vec::new();
            let host = async {
                let mut session = accept(&mut server, SECRET).await;
                let mut frame = session.seal(FrameKind::Patch, b"library").unwrap();
                *frame.last_mut().unwrap() ^= 1;
                server.send(Message::Binary(frame)).await.unwrap();
                expect_hangup(&mut server).await;
            };
            let (end, ()) = tokio::join!(run(client, &budget, &mut patches), host);
            assert_eq!(rejected(end), "development frame failed authentication");
            assert!(patches.is_empty());
        }

        #[tokio::test]
        async fn replayed_frame_is_rejected() {
            let (client, budget, mut server) = pair().await;
            let mut patches = Vec::new();
            let host = async {
                let mut session = accept(&mut server, SECRET).await;
                let frame = session.seal(FrameKind::Patch, b"library").unwrap();
                server.send(Message::Binary(frame.clone())).await.unwrap();
                server.send(Message::Binary(frame)).await.unwrap();
                expect_hangup(&mut server).await;
            };
            let (end, ()) = tokio::join!(run(client, &budget, &mut patches), host);
            assert_eq!(rejected(end), "replayed or out-of-order development frame");
            assert_eq!(patches, [b"library".to_vec()]);
        }

        #[tokio::test]
        async fn unauthenticated_text_after_the_handshake_is_rejected() {
            let (client, budget, mut server) = pair().await;
            let mut patches = Vec::new();
            let host = async {
                accept(&mut server, SECRET).await;
                server
                    .send(Message::Text(r#"{"kind":"trace","enabled":true}"#.into()))
                    .await
                    .unwrap();
                expect_hangup(&mut server).await;
            };
            let (end, ()) = tokio::join!(run(client, &budget, &mut patches), host);
            assert!(rejected(end).contains("unauthenticated text message"));
        }

        #[tokio::test]
        async fn silent_server_times_out_the_handshake() {
            let (client, budget, mut server) = pair().await;
            let silent = tokio::spawn(async move {
                text(&mut server).await;
                std::future::pending::<()>().await;
            });
            let end = handle_session(client, &budget, SECRET, Duration::from_millis(50), |_| {
                panic!("no patch expected")
            })
            .await;
            silent.abort();
            assert!(rejected(end).contains("did not finish within"));
        }

        #[tokio::test]
        async fn oversized_pre_authentication_message_exhausts_the_read_budget() {
            let (client, budget, mut server) = pair().await;
            let flood = tokio::spawn(async move {
                text(&mut server).await;
                let _ = server
                    .send(Message::Text("x".repeat(HANDSHAKE_READ_BUDGET + 1)))
                    .await;
                std::future::pending::<()>().await;
            });
            let mut patches = Vec::new();
            let end = run(client, &budget, &mut patches).await;
            flood.abort();
            assert!(rejected(end).contains("bytes before authenticating"));
        }

        fn dummy_patch(dylib: &[u8]) -> whisker_dev_server::Patch {
            whisker_dev_server::Patch {
                table: serde_json::from_str(
                    r#"{"lib":"/tmp/p.so","map":{},"aslr_reference":0,"new_base_address":0,"ifunc_count":0}"#,
                )
                .unwrap(),
                dylib_bytes: std::sync::Arc::new(dylib.to_vec()),
            }
        }

        #[tokio::test]
        async fn receiver_authenticates_against_the_dev_server_and_receives_its_patch() {
            let (sender, addr, _handle) = whisker_dev_server::server::serve(
                "127.0.0.1:0".parse().unwrap(),
                None,
                Some(SECRET.into()),
                Vec::new(),
                false,
                None,
            )
            .await
            .unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let addr = addr.to_string();
            let receiver = connect_and_serve(&addr, SECRET, move |patch| {
                tx.send(patch.to_vec()).unwrap();
            });
            let host = async {
                for _ in 0..400 {
                    if sender.latest_aslr_reference().is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                assert_eq!(sender.send(dummy_patch(b"library")), 1);
                let frame = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                let (table, dylib) = parse_patch_frame(&frame).unwrap();
                assert_eq!(table.lib, std::path::Path::new("/tmp/p.so"));
                assert_eq!(dylib, b"library");
            };
            tokio::select! {
                end = receiver => panic!("session ended early: {end:?}"),
                () = host => {}
            }
        }

        #[tokio::test]
        async fn receiver_launched_by_another_session_reports_the_stale_secret() {
            let (sender, addr, _handle) = whisker_dev_server::server::serve(
                "127.0.0.1:0".parse().unwrap(),
                None,
                Some("current-session".into()),
                Vec::new(),
                false,
                None,
            )
            .await
            .unwrap();
            let end = connect_and_serve(&addr.to_string(), "previous-session", |_| {
                panic!("no patch expected")
            })
            .await;
            assert!(rejected(end).contains("different `whisker run` session"));
            assert_eq!(sender.latest_aslr_reference(), None);
        }

        #[tokio::test]
        async fn missing_server_is_unreachable_and_never_logged() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            drop(listener);
            let end = connect_and_serve(&addr, SECRET, |_| {}).await;
            assert!(matches!(end, SessionEnd::Unreachable), "{end:?}");
            assert_eq!(FailureLog::default().observe(end), None);
        }

        #[test]
        fn repeated_failures_are_logged_once_per_reason() {
            let mut log = FailureLog::default();
            let stale = || SessionEnd::Rejected("stale".into());
            assert_eq!(
                log.observe(stale()).as_deref(),
                Some("whisker hot-reload: stale")
            );
            assert_eq!(log.observe(stale()), None);
            assert_eq!(log.observe(SessionEnd::Unreachable), None);
            assert_eq!(log.observe(stale()), None);
            assert!(log.observe(SessionEnd::Rejected("other".into())).is_some());
            assert_eq!(log.observe(SessionEnd::Closed), None);
            assert!(log.observe(stale()).is_some());
        }
    }

    #[test]
    fn coordinator_starts_without_a_pending_patch() {
        let mut coordinator = COORDINATOR
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        coordinator.pending = None;
        assert!(coordinator.pending.is_none());
    }
}
