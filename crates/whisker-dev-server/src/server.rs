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
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
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
    use serde::ser::SerializeStruct;
    use serde::Serializer;
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
) -> Result<(PatchSender, SocketAddr, tokio::task::JoinHandle<()>)> {
    let (tx, _rx) = broadcast::channel::<Patch>(16);
    let aslr_reference: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let state = AppState {
        tx: tx.clone(),
        on_event,
        aslr_reference: Arc::clone(&aslr_reference),
    };

    let app = Router::new()
        .route("/whisker-dev", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[whisker-dev-server] axum serve error: {e}");
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
    eprintln!(
        "[whisker-dev-server] client connected (total: {})",
        state.tx.receiver_count(),
    );
    if let Some(cb) = &state.on_event {
        cb(Event::ClientConnected);
    }

    loop {
        tokio::select! {
            // server → client: forward broadcast patches as binary frames.
            recv = bcast_rx.recv() => {
                let patch = match recv {
                    Ok(p) => p,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let frame = match encode_patch_frame(&patch) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("[whisker-dev-server] encode patch frame: {e}");
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
            // `aslr_reference`.
            msg = rx_ws.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Text(t))) => {
                        if let Some(aslr) = parse_client_aslr_reference(&t) {
                            eprintln!(
                                "[whisker-dev-server] client hello: aslr_reference={aslr:#x}"
                            );
                            if let Ok(mut g) = state.aslr_reference.lock() {
                                *g = Some(aslr);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
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

/// Pull the `aslr_reference` field out of a client hello envelope.
/// Returns `None` for non-hello text frames (or malformed payloads)
/// — the only thing we actively listen for client→server today is the
/// initial handshake.
fn parse_client_aslr_reference(text: &str) -> Option<u64> {
    #[derive(serde::Deserialize)]
    struct Hello {
        kind: String,
        aslr_reference: u64,
    }
    let h: Hello = serde_json::from_str(text).ok()?;
    if h.kind == "hello" {
        Some(h.aslr_reference)
    } else {
        None
    }
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
        let (sender, addr, _handle) = serve(any, on_event).await.expect("serve");
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
}
