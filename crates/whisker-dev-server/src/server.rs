//! WebSocket dev server.
//!
//! `whisker run` opens a TCP listener, exposes a single
//! `GET /whisker-dev` route that upgrades to WebSocket, and pushes
//! patch messages to every connected client. The on-device
//! `whisker-dev-runtime` is the canonical client.
//!
//! ## Wire format
//!
//! With a session secret, every client first completes the
//! [`whisker_dev_protocol`] handshake and then exchanges only authenticated
//! envelopes. After the handshake the device sends a `hello` with its
//! `aslr_reference`, then logs and traces; the server sends patches and
//! control messages. A patch envelope carries
//! `[u64 BE header length][JSON jump table][library bytes]`.
//!
//! Without a secret, clients are browsers: they exchange plain messages and
//! receive reload requests and validated WebAssembly patches, never native
//! libraries.
//!
//! ## Architecture
//!
//! A single `tokio::sync::broadcast` channel: every connected socket
//! has its own subscriber receiver, so one `PatchSender::send` reaches
//! all clients. New connections see only payloads sent *after* they
//! subscribe — the receiver is at the tail end of the broadcast
//! buffer, not replayed.

use anyhow::Result;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tower_http::services::{ServeDir, ServeFile};

use crate::Event;
use whisker_dev_protocol::{self as protocol, FrameKind, ServerHandshake, Session};

/// Cheap-to-clone broadcast payload. The dylib bytes are held by
/// `Arc` so cloning into each subscribed client's receive queue is
/// just a refcount bump.
#[derive(Debug, Clone)]
pub struct Patch {
    /// The address-map metadata. Serialized as JSON in the binary
    /// frame's prefix.
    pub table: subsecond_types::JumpTable,
    /// Raw native library or WebAssembly module bytes following the JSON
    /// prefix. Native clients stage and load a library; browser clients apply
    /// a separately validated WebAssembly side module.
    pub dylib_bytes: Arc<Vec<u8>>,
}

/// JSON header that prefixes the binary patch frame. Mirrors the
/// shape `whisker-dev-runtime::hot_reload::Header` deserialises.
///
/// `table.map` is serialised as a JSON array of `[old, new]` pairs
/// rather than a JSON object. JSON objects can only have string
/// keys, so the default `HashMap<u64, u64>` derive would produce
/// `{ "1234": 5678 }` — and the matching deserialize side, given a
/// custom hasher like `subsecond_types::BuildAddressHasher`, fails
/// to convert the string back to `u64`. The pair-array form
/// sidesteps both.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PatchHeader<'a> {
    Patch {
        #[serde(serialize_with = "wire_jump_table::serialize")]
        table: &'a subsecond_types::JumpTable,
    },
}

/// Serializer for the patch header's jump table.
///
/// Its JSON shape must match `deserialize_jump_table` in
/// `whisker-dev-runtime`'s `hot_reload` module. It stays next to its
/// `subsecond_types` users instead of moving into `whisker-dev-protocol`,
/// which carries opaque payloads and has no subsecond dependency.
pub mod wire_jump_table {
    use serde::Serializer;
    use serde::ser::SerializeStruct;
    use subsecond_types::JumpTable;

    pub fn serialize<S: Serializer>(t: &JumpTable, s: S) -> Result<S::Ok, S::Error> {
        let pairs: Vec<(u64, u64)> = t.map.iter().map(|(k, v)| (*k, *v)).collect();
        let mut st = s.serialize_struct("JumpTable", 5)?;
        st.serialize_field("lib", &t.lib)?;
        st.serialize_field("map", &pairs)?;
        st.serialize_field("aslr_reference", &t.aslr_reference)?;
        st.serialize_field("new_base_address", &t.new_base_address)?;
        st.serialize_field("ifunc_count", &t.ifunc_count)?;
        st.end()
    }
}

/// Cheap-to-clone handle for sending patches from the rest of the
/// dev server (file watcher / builder / etc.) to every connected
/// client.
#[derive(Clone)]
pub struct PatchSender {
    tx: broadcast::Sender<ServerUpdate>,
    /// Latest `aslr_reference` reported in a connected client's `hello`
    /// message. Single-slot, last-write-wins: we don't yet
    /// support targeted-per-client patches, so all connected clients
    /// must share an ASLR base. For typical single-emulator dev
    /// sessions that's fine; for multi-device this becomes the
    /// natural boundary where patches start being per-client.
    aslr_reference: Arc<Mutex<Option<u64>>>,
}

impl PatchSender {
    /// Queue a native patch for delivery to authenticated native clients.
    ///
    /// Browser clients never receive it. Returns the number of
    /// subscribed clients — `0` is fine (no client connected yet) and
    /// not an error. A patch too large for one envelope is dropped with a
    /// warning when it is encoded; the developer then needs a Full Reload.
    pub fn send(&self, patch: Patch) -> usize {
        self.tx.send(ServerUpdate::Patch(patch)).unwrap_or(0)
    }

    /// Queue a WebAssembly side module for browser clients only.
    ///
    /// The Web target's dev server runs without a session secret (see
    /// [`crate::Config::dev_token`]), so its clients are unauthenticated.
    /// To keep that channel from carrying anything but WebAssembly, the
    /// payload must start with the Wasm magic and version and pass
    /// `wasmparser` validation; anything else, including native libraries,
    /// is not queued and `0` is returned. Authenticated native clients
    /// never receive these patches. Otherwise returns the number of
    /// subscribed clients, like [`Self::send`].
    pub fn send_web(&self, patch: Patch) -> usize {
        if !patch.dylib_bytes.starts_with(b"\0asm\x01\0\0\0")
            || wasmparser::Validator::new()
                .validate_all(&patch.dylib_bytes)
                .is_err()
        {
            return 0;
        }
        self.tx.send(ServerUpdate::WebPatch(patch)).unwrap_or(0)
    }

    /// Ask connected browser clients to load the freshly-built WASM bundle.
    /// Native clients ignore this text-only control message.
    pub fn reload_browser(&self) -> usize {
        self.tx.send(ServerUpdate::Reload).unwrap_or(0)
    }

    /// Number of clients currently subscribed.
    pub fn client_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// The runtime address of `whisker_aslr_anchor`
    /// (= `subsecond::aslr_reference()`) most recently reported by a
    /// connected client in its `hello` message. `None` when no
    /// client has connected or sent its `hello` yet — the patcher
    /// should withhold hot-reload patches in that case (fall back to
    /// full reload).
    pub fn latest_aslr_reference(&self) -> Option<u64> {
        self.aslr_reference.lock().ok().and_then(|g| *g)
    }
}

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<ServerUpdate>,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    aslr_reference: Arc<Mutex<Option<u64>>>,
    /// Session secret every client must prove; `None` treats clients as
    /// browsers (see [`crate::Config::dev_token`]).
    expected_token: Option<Arc<str>>,
    trace: bool,
    handshake_timeout: std::time::Duration,
    /// Last handshake rejection reported, so retries don't repeat it.
    last_rejection: Arc<Mutex<Option<String>>>,
    bound_addr: SocketAddr,
    /// `Host` authorities accepted besides `bound_addr`; see
    /// [`crate::Config::forwarded_authorities`]. Already emptied by
    /// [`serve`] when the bind is not loopback or unspecified.
    forwarded_authorities: Arc<[String]>,
}

