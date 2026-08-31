//! WebSocket-based hot-reload patch receiver.
//!
//! Connection direction is **device → host**: a Whisker app running on
//! a device / emulator / simulator opens a WebSocket to the host
//! running `whisker run`. The host pushes patches as *binary* frames
//! laid out as:
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
//! app on the device.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use subsecond::JumpTable;
use whisker_runtime::view::Element;
use whisker_runtime::{RuntimeInstance, RuntimeWakeHandle};

/// Log a one-line message tagged `whisker-dev`. On Android this goes
/// to logcat and on iOS to `syslog(3)` — plain `eprintln!` reaches
/// neither platform's log surface. Elsewhere it is an `eprintln!`.
///
/// Public so whisker-driver's patch-apply path can log under the same
/// tag.
pub fn devlog(line: &str) {
    #[cfg(target_os = "android")]
    {
        // Both tag and text must be NUL-terminated.
        unsafe extern "C" {
            fn __android_log_write(
                prio: std::os::raw::c_int,
                tag: *const std::os::raw::c_char,
                text: *const std::os::raw::c_char,
            ) -> std::os::raw::c_int;
        }
        const ANDROID_LOG_INFO: std::os::raw::c_int = 4;
        let tag = b"whisker-dev\0";
        let mut buf: Vec<u8> = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line.as_bytes());
        buf.push(0);
        unsafe {
            __android_log_write(
                ANDROID_LOG_INFO,
                tag.as_ptr() as *const _,
                buf.as_ptr() as *const _,
            );
        }
    }
    #[cfg(target_os = "ios")]
    {
        unsafe extern "C" {
            fn syslog(priority: std::os::raw::c_int, fmt: *const std::os::raw::c_char, ...);
        }
        // LOG_INFO surfaces in `log stream` without being filtered as
        // debug noise.
        const LOG_INFO: std::os::raw::c_int = 6;
        let mut buf: Vec<u8> = Vec::with_capacity(line.len() + 16);
        buf.extend_from_slice(b"[whisker-dev] ");
        buf.extend_from_slice(line.as_bytes());
        buf.push(0);
        let fmt = b"%s\0";
        unsafe {
            syslog(LOG_INFO, fmt.as_ptr() as *const _, buf.as_ptr());
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        eprintln!("[whisker-dev] {line}");
    }
}

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
        let Some(update) = self.registration.poll()? else {
            return Ok(false);
        };
        let current_hash = (self.application_hash)();
        let mut remount_root = update.requires_root_remount || current_hash != self.mounted_hash;
        let generation = update.generation;
        if !remount_root {
            let patched_functions = update
                .patched_functions
                .iter()
                .map(|address| *address as *const ())
                .collect::<Vec<_>>();
            let stats = runtime
                .remount_components(&patched_functions)
                .map_err(|error| error.to_string())?;
            remount_root = stats.remounted == 0 || stats.layout_changed > 0;
            devlog(&format!(
                "patch generation {generation} reflected: {} component(s), {} layout mismatch(es)",
                stats.remounted, stats.layout_changed,
            ));
        }
        if remount_root {
            runtime
                .remount_root(self.application)
                .map_err(|error| error.to_string())?;
            devlog(&format!(
                "patch generation {generation} reflected with an application-root remount"
            ));
        }
        self.mounted_hash = current_hash;
        Ok(true)
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
    devlog(&format!(
        "hot-reload receiver targeting ws://{addr}/whisker-dev",
    ));
    std::thread::Builder::new()
        .name("whisker-hot-reload".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    devlog(&format!("couldn't build tokio runtime: {e}"));
                    return;
                }
            };
            rt.block_on(client_loop(addr));
        })
        .expect("spawn whisker-hot-reload thread");
}

