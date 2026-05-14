//! WebSocket-based hot-reload patch receiver.
//!
//! Connection direction is **device → host**: a Whisker app running on
//! a device / emulator / simulator opens a WebSocket to the host
//! running `whisker run`. The host pushes envelopes of the form
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
//! Connection address is taken from the `WHISKER_DEV_ADDR` env var. If
//! unset, [`start_receiver`] no-ops, so a stray `hot-reload`-built
//! binary running without a dev server stays inert.
//!
//! The receiver retries on disconnect with a small backoff so a
//! `whisker run` restart on the host doesn't require restarting the
//! app on the device.

use std::sync::Mutex;
use std::time::Duration;

use subsecond::JumpTable;

/// Log a one-line message tagged `whisker-dev`. On Android, writes to
/// logcat via `__android_log_write` (Rust's `eprintln!` doesn't go
/// anywhere useful on Android — stderr is dropped). On other
/// platforms it's a plain `eprintln!` so dev sessions on host /
/// macOS / Linux still get readable output.
///
/// Public so whisker-driver's `apply_pending_hot_patch` can log under
/// the same `whisker-dev` tag without duplicating the helper.
pub fn devlog(line: &str) {
    #[cfg(target_os = "android")]
    {
        // bionic exports __android_log_write(prio, tag, text) → int.
        // ANDROID_LOG_INFO = 4. Both tag and text must be
        // NUL-terminated.
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
    #[cfg(not(target_os = "android"))]
    {
        eprintln!("[whisker-dev] {line}");
    }
}

/// Most-recent-wins: an older queued patch is silently superseded.
/// `whisker run` should be sending fully-replaced JumpTables anyway.
static PENDING: Mutex<Option<JumpTable>> = Mutex::new(None);

/// TASM-thread entry — pop the queued patch, if any. Safe to call
/// every tick (returns `None` cheaply).
pub fn take_pending_patch() -> Option<JumpTable> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Spawn the receiver thread. Reads `WHISKER_DEV_ADDR` from the env;
/// if unset, falls back to `127.0.0.1:9876` (the dev-server's
/// default), which works on Android once `adb reverse` is in
/// place. Safe to call unconditionally from app bootstrap — the
/// loop retries on connection failure so a dev server starting
/// later still gets picked up.
pub fn start_receiver() {
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
    let url = format!("ws://{addr}/whisker-dev");
    loop {
        match tokio_tungstenite::connect_async(&url).await {
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
            Message::Text(text) => {
                devlog(&format!("envelope received ({} bytes)", text.len()));
                match parse_envelope(&text) {
                    Ok(Envelope::Patch { mut table, lib_bytes_b64 }) => {
                        devlog(&format!(
                            "envelope parsed (map={} entries, dylib_b64={} bytes)",
                            table.map.len(),
                            lib_bytes_b64.len(),
                        ));
                        match materialise_patch_dylib(&lib_bytes_b64) {
                            Ok(local) => {
                                devlog(&format!(
                                    "patch dylib materialised at {}",
                                    local.display(),
                                ));
                                table.lib = local;
                                if let Ok(mut p) = PENDING.lock() {
                                    *p = Some(table);
                                    devlog("patch queued");
                                }
                                // Wake the host so a frame is
                                // scheduled — `take_pending_patch`
                                // only runs inside `tick_callback`
                                // and the TASM thread is idle when
                                // nothing else is happening.
                                whisker_runtime::signal::wake_runtime();
                            }
                            Err(e) => {
                                devlog(&format!("could not materialise patch dylib: {e}"));
                            }
                        }
                    }
                    Err(e) => devlog(&format!("malformed envelope: {e}")),
                }
            }
            Message::Close(_) => return Ok(()),
            _ => {} // ignore Binary / Ping / Pong for now
        }
    }
    Ok(())
}

/// Decode the base64 patch dylib payload, write it to a local file
/// under the app's cache dir, and return the local path. The
/// returned path is what `table.lib` gets overwritten with, so
/// `subsecond::apply_patch`'s `dlopen` sees a real on-device file.
///
/// File naming uses a monotonic counter + timestamp so multiple
/// patches in one session don't collide; old files are left around
/// (cleaned up when the OS reclaims the cache dir). Total disk use
/// per session is tiny — each patch is ~tens of KB.
fn materialise_patch_dylib(
    lib_bytes_b64: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    use base64::Engine;
    use std::sync::atomic::{AtomicU64, Ordering};

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(lib_bytes_b64)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("base64 decode: {e}").into()
        })?;
    let dir = patch_cache_dir().ok_or_else(
        || -> Box<dyn std::error::Error + Send + Sync> {
            "could not resolve a writable cache dir".into()
        },
    )?;
    std::fs::create_dir_all(&dir)?;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("patch-{ts}-{n}.so"));
    std::fs::write(&path, &bytes)?;
    Ok(path)
}

