//! WebSocket dev server.
//!
//! `whisker run` opens a TCP listener, exposes a single
//! `GET /whisker-dev` route that upgrades to WebSocket, and pushes
//! patch messages to every connected client. The on-device
//! `whisker-dev-runtime` is the canonical client.
//!
//! ## Wire format
//!
//! Native peers prove possession of the session secret using fresh client and
//! server nonce challenges. Each binary envelope then authenticates its version,
//! payload type, sequence number and bytes with a directional HMAC-SHA256 bound
//! to that transcript. The inner patch is `[u64 BE header length][JSON][dylib]`.
//! Browser sessions receive reload controls only, never native patches.
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
use whisker_dev_runtime::auth::{self, Session};

/// Cheap-to-clone broadcast payload. The dylib bytes are held by
/// `Arc` so cloning into each subscribed client's receive queue is
/// just a refcount bump.
#[derive(Debug, Clone)]
pub struct Patch {
    /// The address-map metadata. Serialized as JSON in the binary
    /// frame's prefix.
    pub table: subsecond_types::JumpTable,
    /// Raw patch dylib bytes. Streamed verbatim after the JSON
    /// prefix; the device writes them to disk and `dlopen`s the
    /// resulting file.
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

/// Shared serde adapter used by `whisker-dev-runtime::hot_reload` too —
/// both sides must agree on the JSON shape. Kept inline (not a
/// shared crate) because the type is tiny and the duplication
/// burden is one ~30-line module.
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
    /// Latest `aslr_reference` reported by a connected client via the
    /// `hello` handshake. Single-slot, last-write-wins: we don't yet
    /// support targeted-per-client patches, so all connected clients
    /// must share an ASLR base. For typical single-emulator dev
    /// sessions that's fine; for multi-device this becomes the
    /// natural boundary where patches start being per-client.
    aslr_reference: Arc<Mutex<Option<u64>>>,
}

impl PatchSender {
    /// Broadcast `patch` to every currently-connected client.
    /// Returns the number of clients the message was queued for —
    /// `0` is fine (no client connected yet) and not an error.
    pub fn send(&self, patch: Patch) -> usize {
        self.tx.send(ServerUpdate::Patch(patch)).unwrap_or(0)
    }

    /// Broadcast a validated WebAssembly side module to browser clients only.
    /// Invalid modules and native libraries are never queued on this channel.
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
    /// connected client. `None` when no
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
    /// Shared secret required for native mutual authentication; None selects browser-only reload.
    expected_token: Option<Arc<str>>,
    trace: bool,
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
pub async fn serve(
    addr: SocketAddr,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    expected_token: Option<String>,
    trace: bool,
    static_files: Option<StaticFiles>,
) -> Result<(PatchSender, SocketAddr, tokio::task::JoinHandle<()>)> {
    let (tx, _rx) = broadcast::channel::<ServerUpdate>(16);
    let aslr_reference: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let state = AppState {
        tx: tx.clone(),
        on_event,
        aslr_reference: Arc::clone(&aslr_reference),
        expected_token: expected_token.map(Arc::from),
        trace,
    };

    let mut app = Router::new()
        .route("/whisker-dev", get(ws_handler))
        .with_state(state);
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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            whisker_build::ui::error(format!("axum serve error: {e}"));
        }
    });

    Ok((PatchSender { tx, aslr_reference }, bound, handle))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.max_frame_size(auth::MAX_CONTROL + 42)
        .max_message_size(auth::MAX_CONTROL + 42)
        .on_upgrade(move |socket| handle_socket(socket, state))
}

async fn authenticate(socket: &mut WebSocket, token: &str) -> Result<Session> {
    use futures_util::StreamExt;
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        anyhow::bail!("missing challenge");
    };
    anyhow::ensure!(text.len() <= auth::MAX_HANDSHAKE, "challenge too large");
    let challenge: auth::Challenge = serde_json::from_str(&text)?;
    anyhow::ensure!(
        challenge.version == 1 && !token.is_empty(),
        "unsupported authentication"
    );
    let nonce = auth::nonce()?;
    let proof = auth::ServerProof {
        nonce,
        mac: auth::proof(token, true, &challenge.nonce, &nonce),
    };
    socket
        .send(Message::Text(serde_json::to_string(&proof)?.into()))
        .await?;
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        anyhow::bail!("missing client proof");
    };
    anyhow::ensure!(text.len() <= auth::MAX_HANDSHAKE, "proof too large");
    let proof: auth::ClientProof = serde_json::from_str(&text)?;
    anyhow::ensure!(
        auth::verify_proof(token, false, &challenge.nonce, &nonce, &proof.mac),
        "invalid client proof"
    );
    Ok(Session::new(token.to_owned(), challenge.nonce, nonce, true))
}

