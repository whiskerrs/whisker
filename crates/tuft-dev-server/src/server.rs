//! WebSocket dev server.
//!
//! `tuft run` opens a TCP listener, exposes a single
//! `GET /tuft-dev` route that upgrades to WebSocket, and pushes
//! `Envelope` messages to every connected client. The on-device
//! `tuft-dev-runtime` is the canonical client.
//!
//! Wire format is the same JSON envelope `tuft-dev-runtime` parses
//! (and unit-tests against):
//!
//! ```text
//! { "kind": "patch", "table": <subsecond::JumpTable JSON> }
//! ```
//!
//! Architecture is a single `tokio::sync::broadcast` channel: every
//! connected socket has its own subscriber receiver, so one
//! `PatchSender::send` reaches all clients. New connections see only
//! envelopes sent *after* they subscribe — the receiver is at the
//! tail end of the broadcast buffer, not replayed.

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::Event;

/// Wire-level message pushed to clients. `Envelope` mirrors the enum
/// `tuft-dev-runtime::hot_reload` parses on the receive side; both
/// rely on serde's tag/snake_case to keep the shape stable.
///
/// The `table` field is wrapped in [`WireJumpTable`] so the address
/// map serialises as a JSON array of `[old, new]` pairs rather than
/// a JSON object. JSON objects can only have string keys, so the
/// default `HashMap<u64, u64>` derive would produce
/// `{ "1234": 5678 }` — and the matching deserialize side, given a
/// custom hasher like `subsecond_types::BuildAddressHasher`, fails
/// to convert the string back to `u64`. The pair-array form
/// sidesteps both: keys travel as JSON numbers, deserialize is
/// straightforward, and the on-the-wire payload is also smaller.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    /// A new subsecond JumpTable for the device to apply.
    Patch {
        #[serde(serialize_with = "wire_jump_table::serialize")]
        table: subsecond_types::JumpTable,
    },
}

/// Shared serde adapter used by `tuft-dev-runtime::hot_reload` too —
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

/// Cheap-to-clone handle for sending envelopes from the rest of the
/// dev server (file watcher / builder / etc.) to every connected
/// client.
#[derive(Clone)]
pub struct PatchSender {
    tx: broadcast::Sender<Envelope>,
}

impl PatchSender {
    /// Broadcast `env` to every currently-connected client.
    /// Returns the number of clients the message was queued for —
    /// `0` is fine (no client connected yet) and not an error.
    pub fn send(&self, env: Envelope) -> usize {
        self.tx.send(env).unwrap_or(0)
    }

    /// Number of clients currently subscribed.
    pub fn client_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<Envelope>,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
}

/// Bind on `addr`, spawn the axum server on the current tokio
/// runtime, and return:
///   - a [`PatchSender`] for the rest of the dev loop to push patches
///   - the actual bound address (useful when caller asked for port 0)
///   - the spawned server task's `JoinHandle`
///
/// `on_event` is an optional observer hook — `tuft-cli` uses it to
/// render terminal UI on connect/disconnect events.
pub async fn serve(
    addr: SocketAddr,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
) -> Result<(PatchSender, SocketAddr, tokio::task::JoinHandle<()>)> {
    let (tx, _rx) = broadcast::channel::<Envelope>(16);
    let state = AppState { tx: tx.clone(), on_event };

    let app = Router::new()
        .route("/tuft-dev", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[tuft-dev-server] axum serve error: {e}");
        }
    });

    Ok((PatchSender { tx }, bound, handle))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    use futures_util::{SinkExt, StreamExt};

    let (mut tx_ws, mut rx_ws) = socket.split();
    let mut bcast_rx = state.tx.subscribe();
    eprintln!(
        "[tuft-dev-server] client connected (total: {})",
        state.tx.receiver_count(),
    );
    if let Some(cb) = &state.on_event {
        cb(Event::ClientConnected);
    }

    loop {
        tokio::select! {
            // server → client: forward broadcast envelopes as text frames.
            recv = bcast_rx.recv() => {
                let env = match recv {
                    Ok(e) => e,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let json = match serde_json::to_string(&env) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[tuft-dev-server] serialize: {e}");
                        continue;
                    }
                };
                if tx_ws.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            // client → server: drain incoming so Pings/Pongs are honoured;
            // close on Close frame or transport error.
            msg = rx_ws.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    if let Some(cb) = &state.on_event {
        cb(Event::ClientDisconnected);
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
    ) -> tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    > {
        let url = format!("ws://{addr}/tuft-dev");
        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect");
        ws
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
        let n = sender.send(Envelope::Patch { table: table.clone() });
        assert_eq!(n, 1);

        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.next(),
        )
        .await
        .expect("recv timed out")
        .expect("stream ended")
        .expect("ws error");
        let text = match msg {
            tokio_tungstenite::tungstenite::Message::Text(s) => s,
            other => panic!("expected text, got {other:?}"),
        };
        // It must round-trip — we deliberately use the same envelope
        // shape the receiver in tuft-dev-runtime parses.
        let parsed: serde_json::Value =
            serde_json::from_str(&text).expect("parse json");
        assert_eq!(parsed["kind"], "patch");
        assert_eq!(parsed["table"]["lib"], "/tmp/dummy.dylib");
        assert_eq!(parsed["table"]["aslr_reference"], 4294967296_u64);
    }

    #[tokio::test]
    async fn send_with_no_clients_returns_zero_and_does_not_error() {
        let (sender, _addr) = spawn_test_server(None).await;
        assert_eq!(sender.client_count(), 0);
        let n = sender.send(Envelope::Patch {
            table: make_dummy_jump_table(),
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

        let n = sender.send(Envelope::Patch {
            table: make_dummy_jump_table(),
        });
        assert_eq!(n, 2);

        for client in [&mut a, &mut b] {
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                client.next(),
            )
            .await
            .expect("timeout")
            .expect("stream end")
            .expect("ws err");
            assert!(matches!(
                msg,
                tokio_tungstenite::tungstenite::Message::Text(_)
            ));
        }
    }

    #[tokio::test]
    async fn on_event_callback_fires_for_connect_and_disconnect() {
        let connect_count = Arc::new(AtomicUsize::new(0));
        let disconnect_count = Arc::new(AtomicUsize::new(0));

        let cc = connect_count.clone();
        let dc = disconnect_count.clone();
        let on_event: Arc<dyn Fn(Event) + Send + Sync> =
            Arc::new(move |e| match e {
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
