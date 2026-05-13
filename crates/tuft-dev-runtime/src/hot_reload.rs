//! WebSocket-based hot-reload patch receiver.
//!
//! Connection direction is **device → host**: a Tuft app running on
//! a device / emulator / simulator opens a WebSocket to the host
//! running `tuft run`. The host pushes envelopes of the form
//!
//! ```text
//! { "kind": "patch", "table": <subsecond::JumpTable JSON> }
//! ```
//!
//! down the socket; the receiver dropping them into a single-slot
//! mutex. The Lynx TASM thread later drains the slot at the top of
//! its tick (via [`take_pending_patch`]) and invokes
//! `subsecond::apply_patch` while **no** `subsecond::call` is on the
//! stack — the only safe window.
//!
//! Connection address is taken from the `TUFT_DEV_ADDR` env var. If
//! unset, [`start_receiver`] no-ops, so a stray `hot-reload`-built
//! binary running without a dev server stays inert.
//!
//! The receiver retries on disconnect with a small backoff so a
//! `tuft run` restart on the host doesn't require restarting the
//! app on the device.

use std::sync::Mutex;
use std::time::Duration;

use subsecond::JumpTable;

/// Most-recent-wins: an older queued patch is silently superseded.
/// `tuft run` should be sending fully-replaced JumpTables anyway.
static PENDING: Mutex<Option<JumpTable>> = Mutex::new(None);

/// TASM-thread entry — pop the queued patch, if any. Safe to call
/// every tick (returns `None` cheaply).
pub fn take_pending_patch() -> Option<JumpTable> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Spawn the receiver thread. Reads `TUFT_DEV_ADDR` from the env on
/// first call; if unset, logs once and returns — making this safe to
/// call unconditionally from app bootstrap.
pub fn start_receiver() {
    let addr = match std::env::var("TUFT_DEV_ADDR") {
        Ok(a) if !a.is_empty() => a,
        _ => {
            eprintln!(
                "[tuft-dev] TUFT_DEV_ADDR not set; hot-reload receiver disabled",
            );
            return;
        }
    };
    std::thread::Builder::new()
        .name("tuft-hot-reload".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("[tuft-dev] couldn't build tokio runtime: {e}");
                    return;
                }
            };
            rt.block_on(client_loop(addr));
        })
        .expect("spawn tuft-hot-reload thread");
}

async fn client_loop(addr: String) {
    let url = format!("ws://{addr}/tuft-dev");
    loop {
        match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => {
                eprintln!("[tuft-dev] connected: {url}");
                if let Err(e) = handle_session(ws).await {
                    eprintln!("[tuft-dev] session ended: {e}");
                }
            }
            Err(e) => eprintln!("[tuft-dev] connect {url} failed: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn handle_session<S>(
    mut ws: tokio_tungstenite::WebSocketStream<S>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    while let Some(msg) = ws.next().await {
        let msg = msg?;
        match msg {
            Message::Text(text) => match parse_envelope(&text) {
                Ok(Envelope::Patch { table }) => {
                    if let Ok(mut p) = PENDING.lock() {
                        *p = Some(table);
                        eprintln!("[tuft-dev] patch queued");
                    }
                }
                Err(e) => eprintln!("[tuft-dev] malformed envelope: {e}"),
            },
            Message::Close(_) => return Ok(()),
            _ => {} // ignore Binary / Ping / Pong for now
        }
    }
    Ok(())
}

// ----- Wire format ----------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Envelope {
    Patch { table: JumpTable },
}

fn parse_envelope(s: &str) -> Result<Envelope, serde_json::Error> {
    serde_json::from_str(s)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_patch_envelope() {
        // Construct a JumpTable JSON by hand — we don't want to depend
        // on a particular subsecond-types serialisation example. Just
        // every required field present, plausible values.
        let json = r#"{
            "kind": "patch",
            "table": {
                "lib": "/tmp/some-patch.dylib",
                "map": {},
                "aslr_reference": 4294967296,
                "new_base_address": 8589934592,
                "ifunc_count": 0
            }
        }"#;
        let env = parse_envelope(json).expect("should parse");
        match env {
            Envelope::Patch { table } => {
                assert_eq!(
                    table.lib.to_string_lossy(),
                    "/tmp/some-patch.dylib",
                );
                assert_eq!(table.aslr_reference, 0x1_0000_0000);
                assert_eq!(table.new_base_address, 0x2_0000_0000);
                assert_eq!(table.ifunc_count, 0);
            }
        }
    }

    #[test]
    fn rejects_unknown_envelope_kind() {
        let json = r#"{ "kind": "frobnicate" }"#;
        assert!(parse_envelope(json).is_err());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_envelope("not json").is_err());
    }

    #[test]
    fn take_pending_returns_none_when_queue_is_empty() {
        // The static slot is shared across the test binary; drain
        // anything a sibling test parked, then assert empty.
        let _ = take_pending_patch();
        assert!(take_pending_patch().is_none());
    }
}