#[derive(Debug, Clone)]
enum ServerUpdate {
    Patch(Patch),
    WebPatch(Patch),
    Reload,
}

pub struct StaticFiles {
    pub root: PathBuf,
    pub base_path: String,
}

/// Bind on `addr`, spawn the axum server on the current tokio
/// runtime, and return:
///   - a [`PatchSender`] for the rest of the dev loop to push patches
///   - the actual bound address (useful when caller asked for port 0)
///   - the spawned server task's `JoinHandle`
///
/// `on_event` is an optional observer hook — `whisker-cli` uses it to
/// render terminal UI on connect/disconnect events.
///
/// `forwarded_authorities` lists extra `Host` values to accept, as
/// described on [`crate::Config::forwarded_authorities`]; they are
/// ignored unless `addr` is a loopback or unspecified address.
pub async fn serve(
    addr: SocketAddr,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    expected_token: Option<String>,
    forwarded_authorities: Vec<String>,
    trace: bool,
    static_files: Option<StaticFiles>,
) -> Result<(PatchSender, SocketAddr, tokio::task::JoinHandle<()>)> {
    serve_with_handshake_timeout(
        addr,
        on_event,
        expected_token,
        forwarded_authorities,
        trace,
        static_files,
        protocol::HANDSHAKE_TIMEOUT,
    )
    .await
}

async fn serve_with_handshake_timeout(
    addr: SocketAddr,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    expected_token: Option<String>,
    forwarded_authorities: Vec<String>,
    trace: bool,
    static_files: Option<StaticFiles>,
    handshake_timeout: std::time::Duration,
) -> Result<(PatchSender, SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    let (tx, _rx) = broadcast::channel::<ServerUpdate>(16);
    let aslr_reference: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    // A loopback port forward (`adb reverse`) can only land on a loopback
    // or wildcard listener, so a LAN-IP bind gains nothing from the list.
    let forwarded_authorities = if bound.ip().is_loopback() || bound.ip().is_unspecified() {
        forwarded_authorities
    } else {
        Vec::new()
    };
    let state = AppState {
        tx: tx.clone(),
        on_event,
        aslr_reference: Arc::clone(&aslr_reference),
        expected_token: expected_token.map(Arc::from),
        trace,
        handshake_timeout,
        last_rejection: Arc::new(Mutex::new(None)),
        bound_addr: bound,
        forwarded_authorities: forwarded_authorities.into(),
    };

    let mut app = Router::new()
        .route("/whisker-dev", get(ws_handler))
        .with_state(state.clone());
    if let Some(StaticFiles { root, base_path }) = static_files {
        let files = ServeDir::new(root.clone()).fallback(ServeFile::new(root.join("index.html")));
        let prefix = base_path.trim_end_matches('/');
        if prefix.is_empty() {
            app = app.fallback_service(files);
        } else {
            app = app.nest_service(prefix, files).route(
                "/",
                get(move || std::future::ready(axum::response::Redirect::temporary(&base_path))),
            );
        }
    }

    let app = app.layer(axum::middleware::from_fn_with_state(
        state,
        validate_request,
    ));

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            whisker_build::ui::error(format!("axum serve error: {e}"));
        }
    });

    Ok((PatchSender { tx, aslr_reference }, bound, handle))
}

/// Port implied by an `http`/`ws` authority that omits one.
const DEFAULT_HTTP_PORT: u16 = 80;

fn valid_authority(value: &str, bound: SocketAddr, forwarded: &[String]) -> bool {
    use std::str::FromStr;
    let Ok(authority) = axum::http::uri::Authority::from_str(value) else {
        return false;
    };
    if value.contains('@') {
        return false;
    }
    // DNS rebinding works by getting a browser to send an attacker-owned
    // name in `Host`. Forwarded authorities are IP literals or
    // `localhost`, which no attacker can point at another address, and a
    // page can only be same-origin with them by being served from that
    // exact authority — which `valid_origin` still enforces.
    if forwarded
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(value))
    {
        return true;
    }
    // URL serialization drops the port only when it is the scheme default,
    // so a portless Host means port 80 rather than a malformed request.
    if authority.port_u16().unwrap_or(DEFAULT_HTTP_PORT) != bound.port() {
        return false;
    }
    let host = authority.host().trim_matches(['[', ']']);
    if host.eq_ignore_ascii_case("localhost") {
        return bound.ip().is_loopback() || bound.ip().is_unspecified();
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| bound.ip().is_unspecified() || ip == bound.ip())
}

/// Accepts an `Origin` only when it is the `http` origin of `host`, the
/// already-validated `Host` header. `https`/`wss` pages, opaque `null`
/// origins and any other authority are refused.
fn valid_origin(value: &str, host: &str) -> bool {
    value.parse::<axum::http::Uri>().is_ok_and(|uri| {
        uri.scheme_str() == Some("http")
            && uri.path() == "/"
            && uri.query().is_none()
            && uri
                .authority()
                .is_some_and(|authority| authority.as_str() == host)
    })
}

async fn validate_request(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use axum::http::header::{HOST, ORIGIN, UPGRADE};
    use axum::response::IntoResponse;
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok());
    let valid_host = host
        .is_some_and(|host| valid_authority(host, state.bound_addr, &state.forwarded_authorities));
    let origin = request.headers().get(ORIGIN);
    let origin_allowed = match (origin, host) {
        (None, _) => true,
        (Some(value), Some(host)) => value.to_str().is_ok_and(|value| valid_origin(value, host)),
        (Some(_), None) => false,
    };
    let websocket = request
        .headers()
        .get(UPGRADE)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
    if !valid_host
        || !origin_allowed
        || (websocket && state.expected_token.is_none() && origin.is_none())
    {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    // Clients never send patches, so one control message plus its envelope
    // bounds every read, before and after authentication.
    let limit = protocol::MAX_CONTROL + protocol::ENVELOPE_OVERHEAD;
    ws.max_frame_size(limit)
        .max_message_size(limit)
        .on_upgrade(move |socket| handle_socket(socket, state))
}

/// Next text message during the handshake, skipping control frames.
async fn next_text(socket: &mut WebSocket) -> Result<String> {
    use futures_util::StreamExt;
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => return Ok(text.to_string()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Ok(Message::Binary(_))) => {
                anyhow::bail!("the client sent a binary message during authentication")
            }
            Some(Ok(Message::Close(_))) | None => anyhow::bail!(
                "the client closed the connection during authentication (usually an app \
                 launched by a previous `whisker run` session; it reconnects after the next \
                 launch or Full Reload (R))"
            ),
            Some(Err(error)) => return Err(error.into()),
        }
    }
}

async fn authenticate(socket: &mut WebSocket, secret: &str) -> Result<Session> {
    let challenge = next_text(socket).await?;
    let (handshake, proof) = ServerHandshake::respond(secret, &challenge)?;
    socket.send(Message::Text(proof.into())).await?;
    // Send nothing else until the client proof verifies: the runtime caps
    // its pre-authentication reads and lifts the cap without expecting
    // any server bytes buffered past this proof.
    Ok(handshake.finish(&next_text(socket).await?)?)
}

/// Connected client, decided once by the handshake.
enum Peer {
    /// Authenticated app: every message is an envelope, native patches allowed.
    Native(Session),
    /// Unauthenticated browser on a secret-less server: plain messages,
    /// validated WebAssembly patches only.
    Browser,
}