async fn send_frame(
    socket: &mut WebSocket,
    session: &mut Option<Session>,
    message: Message,
) -> Result<()> {
    let message = if let Some(session) = session {
        let (kind, payload) = match &message {
            Message::Text(text) => (auth::TEXT, text.as_bytes()),
            Message::Binary(bytes) => (auth::PATCH, bytes.as_ref()),
            _ => {
                socket.send(message).await?;
                return Ok(());
            }
        };
        Message::Binary(
            session
                .seal(kind, payload)
                .map_err(anyhow::Error::msg)?
                .into(),
        )
    } else {
        message
    };
    socket.send(message).await?;
    Ok(())
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    use futures_util::StreamExt;
    let mut session = if let Some(token) = &state.expected_token {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            authenticate(&mut socket, token),
        )
        .await
        {
            Ok(Ok(session)) => Some(session),
            _ => return,
        }
    } else {
        None
    };
    let native = session.is_some();
    let mut receiver = state.tx.subscribe();
    if let Some(callback) = &state.on_event {
        callback(Event::ClientConnected);
    }
    if state.trace
        && native
        && send_frame(&mut socket, &mut session, Message::Text(TRACE_ON.into()))
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
                let message = match update {
                    ServerUpdate::Patch(patch) if native => match encode_patch_frame(&patch) {
                        Ok(frame) => Message::Binary(frame.into()),
                        Err(_) => { whisker_build::ui::warn("development patch exceeds transport limits"); continue; }
                    },
                    ServerUpdate::WebPatch(patch) if !native => match encode_patch_frame(&patch) {
                        Ok(frame) => Message::Binary(frame.into()),
                        Err(_) => { whisker_build::ui::warn("WebAssembly patch exceeds transport limits"); continue; }
                    },
                    ServerUpdate::Patch(_) | ServerUpdate::WebPatch(_) => continue,
                    ServerUpdate::Reload => Message::Text(r#"{"kind":"reload"}"#.into()),
                };
                if send_frame(&mut socket, &mut session, message).await.is_err() { break; }
            }
            message = socket.next() => {
                let text = match message {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(Message::Binary(frame))) if native => {
                        let Some(session) = session.as_mut() else { break; };
                        match session.open(&frame) {
                            Ok((auth::TEXT, payload)) => match std::str::from_utf8(payload) { Ok(text) => text.to_owned(), Err(_) => break },
                            _ => break,
                        }
                    },
                    Some(Ok(Message::Text(text))) if !native && text.len() <= auth::MAX_CONTROL => text.to_string(),
                    _ => break,
                };
                if let Some(hello) = parse_client_hello(&text) {
                    if let Ok(mut reference) = state.aslr_reference.lock() { *reference = Some(hello.aslr_reference); }
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
fn encode_patch_frame(patch: &Patch) -> Result<Vec<u8>> {
    let header = PatchHeader::Patch {
        table: &patch.table,
    };
    let json = serde_json::to_vec(&header)?;
    anyhow::ensure!(
        json.len() <= auth::MAX_CONTROL
            && patch.dylib_bytes.len() <= auth::MAX_MESSAGE - 50 - json.len(),
        "patch exceeds transport limits"
    );
    let json_len = json.len() as u64;
    let dylib = patch.dylib_bytes.as_slice();
    let mut frame = Vec::with_capacity(8 + json.len() + dylib.len());
    frame.extend_from_slice(&json_len.to_be_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(dylib);
    Ok(frame)
}

/// A decoded `{"kind":"hello",…}` handshake from the device.
struct ClientHello {
    aslr_reference: u64,
}

/// Parse a client hello envelope. Returns `None` for non-hello text
/// frames (or malformed payloads) — the only things we listen for
/// client→server today are the initial handshake and log frames.
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

/// Parse a client log envelope. Returns `None` for any other text
/// frame so the caller can fall through to other handlers (the hello
/// envelope is the only other text frame today).
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
        stream: h.stream,
        line: h.line,
        ts_micros,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        let (sender, addr, _handle) = serve(any, on_event, Some("s3kret".into()), false, None)
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

    impl TestClient {
        async fn authenticate(mut socket: TestSocket) -> Self {
            use tokio_tungstenite::tungstenite::Message as TMsg;
            let client = auth::nonce().unwrap();
            socket
                .send(TMsg::Text(
                    serde_json::to_string(&auth::Challenge {
                        version: 1,
                        nonce: client,
                    })
                    .unwrap(),
                ))
                .await
                .unwrap();
            let TMsg::Text(text) = socket.next().await.unwrap().unwrap() else {
                panic!("missing proof");
            };
            let server: auth::ServerProof = serde_json::from_str(&text).unwrap();
            assert!(auth::verify_proof(
                "s3kret",
                true,
                &client,
                &server.nonce,
                &server.mac
            ));
            socket
                .send(TMsg::Text(
                    serde_json::to_string(&auth::ClientProof {
                        mac: auth::proof("s3kret", false, &client, &server.nonce),
                    })
                    .unwrap(),
                ))
                .await
                .unwrap();
            Self {
                socket,
                session: Session::new("s3kret".into(), client, server.nonce, false),
            }
        }

        async fn send(
            &mut self,
            message: tokio_tungstenite::tungstenite::Message,
        ) -> Result<(), tokio_tungstenite::tungstenite::Error> {
            use tokio_tungstenite::tungstenite::Message as TMsg;
            let message = match message {
                TMsg::Text(text) => {
                    TMsg::Binary(self.session.seal(auth::TEXT, text.as_bytes()).unwrap())
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
                    Ok((auth::PATCH, bytes)) => Ok(TMsg::Binary(bytes.to_vec())),
                    Ok((auth::TEXT, bytes)) => {
                        Ok(TMsg::Text(String::from_utf8(bytes.to_vec()).unwrap()))
                    }
                    _ => Err(Error::Io(std::io::Error::other(
                        "invalid authenticated frame",
                    ))),
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
        // Mirror the device receiver's lifted read limits
        // (whisker-dev-runtime::hot_reload::client_loop).
        let config = WebSocketConfig {
            max_frame_size: Some(auth::MAX_MESSAGE),
            max_message_size: Some(auth::MAX_MESSAGE),
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
        let (sender, addr, _handle) = serve(any, None, token, false, None).await.expect("serve");
        (sender, addr)
    }

    #[tokio::test]
    async fn client_with_valid_token_is_armed_and_receives_patches() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = connect(addr).await;

        // Authenticate via the hello handshake.
        client
            .send(TMsg::Text(
                r#"{"kind":"hello","aslr_reference":4294967296}"#.into(),
            ))
            .await
            .expect("send hello");

        // The server only records the aslr_reference *after* the token
        // check passes, so its presence is a proxy for "authed".
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

    #[tokio::test]
    async fn client_with_invalid_token_is_disconnected_and_gets_no_patch() {
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = raw_connect(addr).await;

        // Wrong token → the server closes the connection without ever
        // arming the patch path.
        client
            .send(TMsg::Text(
                r#"{"kind":"hello","aslr_reference":1,"token":"WRONG"}"#.into(),
            ))
            .await
            .expect("send hello");

        // The connection should end (Close frame or stream end) and the
        // client count drop back to zero.
        let ended = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match client.next().await {
                    Some(Ok(TMsg::Binary(_))) => return false, // a patch leaked through — fail
                    None | Some(Ok(TMsg::Close(_))) | Some(Err(_)) => return true,
                    _ => continue,
                }
            }
        })
        .await
        .expect("disconnect timed out");
        assert!(
            ended,
            "unauthenticated client must be disconnected, not fed patches"
        );

        // A patch broadcast now reaches zero armed clients.
        for _ in 0..200 {
            if sender.client_count() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sender.client_count(), 0);
    }

    #[tokio::test]
    async fn browser_only_sessions_never_receive_native_patch_bytes() {
        let (sender, addr) = spawn_test_server_with_token(None).await;
        let mut client = raw_connect(addr).await;
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
        let (sender, addr, _handle) =
            serve("127.0.0.1:0".parse().unwrap(), None, None, false, None)
                .await
                .unwrap();
        let mut browser = raw_connect(addr).await;
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
        let (sender, addr, _handle) =
            serve("127.0.0.1:0".parse().unwrap(), None, None, false, None)
                .await
                .unwrap();
        let mut browser = raw_connect(addr).await;
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
        let (_sender, addr, _handle) = serve(any, None, Some("s3kret".into()), true, None)
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