async fn client_loop(addr: String) {
    use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

    let url = format!("ws://{addr}/whisker-dev");
    // Patch dylibs for large apps exceed tungstenite's 16 MiB read
    // default; dev-only loopback channel, so lift the limits entirely.
    let ws_config = WebSocketConfig {
        max_frame_size: None,
        max_message_size: None,
        ..Default::default()
    };
    loop {
        match tokio_tungstenite::connect_async_with_config(&url, Some(ws_config), false).await {
            Ok((ws, _)) => {
                devlog(&format!("connected: {url}"));
                if let Err(e) = handle_session(ws).await {
                    devlog(&format!("session ended: {e}"));
                }
            }
            Err(e) => devlog(&format!("connect {url} failed: {e}")),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
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
/// The server rejects clients whose `hello` doesn't carry the
/// session's token (the patch channel `dlopen`s whatever it receives,
/// so an unauthenticated connection on a LAN-exposed bind is a
/// remote-code-execution surface). Delivery is per-platform:
///   * iOS Simulator / host: the `WHISKER_DEV_TOKEN` env var.
///   * Android: the `debug.whisker_dev_token` system property — the
///     app process doesn't inherit adb-set env vars.
///
/// `None` = token-less setup; the server then runs unauthenticated.
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

async fn handle_session<S>(
    mut ws: tokio_tungstenite::WebSocketStream<S>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    // The hello goes first — the server needs our `aslr_reference` to
    // build patches at all.
    let hello = serde_json::json!({
        "kind": "hello",
        "aslr_reference": device_aslr_reference(),
        "token": dev_token(),
    })
    .to_string();
    devlog(&format!(
        "sending hello with aslr_reference={:#x}",
        device_aslr_reference()
    ));
    ws.send(Message::Text(hello)).await?;

    loop {
        tokio::select! {
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
                    ws.send(Message::Text(frame)).await?;
                }
            }
            // host → device: patches + close.
            msg = ws.next() => {
                let Some(msg) = msg else { return Ok(()); };
                match msg? {
                    Message::Binary(bytes) => handle_patch_frame(&bytes),
                    Message::Close(_) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

/// Decode one patch frame from the dev-server and park it in the process
/// coordinator for a Host transaction to apply at its next safe point.
fn handle_patch_frame(bytes: &[u8]) {
    devlog(&format!("patch frame received ({} bytes)", bytes.len()));
    let (mut table, dylib_bytes) = match parse_patch_frame(bytes) {
        Ok(parsed) => parsed,
        Err(e) => {
            devlog(&format!("malformed patch frame: {e}"));
            return;
        }
    };
    devlog(&format!(
        "frame parsed (map={} entries, dylib={} bytes)",
        table.map.len(),
        dylib_bytes.len(),
    ));
    let local = match materialise_patch_dylib(dylib_bytes) {
        Ok(p) => p,
        Err(e) => {
            devlog(&format!("could not materialise patch dylib: {e}"));
            return;
        }
    };
    devlog(&format!("patch dylib materialised at {}", local.display()));
    table.lib = local;
    if let Ok(mut coordinator) = COORDINATOR.lock() {
        coordinator.pending = Some(table);
        devlog("patch queued");
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
/// File naming uses a monotonic counter + timestamp so multiple
/// patches in one session don't collide; old files are left for the
/// OS to reclaim with the cache dir.
fn materialise_patch_dylib(
    bytes: &[u8],
) -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    use std::sync::atomic::{AtomicU64, Ordering};

    let dir = patch_cache_dir().ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
        "could not resolve a writable cache dir".into()
    })?;
    std::fs::create_dir_all(&dir)?;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let extension = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(any(target_os = "macos", target_os = "ios")) {
        "dylib"
    } else {
        "so"
    };
    let path = dir.join(format!("patch-{ts}-{n}.{extension}"));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Resolve a writable, dlopen-able directory for patch dylibs: on
/// Android `/data/data/<package>/cache/whisker-patches/` (package
/// name from `/proc/self/cmdline`), elsewhere `$TMPDIR/whisker-patches/`.
fn patch_cache_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    {
        let cmdline = std::fs::read_to_string("/proc/self/cmdline").ok()?;
        let pkg = cmdline.split('\0').next().unwrap_or("").trim().to_string();
        if !pkg.is_empty() {
            return Some(std::path::PathBuf::from(format!(
                "/data/data/{pkg}/cache/whisker-patches"
            )));
        }
        None
    }
    #[cfg(not(target_os = "android"))]
    {
        Some(std::env::temp_dir().join("whisker-patches"))
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
    let json_len = u64::from_be_bytes(bytes[..8].try_into().unwrap()) as usize;
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

    #[test]
    fn coordinator_starts_without_a_pending_patch() {
        let mut coordinator = COORDINATOR
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        coordinator.pending = None;
        assert!(coordinator.pending.is_none());
    }
}