async fn send_frame(socket: &mut WebSocket, peer: &mut Peer, message: Message) -> Result<()> {
    let message = match (peer, message) {
        (Peer::Native(session), Message::Text(text)) => {
            Message::Binary(session.seal(FrameKind::Text, text.as_bytes())?.into())
        }
        (Peer::Native(session), Message::Binary(bytes)) => {
            Message::Binary(session.seal(FrameKind::Patch, &bytes)?.into())
        }
        (_, message) => message,
    };
    socket.send(message).await?;
    Ok(())
}

/// Warns about rejected handshakes without repeating the same reason.
///
/// An app from a previous session retries every 2 s for as long as it
/// runs; one warning per distinct reason keeps the TUI readable.
fn report_rejection(state: &AppState, reason: String) {
    let mut last = state
        .last_rejection
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if last.as_deref() != Some(reason.as_str()) {
        whisker_build::ui::warn(format!("hot-reload client rejected: {reason}"));
        *last = Some(reason);
    }
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    use futures_util::StreamExt;
    let mut peer = match &state.expected_token {
        Some(secret) => {
            match tokio::time::timeout(state.handshake_timeout, authenticate(&mut socket, secret))
                .await
            {
                Ok(Ok(session)) => {
                    if let Ok(mut last) = state.last_rejection.lock() {
                        *last = None;
                    }
                    Peer::Native(session)
                }
                Ok(Err(error)) => return report_rejection(&state, format!("{error:#}")),
                Err(_) => {
                    return report_rejection(
                        &state,
                        format!(
                            "authentication did not finish within {:?}",
                            state.handshake_timeout
                        ),
                    );
                }
            }
        }
        None => Peer::Browser,
    };
    let native = matches!(peer, Peer::Native(_));
    let mut receiver = state.tx.subscribe();
    if let Some(callback) = &state.on_event {
        callback(Event::ClientConnected);
    }
    if state.trace
        && native
        && send_frame(&mut socket, &mut peer, Message::Text(TRACE_ON.into()))
            .await
            .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            update = receiver.recv() => {
                let update = match update {
                    Ok(update) => update,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let message = match (update, native) {
                    (ServerUpdate::Patch(patch), true) | (ServerUpdate::WebPatch(patch), false) => {
                        match encode_patch_frame(&patch) {
                            Ok(frame) => Message::Binary(frame.into()),
                            Err(error) => {
                                whisker_build::ui::warn(format!("{error:#}; use Full Reload (R)"));
                                continue;
                            }
                        }
                    }
                    (ServerUpdate::Patch(_) | ServerUpdate::WebPatch(_), _) => continue,
                    (ServerUpdate::Reload, _) => Message::Text(r#"{"kind":"reload"}"#.into()),
                };
                if send_frame(&mut socket, &mut peer, message).await.is_err() { break; }
            }
            message = socket.next() => {
                let text = match (message, &mut peer) {
                    (Some(Ok(Message::Close(_)) | Err(_)) | None, _) => break,
                    (Some(Ok(Message::Ping(_) | Message::Pong(_))), _) => continue,
                    (Some(Ok(Message::Binary(frame))), Peer::Native(session)) => {
                        match session.open(&frame) {
                            Ok((FrameKind::Text, payload)) => match std::str::from_utf8(payload) {
                                Ok(text) => text.to_owned(),
                                Err(_) => {
                                    whisker_build::ui::warn("closing hot-reload client: control message is not UTF-8");
                                    break;
                                }
                            },
                            Ok((FrameKind::Patch, _)) => {
                                whisker_build::ui::warn("closing hot-reload client: clients may not send patches");
                                break;
                            }
                            Err(error) => {
                                whisker_build::ui::warn(format!("closing hot-reload client: {error}"));
                                break;
                            }
                        }
                    }
                    (Some(Ok(Message::Text(text))), Peer::Browser) => text.to_string(),
                    (Some(Ok(_)), _) => break,
                };
                if let Some(hello) = parse_client_hello(&text) {
                    // A browser peer must not choose the slide native patches are built against.
                    let aslr = if native { hello.aslr_reference } else { BROWSER_ASLR_REFERENCE };
                    if let Ok(mut reference) = state.aslr_reference.lock() { *reference = Some(aslr); }
                } else if let Some(log) = parse_client_log(&text) {
                    if let Some(callback) = &state.on_event { callback(Event::DeviceLog { stream: log.stream, line: log.line, ts_micros: log.ts_micros }); }
                } else if native {
                    if let Some(frames) = parse_client_trace(&text) {
                        if let Some(callback) = &state.on_event { callback(Event::DeviceTrace { frames }); }
                    }
                }
            }
        }
    }
    // The stored slide belongs to the now-dead native process; stamping
    // jump stubs with it for the replacement would crash the device.
    if native {
        if let Ok(mut reference) = state.aslr_reference.lock() {
            *reference = None;
        }
    }
    if let Some(callback) = &state.on_event {
        callback(Event::ClientDisconnected);
    }
}

