//! WebSocket dev server.
//!
//! `whisker run` opens a TCP listener, exposes a single
//! `GET /whisker-dev` route that upgrades to WebSocket, and pushes
//! patch messages to every connected client. The on-device
//! `whisker-dev-runtime` is the canonical client.
//!
//! ## Wire format
//!
//! Two frame types:
//!
//! **Patches** — *binary* frames laid out as:
//!
//! ```text
//! [8 bytes: u64 BE — JSON header length]
//! [N bytes:        JSON header { "kind": "patch", "table": {...} } ]
//! [rest:           raw patch dylib bytes (no encoding) ]
//! ```
//!
//! No base64. The dylib lands on the device with the original byte
//! count, ~30 % smaller on the wire than the previous JSON-with-
//! base64-string protocol.
//!
//! **Hello** — *text* frame, `{"kind":"hello","aslr_reference":<u64>}`.
//! The device sends this on connect; the server stores the value
//! and the patcher uses it to compute the ASLR slide.
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
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use crate::Event;

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
    tx: broadcast::Sender<Patch>,
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
        self.tx.send(patch).unwrap_or(0)
    }

    /// Number of clients currently subscribed.
    pub fn client_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// The runtime address of `main` (= `subsecond::aslr_reference()`)
    /// most recently reported by a connected client. `None` when no
    /// client has connected or sent its `hello` yet — the patcher
    /// should withhold Tier 1 patches in that case (fall back to
    /// Tier 2 cold rebuild).
    pub fn latest_aslr_reference(&self) -> Option<u64> {
        self.aslr_reference.lock().ok().and_then(|g| *g)
    }
}

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<Patch>,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    aslr_reference: Arc<Mutex<Option<u64>>>,
    /// Expected shared dev-session token. When `Some`, a client must
    /// present a matching `token` in its `hello` before any patch is
    /// forwarded to it; a missing/mismatched token closes the
    /// connection. The patch channel `dlopen`s whatever it ships, so on
    /// a LAN-exposed bind this gate is what stops an unauthenticated
    /// peer from pushing arbitrary native code. `None` = unauthenticated
    /// (token-less local setup / tests).
    expected_token: Option<Arc<str>>,
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
) -> Result<(PatchSender, SocketAddr, tokio::task::JoinHandle<()>)> {
    let (tx, _rx) = broadcast::channel::<Patch>(16);
    let aslr_reference: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let state = AppState {
        tx: tx.clone(),
        on_event,
        aslr_reference: Arc::clone(&aslr_reference),
        expected_token: expected_token.map(Arc::from),
    };

    let app = Router::new()
        .route("/whisker-dev", get(ws_handler))
        .with_state(state);

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
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    use futures_util::{SinkExt, StreamExt};

    let (mut tx_ws, mut rx_ws) = socket.split();
    let mut bcast_rx = state.tx.subscribe();
    whisker_build::ui::set_status(format!("{} client(s) connected", state.tx.receiver_count(),));
    // `aslr_reference` is internal handshake plumbing; emit at debug
    // grade so the steady-state UI stays clean.
    if let Some(cb) = &state.on_event {
        cb(Event::ClientConnected);
    }

    // A client starts unauthenticated when a token is required, and is
    // promoted on a valid `hello`. While unauthenticated we never
    // forward a patch (the security gate). A token-less server (`None`)
    // is open by default — local loopback / tests.
    let mut authed = state.expected_token.is_none();

    loop {
        tokio::select! {
            // server → client: forward broadcast patches as binary frames.
            recv = bcast_rx.recv() => {
                let patch = match recv {
                    Ok(p) => p,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                // Never ship native-code patches to an unauthenticated
                // peer. Drop (not buffer) — the device re-receives the
                // full JumpTable on the next save anyway.
                if !authed {
                    continue;
                }
                let frame = match encode_patch_frame(&patch) {
                    Ok(b) => b,
                    Err(e) => {
                        whisker_build::ui::warn(format!("encode patch frame: {e}"));
                        continue;
                    }
                };
                if tx_ws.send(Message::Binary(frame.into())).await.is_err() {
                    break;
                }
            }
            // client → server: drain incoming so Pings/Pongs are honoured;
            // close on Close frame or transport error. Text frames are
            // parsed for `hello` envelopes carrying the client's
            // `aslr_reference` (+ session token).
            msg = rx_ws.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Text(t))) => {
                        if let Some(hello) = parse_client_hello(&t) {
                            // Token gate. If a token is required, the
                            // hello must carry the matching one or we
                            // drop the connection without ever arming
                            // the patch path for it.
                            if let Some(expected) = &state.expected_token {
                                if hello.token.as_deref() != Some(expected.as_ref()) {
                                    whisker_build::ui::warn(
                                        "rejecting hot-reload client: missing/invalid dev token",
                                    );
                                    break;
                                }
                                authed = true;
                            }
                            let aslr = hello.aslr_reference;
                            whisker_build::ui::debug(format!(
                                "client hello · aslr_reference={aslr:#x}"
                            ));
                            if let Ok(mut g) = state.aslr_reference.lock() {
                                *g = Some(aslr);
                            }
                        } else if let Some(log) = parse_client_log(&t) {
                            if let Some(cb) = &state.on_event {
                                cb(Event::DeviceLog {
                                    stream: log.stream,
                                    line: log.line,
                                    ts_micros: log.ts_micros,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Clear the stored `aslr_reference` on disconnect. It's the ASLR
    // slide of the *now-dead* process; reusing it to build a patch for
    // the next process (e.g. a Tier 2 rebuild relaunch) would stamp
    // jump stubs against meaningless addresses and crash the device.
    // The replacement process re-sends its own `hello` with a fresh
    // slide; until then `latest_aslr_reference()` returns `None` and the
    // patch path skips rather than shipping a stale-based patch.
    if let Ok(mut g) = state.aslr_reference.lock() {
        *g = None;
    }

    if let Some(cb) = &state.on_event {
        cb(Event::ClientDisconnected);
    }
}

/// Build the on-the-wire binary frame:
///   `[u64 BE json_len][json header][raw dylib bytes]`
fn encode_patch_frame(patch: &Patch) -> Result<Vec<u8>> {
    let header = PatchHeader::Patch {
        table: &patch.table,
    };
    let json = serde_json::to_vec(&header)?;
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
    /// The shared dev-session token, if the device was provisioned one.
    token: Option<String>,
}

/// Parse a client hello envelope. Returns `None` for non-hello text
/// frames (or malformed payloads) — the only things we listen for
/// client→server today are the initial handshake and log frames.
fn parse_client_hello(text: &str) -> Option<ClientHello> {
    #[derive(serde::Deserialize)]
    struct Hello {
        kind: String,
        aslr_reference: u64,
        #[serde(default)]
        token: Option<String>,
    }
    let h: Hello = serde_json::from_str(text).ok()?;
    if h.kind == "hello" {
        Some(ClientHello {
            aslr_reference: h.aslr_reference,
            token: h.token,
        })
    } else {
        None
    }
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

// ============================================================================
// Tests
// ============================================================================

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
        let (sender, addr, _handle) = serve(any, on_event, None).await.expect("serve");
        (sender, addr)
    }

    async fn connect(
        addr: SocketAddr,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let url = format!("ws://{addr}/whisker-dev");
        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect");
        ws
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

    async fn spawn_test_server_with_token(token: Option<String>) -> (PatchSender, SocketAddr) {
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (sender, addr, _handle) = serve(any, None, token).await.expect("serve");
        (sender, addr)
    }

    #[tokio::test]
    async fn client_with_valid_token_is_armed_and_receives_patches() {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = connect(addr).await;

        // Authenticate via the hello handshake.
        client
            .send(TMsg::Text(
                r#"{"kind":"hello","aslr_reference":4294967296,"token":"s3kret"}"#.into(),
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
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (sender, addr) = spawn_test_server_with_token(Some("s3kret".into())).await;
        let mut client = connect(addr).await;

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