/// Resolve a writable, dlopen-able directory for patch dylibs.
///
/// On Android, `/data/data/<package>/cache/whisker-patches/` is the
/// canonical "owned by this app process" location. The package
/// name comes from `/proc/self/cmdline` (the Linux process-init
/// name Android writes there). On other platforms (host POC builds),
/// `$TMPDIR/whisker-patches/` is enough.
fn patch_cache_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    {
        let cmdline = std::fs::read_to_string("/proc/self/cmdline").ok()?;
        let pkg = cmdline
            .split('\0')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !pkg.is_empty() {
            return Some(
                std::path::PathBuf::from(format!("/data/data/{pkg}/cache/whisker-patches")),
            );
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
enum Envelope {
    Patch {
        #[serde(deserialize_with = "deserialize_jump_table")]
        table: JumpTable,
        /// Base64 of the patch dylib bytes. We decode + write to a
        /// local cache file and rewrite `table.lib` to the local
        /// path before queuing — `subsecond::apply_patch` calls
        /// `dlopen(table.lib)` and the host path obviously doesn't
        /// exist on the device.
        lib_bytes_b64: String,
    },
}

/// Counterpart of `whisker-dev-server::server::wire_jump_table::serialize`.
/// Reads the address map as a JSON array of `[old, new]` pairs and
/// reconstructs the `subsecond_types::JumpTable`. See the server
/// side for the JSON-object-vs-array rationale.
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
        // The wire format encodes `map` as an array of [old, new]
        // pairs — see deserialize_jump_table for the rationale.
        let json = r#"{
            "kind": "patch",
            "table": {
                "lib": "/tmp/some-patch.dylib",
                "map": [],
                "aslr_reference": 4294967296,
                "new_base_address": 8589934592,
                "ifunc_count": 0
            },
            "lib_bytes_b64": ""
        }"#;
        let env = parse_envelope(json).expect("should parse");
        match env {
            Envelope::Patch { table, lib_bytes_b64 } => {
                assert_eq!(
                    table.lib.to_string_lossy(),
                    "/tmp/some-patch.dylib",
                );
                assert_eq!(table.aslr_reference, 0x1_0000_0000);
                assert_eq!(table.new_base_address, 0x2_0000_0000);
                assert_eq!(table.ifunc_count, 0);
                assert!(table.map.is_empty());
                assert!(lib_bytes_b64.is_empty());
            }
        }
    }

    #[test]
    fn parses_an_envelope_with_a_non_empty_address_map() {
        let json = r#"{
            "kind": "patch",
            "table": {
                "lib": "/tmp/p.so",
                "map": [[100, 200], [300, 400]],
                "aslr_reference": 0,
                "new_base_address": 0,
                "ifunc_count": 0
            },
            "lib_bytes_b64": "AAECAw=="
        }"#;
        let env = parse_envelope(json).expect("should parse");
        let Envelope::Patch { table, lib_bytes_b64 } = env;
        assert_eq!(table.map.len(), 2);
        assert_eq!(table.map.get(&100), Some(&200));
        assert_eq!(table.map.get(&300), Some(&400));
        assert_eq!(lib_bytes_b64, "AAECAw==");
    }

    #[test]
    fn materialise_patch_dylib_writes_bytes_to_cache_and_returns_path() {
        use base64::Engine;
        // Pick a small known byte sequence so we can verify the
        // round-trip; base64-encode it.
        let payload = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00";
        let b64 = base64::engine::general_purpose::STANDARD.encode(payload);
        let path = materialise_patch_dylib(&b64).expect("write");
        let read_back = std::fs::read(&path).unwrap();
        assert_eq!(read_back, payload);
        // Cleanup so repeated runs don't accumulate.
        let _ = std::fs::remove_file(&path);
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