/// Build the on-the-wire binary frame:
///   `[u64 BE json_len][json header][raw dylib bytes]`
///
/// Fails when the frame cannot fit one envelope; such a patch needs a Full
/// Reload instead.
fn encode_patch_frame(patch: &Patch) -> Result<Vec<u8>> {
    let header = PatchHeader::Patch {
        table: &patch.table,
    };
    let json = serde_json::to_vec(&header)?;
    let dylib = patch.dylib_bytes.as_slice();
    let len = 8 + json.len() + dylib.len();
    anyhow::ensure!(
        json.len() <= protocol::MAX_CONTROL && len <= protocol::MAX_PAYLOAD,
        "hot-reload patch is {len} bytes, over the {} byte transport limit",
        protocol::MAX_PAYLOAD
    );
    let json_len = json.len() as u64;
    let mut frame = Vec::with_capacity(len);
    frame.extend_from_slice(&json_len.to_be_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(dylib);
    Ok(frame)
}

/// A decoded `{"kind":"hello",…}` message, sent once after the handshake.
struct ClientHello {
    aslr_reference: u64,
}

/// Parse a client hello. Returns `None` for other client messages
/// (logs, traces) or malformed payloads.
fn parse_client_hello(text: &str) -> Option<ClientHello> {
    #[derive(serde::Deserialize)]
    struct Hello {
        kind: String,
        aslr_reference: u64,
    }
    let h: Hello = serde_json::from_str(text).ok()?;
    if h.kind == "hello" {
        Some(ClientHello {
            aslr_reference: h.aslr_reference,
        })
    } else {
        None
    }
}

/// `aslr_reference` recorded for a browser-mode `hello`; the generated
/// browser bootstrap also sends `0`.
const BROWSER_ASLR_REFERENCE: u64 = 0;

const TRACE_ON: &str = r#"{"kind":"trace","enabled":true}"#;

/// Decodes `whisker-dev-runtime`'s `trace::encode` tuples.
fn parse_client_trace(text: &str) -> Option<Vec<crate::FrameSample>> {
    #[derive(serde::Deserialize)]
    struct Trace {
        kind: String,
        frames: Vec<[u64; 7]>,
    }
    let trace: Trace = serde_json::from_str(text).ok()?;
    (trace.kind == "trace").then(|| {
        trace
            .frames
            .into_iter()
            .map(
                |[
                    total_us,
                    reactive_us,
                    layout_us,
                    measure_us,
                    present_us,
                    measure_batches,
                    measure_requests,
                ]| {
                    crate::FrameSample {
                        total_us,
                        reactive_us,
                        layout_us,
                        measure_us,
                        present_us,
                        measure_batches,
                        measure_requests,
                    }
                },
            )
            .collect()
    })
}

/// Decoded payload of a `{"kind":"log",…}` text frame emitted by the
/// device-side `log_capture` module.
struct ClientLog {
    stream: String,
    line: String,
    ts_micros: u128,
}

/// Parse a client log message. Returns `None` for any other message
/// so the caller can fall through to the hello and trace handlers.
///
/// `ts_micros` arrives as a string on the wire because `u128` doesn't
/// round-trip through JSON's number type cleanly (>2^53 is lossy in
/// most decoders). The device serializes via `to_string`; we decode
/// with `parse`, defaulting to `0` on parse failure rather than
/// rejecting the whole frame — the line itself is more valuable than
/// a precise timestamp.
fn parse_client_log(text: &str) -> Option<ClientLog> {
    #[derive(serde::Deserialize)]
    struct Log {
        kind: String,
        stream: String,
        line: String,
        #[serde(default)]
        ts_micros: Option<String>,
    }
    let h: Log = serde_json::from_str(text).ok()?;
    if h.kind != "log" {
        return None;
    }
    let ts_micros = h
        .ts_micros
        .as_deref()
        .and_then(|s| s.parse::<u128>().ok())
        .unwrap_or(0);
    Some(ClientLog {
        stream: strip_controls(&h.stream, false),
        line: strip_controls(&h.line, true),
        ts_micros,
    })
}

/// Removes characters that let a device steer the host terminal.
///
/// `char::is_control` covers C0, DEL and C1 (which includes the 8-bit CSI
/// `U+009B` and `NEL` `U+0085`). Bidirectional formatting characters are
/// not controls in that sense but can reorder what the terminal shows, so
/// they go too. `keep_tab` preserves `\t` for log lines.
fn strip_controls(text: &str, keep_tab: bool) -> String {
    text.chars()
        .filter(|&character| {
            (keep_tab && character == '\t')
                || !(character.is_control() || is_bidi_format(character))
        })
        .collect()
}

/// Bidirectional marks, embeddings, overrides and isolates.
fn is_bidi_format(character: char) -> bool {
    matches!(
        character,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use whisker_dev_protocol::ClientHandshake;

    fn make_dummy_jump_table() -> subsecond_types::JumpTable {
        // Construct via JSON to avoid pinning ourselves to private
        // field shapes. All fields are public + plain types so this
        // is stable.
        let json = r#"{
            "lib": "/tmp/dummy.dylib",
            "map": {},
            "aslr_reference": 4294967296,
            "new_base_address": 8589934592,
            "ifunc_count": 0
        }"#;
        serde_json::from_str(json).expect("dummy JumpTable")
    }

    /// Spawn the server on an ephemeral port and return its address +
    /// sender so tests don't have to worry about port collisions.
    async fn spawn_test_server(
        on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    ) -> (PatchSender, SocketAddr) {
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (sender, addr, _handle) = serve(
            any,
            on_event,
            Some("s3kret".into()),
            Vec::new(),
            false,
            None,
        )
        .await
        .expect("serve");
        (sender, addr)
    }

    type TestSocket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;
    struct TestClient {
        socket: TestSocket,
        session: Session,
    }

    /// Next text message during the handshake, skipping control frames.
    async fn next_handshake_text(socket: &mut TestSocket) -> Option<String> {
        use tokio_tungstenite::tungstenite::Message as TMsg;
        loop {
            match socket.next().await? {
                Ok(TMsg::Text(text)) => return Some(text),
                Ok(TMsg::Ping(_) | TMsg::Pong(_)) => {}
                _ => return None,
            }
        }
    }

    impl TestClient {
        async fn authenticate(mut socket: TestSocket) -> Self {
            use tokio_tungstenite::tungstenite::Message as TMsg;
            let (handshake, challenge) = ClientHandshake::start("s3kret").unwrap();
            socket.send(TMsg::Text(challenge)).await.unwrap();
            let server_proof = next_handshake_text(&mut socket)
                .await
                .expect("server proof");
            let (session, client_proof) = handshake.finish(&server_proof).unwrap();
            socket.send(TMsg::Text(client_proof)).await.unwrap();
            Self { socket, session }
        }

        async fn send(
            &mut self,
            message: tokio_tungstenite::tungstenite::Message,
        ) -> Result<(), tokio_tungstenite::tungstenite::Error> {
            use tokio_tungstenite::tungstenite::Message as TMsg;
            let message = match message {
                TMsg::Text(text) => {
                    TMsg::Binary(self.session.seal(FrameKind::Text, text.as_bytes()).unwrap())
                }
                message => message,
            };
            self.socket.send(message).await
        }

        async fn next(
            &mut self,
        ) -> Option<
            Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>,
        > {
            use tokio_tungstenite::tungstenite::{Error, Message as TMsg};
            let message = match self.socket.next().await? {
                Ok(message) => message,
                Err(error) => return Some(Err(error)),
            };
            Some(match message {
                TMsg::Binary(bytes) => match self.session.open(&bytes) {
                    Ok((FrameKind::Patch, bytes)) => Ok(TMsg::Binary(bytes.to_vec())),
                    Ok((FrameKind::Text, bytes)) => {
                        Ok(TMsg::Text(String::from_utf8(bytes.to_vec()).unwrap()))
                    }
                    Err(error) => Err(Error::Io(std::io::Error::other(error))),
                },
                message => Ok(message),
            })
        }
    }

    async fn raw_connect(addr: SocketAddr) -> TestSocket {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/whisker-dev"))
            .await
            .unwrap();
        socket
    }

    fn browser_request(url: &str) -> tokio_tungstenite::tungstenite::http::Request<()> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = url.into_client_request().unwrap();
        let origin = url
            .replace("ws://", "http://")
            .trim_end_matches("/whisker-dev")
            .to_owned();
        request
            .headers_mut()
            .insert("Origin", origin.parse().unwrap());
        request
    }

    async fn browser_connect(addr: SocketAddr) -> TestSocket {
        let url = format!("ws://{addr}/whisker-dev");
        let (socket, _) = tokio_tungstenite::connect_async(browser_request(&url))
            .await
            .unwrap();
        socket
    }

    async fn connect(addr: SocketAddr) -> TestClient {
        TestClient::authenticate(raw_connect(addr).await).await
    }

    /// Decode a wire frame back into (header JSON value, dylib bytes)
    /// so tests can assert against both halves.
    fn decode_patch_frame(bytes: &[u8]) -> (serde_json::Value, Vec<u8>) {
        assert!(bytes.len() >= 8, "frame too short");
        let json_len = u64::from_be_bytes(bytes[..8].try_into().unwrap()) as usize;
        assert!(bytes.len() >= 8 + json_len, "frame truncated");
        let header: serde_json::Value =
            serde_json::from_slice(&bytes[8..8 + json_len]).expect("parse header");
        let dylib = bytes[8 + json_len..].to_vec();
        (header, dylib)
    }

    #[tokio::test]
    async fn client_can_connect_and_receive_a_broadcast_patch() {
        let (sender, addr) = spawn_test_server(None).await;
        let mut client = connect(addr).await;

        // Wait for the server to register the subscription before we
        // send. Polling client_count is the cheap, deterministic way.
        for _ in 0..100 {
            if sender.client_count() > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.client_count(), 1);

        let table = make_dummy_jump_table();
        let n = sender.send(Patch {
            table: table.clone(),
            dylib_bytes: Arc::new(b"FAKE_DYLIB_BYTES".to_vec()),
        });
        assert_eq!(n, 1);

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
            .await
            .expect("recv timed out")
            .expect("stream ended")
            .expect("ws error");
        let bytes = match msg {
            tokio_tungstenite::tungstenite::Message::Binary(b) => b,
            other => panic!("expected binary, got {other:?}"),
        };
        let (header, dylib) = decode_patch_frame(&bytes);
        assert_eq!(header["kind"], "patch");
        assert_eq!(header["table"]["lib"], "/tmp/dummy.dylib");
        assert_eq!(header["table"]["aslr_reference"], 4294967296_u64);
        assert_eq!(dylib, b"FAKE_DYLIB_BYTES");
    }

    #[tokio::test]
    async fn browser_reload_is_sent_as_a_text_control_message() {
        let (sender, addr) = spawn_test_server(None).await;
        let mut client = connect(addr).await;
        for _ in 0..100 {
            if sender.client_count() > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        assert_eq!(sender.reload_browser(), 1);
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
            .await
            .expect("recv timed out")
            .expect("stream ended")
            .expect("ws error");
        assert_eq!(
            message,
            tokio_tungstenite::tungstenite::Message::Text(r#"{"kind":"reload"}"#.into())
        );
    }

    #[tokio::test]
    async fn static_root_serves_index_and_browser_assets() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let root = std::env::temp_dir().join(format!(
            "whisker-web-static-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), b"INDEX").unwrap();
        std::fs::write(root.join("whisker_app.js"), b"JAVASCRIPT").unwrap();
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (_sender, addr, _handle) = serve(
            any,
            None,
            None,
            Vec::new(),
            false,
            Some(StaticFiles {
                root: root.clone(),
                base_path: "/".into(),
            }),
        )
        .await
        .expect("serve");

        async fn get(addr: SocketAddr, path: &str) -> String {
            let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
            stream
                .write_all(
                    format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            String::from_utf8(response).unwrap()
        }

        assert!(get(addr, "/").await.ends_with("INDEX"));
        assert!(get(addr, "/detail/42").await.ends_with("INDEX"));
        assert!(get(addr, "/whisker_app.js").await.ends_with("JAVASCRIPT"));
        let (_sender, nested_addr, nested_handle) = serve(
            any,
            None,
            None,
            Vec::new(),
            false,
            Some(StaticFiles {
                root: root.clone(),
                base_path: "/examples/chat/".into(),
            }),
        )
        .await
        .unwrap();
        assert!(
            get(nested_addr, "/")
                .await
                .contains("location: /examples/chat/")
        );
        assert!(get(nested_addr, "/examples/chat/").await.ends_with("INDEX"));
        assert!(
            get(nested_addr, "/examples/chat/settings/connection")
                .await
                .ends_with("INDEX")
        );
        assert!(
            get(nested_addr, "/examples/chat/whisker_app.js")
                .await
                .ends_with("JAVASCRIPT")
        );
        assert!(
            get(nested_addr, "/whisker_app.js")
                .await
                .starts_with("HTTP/1.1 404")
        );
        nested_handle.abort();
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn a_large_patch_reaches_a_client_with_explicit_finite_limits() {
        use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

        let (sender, addr) = spawn_test_server(None).await;
        // Mirror the device receiver's post-authentication read limits
        // (`connect_and_serve` in whisker-dev-runtime's `hot_reload`).
        let config = WebSocketConfig {
            max_frame_size: Some(protocol::MAX_MESSAGE),
            max_message_size: Some(protocol::MAX_MESSAGE),
            ..Default::default()
        };
        let url = format!("ws://{addr}/whisker-dev");
        let (unlimited_socket, _) =
            tokio_tungstenite::connect_async_with_config(&url, Some(config), false)
                .await
                .expect("connect");
        let mut unlimited = TestClient::authenticate(unlimited_socket).await;
        let mut default_limits = connect(addr).await;

        for _ in 0..100 {
            if sender.client_count() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.client_count(), 2);

        // 17 MiB — over tungstenite's 16 MiB default max_frame_size,
        // the size class a large app's subsecond patch lands in.
        let dylib = vec![0xABu8; 17 * 1024 * 1024];
        let n = sender.send(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(dylib.clone()),
        });
        assert_eq!(n, 2);

        let msg = tokio::time::timeout(std::time::Duration::from_secs(45), unlimited.next())
            .await
            .expect("recv timed out")
            .expect("stream ended")
            .expect("ws error");
        let bytes = match msg {
            tokio_tungstenite::tungstenite::Message::Binary(b) => b,
            other => panic!("expected binary, got a {other:?} frame"),
        };
        let (header, got) = decode_patch_frame(&bytes);
        assert_eq!(header["kind"], "patch");
        assert_eq!(got, dylib);

        // A client with tungstenite's default read limits (the old
        // device behavior) rejects the same frame as too long.
        let rejected = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            loop {
                match default_limits.next().await {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_))) => return false,
                    Some(Err(_)) | None => return true,
                    _ => continue,
                }
            }
        })
        .await
        .expect("default-limit client neither errored nor received");
        assert!(
            rejected,
            "a 17 MiB frame must exceed the default 16 MiB read limit"
        );
    }

    async fn spawn_test_server_with_token(token: Option<String>) -> (PatchSender, SocketAddr) {
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (sender, addr, _handle) = serve(any, None, token, Vec::new(), false, None)
            .await
            .expect("serve");
        (sender, addr)
    }

    /// A native client's handshake: no `Origin`, and the `Host` the
    /// device dialled, which differs from `addr` behind `adb reverse`.
    fn native_request(
        addr: SocketAddr,
        host: &str,
    ) -> tokio_tungstenite::tungstenite::http::Request<()> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://{addr}/whisker-dev")
            .into_client_request()
            .unwrap();
        request.headers_mut().insert("Host", host.parse().unwrap());
        request
    }

    fn assert_forbidden<T: std::fmt::Debug>(
        result: Result<T, tokio_tungstenite::tungstenite::Error>,
    ) {
        match result {
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                assert_eq!(response.status().as_u16(), 403);
            }
            other => panic!("expected 403, got {other:?}"),
        }
    }

    /// Sends an authenticated `hello` and waits until the server records its `aslr_reference`.
    async fn send_hello(client: &mut TestClient, sender: &PatchSender) {
        client
            .send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"kind":"hello","aslr_reference":4294967296}"#.into(),
            ))
            .await
            .expect("send hello");
        for _ in 0..200 {
            if sender.latest_aslr_reference().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.latest_aslr_reference(), Some(0x1_0000_0000));
    }

    #[tokio::test]
    async fn android_client_behind_adb_reverse_is_accepted_with_its_forwarded_host() {
        let (sender, addr, _handle) = serve(
            "127.0.0.1:0".parse().unwrap(),
            None,
            Some("s3kret".into()),
            crate::android_forwarded_authorities(),
            false,
            None,
        )
        .await
        .expect("serve");
        let device_host = format!("127.0.0.1:{}", crate::ANDROID_DEVICE_PORT);
        let (socket, _) = tokio_tungstenite::connect_async(native_request(addr, &device_host))
            .await
            .expect("forwarded authority accepted");
        let mut client = TestClient::authenticate(socket).await;
        send_hello(&mut client, &sender).await;

        assert_eq!(
            sender.send(Patch {
                table: make_dummy_jump_table(),
                dylib_bytes: Arc::new(b"OK".to_vec()),
            }),
            1
        );
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
            .await
            .expect("recv timed out")
            .expect("stream ended")
            .expect("ws error");
        assert!(matches!(
            message,
            tokio_tungstenite::tungstenite::Message::Binary(_)
        ));
    }

    #[tokio::test]
    async fn native_client_needs_a_forwarded_authority_for_a_foreign_host() {
        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let device_host = format!("127.0.0.1:{}", crate::ANDROID_DEVICE_PORT);
        assert_forbidden(
            tokio_tungstenite::connect_async(native_request(addr, &device_host)).await,
        );

        // The same originless client is accepted under the bound authority.
        let (socket, _) = tokio_tungstenite::connect_async(native_request(addr, &addr.to_string()))
            .await
            .expect("bound authority accepted");
        send_hello(&mut TestClient::authenticate(socket).await, &sender).await;
    }

    #[tokio::test]
    async fn client_with_valid_secret_is_armed_and_receives_patches() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = connect(addr).await;

        client
            .send(TMsg::Text(
                r#"{"kind":"hello","aslr_reference":4294967296}"#.into(),
            ))
            .await
            .expect("send hello");

        // The hello is only accepted inside an authenticated envelope, so
        // the recorded aslr_reference proves the session is armed.
        for _ in 0..200 {
            if sender.latest_aslr_reference().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.latest_aslr_reference(), Some(0x1_0000_0000));

        let n = sender.send(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(b"OK".to_vec()),
        });
        assert_eq!(n, 1);

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
            .await
            .expect("recv timed out")
            .expect("stream ended")
            .expect("ws error");
        assert!(
            matches!(msg, TMsg::Binary(_)),
            "authed client should receive the patch frame"
        );
    }

    /// Wait for the server to drop `socket`, failing if any binary
    /// (patch-carrying) message reaches it first.
    async fn expect_disconnect(socket: &mut TestSocket) {
        use tokio_tungstenite::tungstenite::Message as TMsg;
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match socket.next().await {
                    Some(Ok(TMsg::Binary(_))) => panic!("server sent data to a rejected client"),
                    None | Some(Ok(TMsg::Close(_))) | Some(Err(_)) => return,
                    _ => continue,
                }
            }
        })
        .await
        .expect("disconnect timed out");
    }

    #[tokio::test]
    async fn legacy_token_hello_is_disconnected_and_gets_no_patch() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = raw_connect(addr).await;

        // The pre-handshake protocol sent the secret in a plain hello. It
        // is not a challenge, so the server closes without arming the
        // patch path.
        client
            .send(TMsg::Text(
                r#"{"kind":"hello","aslr_reference":1,"token":"s3kret"}"#.into(),
            ))
            .await
            .expect("send hello");
        expect_disconnect(&mut client).await;
        assert_eq!(sender.client_count(), 0);
        assert_eq!(sender.latest_aslr_reference(), None);
    }

    #[tokio::test]
    async fn forged_client_proof_is_disconnected() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = raw_connect(addr).await;
        let (_, challenge) = ClientHandshake::start("other secret").unwrap();
        client.send(TMsg::Text(challenge)).await.unwrap();
        next_handshake_text(&mut client)
            .await
            .expect("server proof");
        let forged = format!(r#"{{"mac":{:?}}}"#, [0u8; 32]);
        client.send(TMsg::Text(forged)).await.unwrap();
        expect_disconnect(&mut client).await;
        assert_eq!(sender.client_count(), 0);
    }

    #[tokio::test]
    async fn ping_during_the_handshake_is_skipped() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut socket = raw_connect(addr).await;
        socket.send(TMsg::Ping(vec![1])).await.unwrap();
        let mut client = TestClient::authenticate(socket).await;
        client
            .send(TMsg::Text(r#"{"kind":"hello","aslr_reference":7}"#.into()))
            .await
            .unwrap();
        for _ in 0..200 {
            if sender.latest_aslr_reference().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.latest_aslr_reference(), Some(7));
    }

    #[tokio::test]
    async fn silent_client_times_out_the_handshake() {
        let (sender, addr, _handle) = serve_with_handshake_timeout(
            "127.0.0.1:0".parse().unwrap(),
            None,
            Some("s3kret".into()),
            Vec::new(),
            false,
            None,
            std::time::Duration::from_millis(50),
        )
        .await
        .unwrap();
        let mut client = raw_connect(addr).await;
        expect_disconnect(&mut client).await;
        assert_eq!(sender.client_count(), 0);
    }

    #[tokio::test]
    async fn modified_client_frame_closes_the_connection() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = connect(addr).await;
        let mut frame = client
            .session
            .seal(FrameKind::Text, br#"{"kind":"hello","aslr_reference":1}"#)
            .unwrap();
        *frame.last_mut().unwrap() ^= 1;
        client.socket.send(TMsg::Binary(frame)).await.unwrap();
        expect_disconnect(&mut client.socket).await;
        assert_eq!(sender.latest_aslr_reference(), None);
    }

    #[tokio::test]
    async fn replayed_client_frame_closes_the_connection() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let logs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&logs);
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
            if matches!(event, Event::DeviceLog { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let (_sender, addr) = spawn_test_server(Some(on_event)).await;
        let mut client = connect(addr).await;
        let frame = client
            .session
            .seal(
                FrameKind::Text,
                br#"{"kind":"log","stream":"stdout","line":"once"}"#,
            )
            .unwrap();
        client
            .socket
            .send(TMsg::Binary(frame.clone()))
            .await
            .unwrap();
        client.socket.send(TMsg::Binary(frame)).await.unwrap();
        expect_disconnect(&mut client.socket).await;
        assert_eq!(logs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn oversized_client_frame_closes_the_connection() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (_sender, addr) = spawn_test_server(None).await;
        let mut client = connect(addr).await;
        let oversized = vec![0; protocol::MAX_CONTROL + protocol::ENVELOPE_OVERHEAD + 1];
        // The server may hang up mid-write once it sees the length.
        let _ = client.socket.send(TMsg::Binary(oversized)).await;
        expect_disconnect(&mut client.socket).await;
    }

    #[tokio::test]
    async fn browser_only_sessions_never_receive_native_patch_bytes() {
        let (sender, addr) = spawn_test_server_with_token(None).await;
        let mut client = browser_connect(addr).await;
        for _ in 0..100 {
            if sender.client_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        sender.send(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(b"secret library".to_vec()),
        });
        sender.reload_browser();
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            message,
            tokio_tungstenite::tungstenite::Message::Text(r#"{"kind":"reload"}"#.into())
        );
    }

    #[tokio::test]
    async fn send_with_no_clients_returns_zero_and_does_not_error() {
        let (sender, _addr) = spawn_test_server(None).await;
        assert_eq!(sender.client_count(), 0);
        let n = sender.send(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(Vec::new()),
        });
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn multiple_clients_each_receive_the_same_patch() {
        let (sender, addr) = spawn_test_server(None).await;
        let mut a = connect(addr).await;
        let mut b = connect(addr).await;

        for _ in 0..100 {
            if sender.client_count() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.client_count(), 2);

        let n = sender.send(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(b"SHARED".to_vec()),
        });
        assert_eq!(n, 2);

        for client in [&mut a, &mut b] {
            let msg = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
                .await
                .expect("timeout")
                .expect("stream end")
                .expect("ws err");
            assert!(matches!(
                msg,
                tokio_tungstenite::tungstenite::Message::Binary(_)
            ));
        }
    }

    #[tokio::test]
    async fn on_event_callback_fires_for_connect_and_disconnect() {
        let connect_count = Arc::new(AtomicUsize::new(0));
        let disconnect_count = Arc::new(AtomicUsize::new(0));

        let cc = connect_count.clone();
        let dc = disconnect_count.clone();
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |e| match e {
            Event::ClientConnected => {
                cc.fetch_add(1, Ordering::SeqCst);
            }
            Event::ClientDisconnected => {
                dc.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        });

        let (sender, addr) = spawn_test_server(Some(on_event)).await;

        let mut client = connect(addr).await;
        // Wait for connect callback.
        for _ in 0..100 {
            if connect_count.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(connect_count.load(Ordering::SeqCst), 1);

        // Close the client.
        client
            .send(tokio_tungstenite::tungstenite::Message::Close(None))
            .await
            .expect("send close");
        drop(client);

        // Wait for disconnect callback.
        for _ in 0..200 {
            if disconnect_count.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(disconnect_count.load(Ordering::SeqCst), 1);

        // sender stays usable across the whole flow.
        assert_eq!(sender.client_count(), 0);
    }

    #[tokio::test]
    async fn browser_hello_registers_wasm_relocation_base() {
        use tokio_tungstenite::tungstenite::Message as TMsg;
        let (sender, addr) = spawn_test_server_with_token(None).await;
        let mut browser = browser_connect(addr).await;
        browser
            .send(TMsg::Text(r#"{"kind":"hello","aslr_reference":0}"#.into()))
            .await
            .unwrap();
        for _ in 0..100 {
            if sender.latest_aslr_reference().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.latest_aslr_reference(), Some(0));
    }

    #[tokio::test]
    async fn browser_receives_only_validated_wasm_patches() {
        use tokio_tungstenite::tungstenite::Message as TMsg;
        let (sender, addr) = spawn_test_server_with_token(None).await;
        let mut browser = browser_connect(addr).await;
        for _ in 0..100 {
            if sender.client_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let patch = |bytes: Vec<u8>| Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(bytes),
        };
        let wasm = b"\0asm\x01\0\0\0".to_vec();
        sender.send(patch(b"native library".to_vec()));
        assert_eq!(sender.send_web(patch(b"native library".to_vec())), 0);
        assert_eq!(sender.send_web(patch(wasm.clone())), 1);
        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), browser.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let TMsg::Binary(bytes) = frame else {
            panic!("missing Wasm patch")
        };
        assert_eq!(decode_patch_frame(&bytes).1, wasm);
    }

    #[tokio::test]
    async fn native_sessions_never_receive_browser_wasm_modules() {
        let (sender, addr) = spawn_test_server(None).await;
        let mut native = connect(addr).await;
        for _ in 0..100 {
            if sender.client_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        sender.send_web(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(b"\0asm\x01\0\0\0".to_vec()),
        });
        sender.send(Patch {
            table: make_dummy_jump_table(),
            dylib_bytes: Arc::new(b"native library".to_vec()),
        });
        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), native.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let tokio_tungstenite::tungstenite::Message::Binary(bytes) = frame else {
            panic!("missing native patch")
        };
        assert_eq!(decode_patch_frame(&bytes).1, b"native library");
    }

    #[test]
    fn device_logs_cannot_execute_terminal_control_sequences() {
        let text = serde_json::json!({"kind":"log", "stream":"stdout", "line":"\u{1b}]52;c;payload\u{7}\rline"}).to_string();
        let log = parse_client_log(&text).unwrap();
        assert!(!log.line.chars().any(char::is_control));
    }

    #[test]
    fn device_logs_drop_c1_and_bidi_controls_but_keep_tabs_in_the_line() {
        let text = serde_json::json!({
            "kind": "log",
            "stream": "std\u{9b}out\t\u{85}\u{202e}",
            "line": "a\tb\u{9b}31m\u{85}c\u{202e}d\u{2066}e\u{2069}\u{200e}\u{200f}\u{61c}f\u{7f}",
        })
        .to_string();
        let log = parse_client_log(&text).unwrap();
        assert_eq!(log.stream, "stdout");
        assert_eq!(log.line, "a\tb31mcdef");
    }

    #[test]
    fn host_policy_rejects_rebinding_names_and_wrong_ports() {
        let bound = "127.0.0.1:9876".parse().unwrap();
        assert!(valid_authority("127.0.0.1:9876", bound, &[]));
        assert!(valid_authority("localhost:9876", bound, &[]));
        assert!(!valid_authority("evil.test:9876", bound, &[]));
        assert!(!valid_authority("mac.local:9876", bound, &[]));
        assert!(!valid_authority("127.0.0.1:9877", bound, &[]));
        assert!(!valid_authority("user@127.0.0.1:9876", bound, &[]));

        let forwarded = crate::android_forwarded_authorities();
        let tunnelled = "127.0.0.1:1234".parse().unwrap();
        assert!(valid_authority("127.0.0.1:9876", tunnelled, &forwarded));
        assert!(valid_authority("LOCALHOST:9876", tunnelled, &forwarded));
        assert!(!valid_authority("127.0.0.1:9876", tunnelled, &[]));
        assert!(!valid_authority("evil.test:9876", tunnelled, &forwarded));
    }

    #[test]
    fn portless_host_means_the_default_http_port() {
        let bound = "127.0.0.1:80".parse().unwrap();
        assert!(valid_authority("127.0.0.1", bound, &[]));
        assert!(!valid_authority(
            "127.0.0.1",
            "127.0.0.1:9876".parse().unwrap(),
            &[]
        ));
    }

    #[test]
    fn origin_policy_accepts_only_the_http_origin_of_the_host() {
        let host = "127.0.0.1:9876";
        assert!(valid_origin("http://127.0.0.1:9876", host));
        assert!(!valid_origin("https://127.0.0.1:9876", host));
        assert!(!valid_origin("ws://127.0.0.1:9876", host));
        assert!(!valid_origin("http://localhost:9876", host));
        assert!(!valid_origin("http://127.0.0.1:9877", host));
        assert!(!valid_origin("http://127.0.0.1:9876/app", host));
        assert!(!valid_origin("http://127.0.0.1:9876/?q", host));
        assert!(!valid_origin("null", host));
        assert!(!valid_origin("", host));
    }

    #[tokio::test]
    async fn requests_with_a_foreign_host_are_refused_before_any_handler() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let root = std::env::temp_dir().join(format!(
            "whisker-web-host-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), b"INDEX").unwrap();
        let (_sender, addr, handle) = serve(
            "127.0.0.1:0".parse().unwrap(),
            None,
            Some("s3kret".into()),
            Vec::new(),
            false,
            Some(StaticFiles {
                root: root.clone(),
                base_path: "/".into(),
            }),
        )
        .await
        .expect("serve");

        async fn status_line(addr: SocketAddr, host: &str) -> String {
            let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
            stream
                .write_all(
                    format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            let response = String::from_utf8(response).unwrap();
            response.lines().next().unwrap_or_default().to_owned()
        }

        assert_eq!(
            status_line(addr, &addr.to_string()).await,
            "HTTP/1.1 200 OK"
        );
        let rebinding_host = format!("evil.test:{}", addr.port());
        let mdns_host = format!("mac.local:{}", addr.port());
        for host in [rebinding_host.as_str(), mdns_host.as_str(), "127.0.0.1:1"] {
            assert_eq!(status_line(addr, host).await, "HTTP/1.1 403 Forbidden");
        }

        // Same policy on the WebSocket upgrade, for browsers and natives.
        let mut browser = browser_request(&format!("ws://{addr}/whisker-dev"));
        browser
            .headers_mut()
            .insert("Host", rebinding_host.parse().unwrap());
        assert_forbidden(tokio_tungstenite::connect_async(browser).await);
        assert_forbidden(
            tokio_tungstenite::connect_async(native_request(addr, &rebinding_host)).await,
        );

        handle.abort();
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn websocket_upgrade_rejects_cross_origin_and_originless_browser_requests() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let (_sender, addr, _handle) = serve(
            "127.0.0.1:0".parse().unwrap(),
            None,
            None,
            Vec::new(),
            false,
            None,
        )
        .await
        .unwrap();
        let url = format!("ws://{addr}/whisker-dev");
        let secure_origin = format!("https://{addr}");
        for origin in [
            None,
            Some("http://evil.test"),
            Some("null"),
            Some(secure_origin.as_str()),
        ] {
            let mut request = url.clone().into_client_request().unwrap();
            if let Some(origin) = origin {
                request
                    .headers_mut()
                    .insert("Origin", origin.parse().unwrap());
            }
            let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response) if response.status().as_u16() == 403)
            );
        }
    }

    #[test]
    fn parse_client_log_decodes_a_well_formed_frame() {
        let log = parse_client_log(
            r#"{"kind":"log","stream":"stdout","line":"hello world","ts_micros":"12345"}"#,
        )
        .expect("valid log envelope");
        assert_eq!(log.stream, "stdout");
        assert_eq!(log.line, "hello world");
        assert_eq!(log.ts_micros, 12345);
    }

    #[test]
    fn parse_client_log_falls_back_to_zero_ts_when_missing() {
        let log =
            parse_client_log(r#"{"kind":"log","stream":"stderr","line":"oops"}"#).expect("valid");
        assert_eq!(log.stream, "stderr");
        assert_eq!(log.line, "oops");
        assert_eq!(log.ts_micros, 0);
    }

    #[test]
    fn parse_client_log_rejects_other_kinds() {
        assert!(parse_client_log(r#"{"kind":"hello","aslr_reference":42}"#,).is_none());
    }

    #[test]
    fn parse_client_trace_decodes_frame_tuples() {
        let frames = parse_client_trace(r#"{"kind":"trace","frames":[[900,100,500,300,200,2,7]]}"#)
            .expect("valid");
        assert_eq!(
            frames,
            [crate::FrameSample {
                total_us: 900,
                reactive_us: 100,
                layout_us: 500,
                measure_us: 300,
                present_us: 200,
                measure_batches: 2,
                measure_requests: 7,
            }]
        );
        assert!(parse_client_trace(r#"{"kind":"log","frames":[]}"#).is_none());
    }

    #[tokio::test]
    async fn trace_server_enables_tracing_on_the_authenticated_client() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (_sender, addr, _handle) =
            serve(any, None, Some("s3kret".into()), Vec::new(), true, None)
                .await
                .expect("serve");
        let mut client = connect(addr).await;
        client
            .send(TMsg::Text(r#"{"kind":"hello","aslr_reference":1}"#.into()))
            .await
            .expect("send hello");
        let reply = tokio::time::timeout(std::time::Duration::from_secs(2), client.next())
            .await
            .expect("trace control frame")
            .expect("open socket")
            .expect("frame");
        assert_eq!(reply, TMsg::Text(TRACE_ON.into()));
    }

    #[tokio::test]
    async fn browser_mode_hello_cannot_choose_the_aslr_reference() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(None).await;
        let mut client = browser_connect(addr).await;
        client
            .send(TMsg::Text(
                r#"{"kind":"hello","aslr_reference":3735928559}"#.into(),
            ))
            .await
            .expect("send hello");
        for _ in 0..200 {
            if sender.latest_aslr_reference().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.latest_aslr_reference(), Some(BROWSER_ASLR_REFERENCE));
    }

    #[tokio::test]
    async fn unauthenticated_log_and_trace_frames_close_the_socket() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let events = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&events);
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
            if matches!(event, Event::DeviceLog { .. } | Event::DeviceTrace { .. }) {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        let (sender, addr, _handle) = serve(
            "127.0.0.1:0".parse().unwrap(),
            Some(on_event),
            Some("s3kret".into()),
            Vec::new(),
            false,
            None,
        )
        .await
        .expect("serve");

        for frame in [
            r#"{"kind":"log","stream":"stdout","line":"spoofed"}"#,
            r#"{"kind":"trace","frames":[[1,1,1,1,1,1,1]]}"#,
        ] {
            let (mut client, _) =
                tokio_tungstenite::connect_async(native_request(addr, &addr.to_string()))
                    .await
                    .expect("connect");
            client.send(TMsg::Text(frame.into())).await.expect("send");
            let closed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    match client.next().await {
                        None | Some(Ok(TMsg::Close(_))) | Some(Err(_)) => return true,
                        Some(Ok(TMsg::Binary(_))) => return false,
                        _ => continue,
                    }
                }
            })
            .await
            .expect("socket stayed open");
            assert!(closed, "{frame} must close an unauthenticated socket");
        }
        for _ in 0..200 {
            if sender.client_count() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.client_count(), 0);
        assert_eq!(events.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn on_event_callback_fires_with_device_log_lines() {
        use std::sync::Mutex;
        let captured: Arc<Mutex<Vec<(String, String, u128)>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |e| {
            if let Event::DeviceLog {
                stream,
                line,
                ts_micros,
            } = e
            {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((stream, line, ts_micros));
            }
        });

        let (sender, addr) = spawn_test_server(Some(on_event)).await;
        let mut client = connect(addr).await;
        for _ in 0..100 {
            if sender.client_count() > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.client_count(), 1);

        client
            .send(tokio_tungstenite::tungstenite::Message::Text(
                r#"{"kind":"log","stream":"stdout","line":"hi from device","ts_micros":"42"}"#
                    .into(),
            ))
            .await
            .expect("send log frame");

        // Wait for the server to dispatch the callback.
        for _ in 0..100 {
            if !captured.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let g = captured.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].0, "stdout");
        assert_eq!(g[0].1, "hi from device");
        assert_eq!(g[0].2, 42);
    }
}
