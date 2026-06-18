//! Capture stdout/stderr from device-side Rust code and forward over
//! the dev-server WebSocket.
//!
//! `whisker run` previously surfaced only its own dev-loop status —
//! a `println!` from the user's app went to fd 1, which on Android is
//! redirected to `/dev/null` by the Android runtime and on iOS is
//! sunk into the simulator container with no host-side observability.
//! Users had to attach a separate terminal (`adb logcat` / `simctl log
//! stream`) to read their own code's output. This module fixes that:
//! it installs a pipe over stdout/stderr at app bootstrap, reads the
//! pipe on a background thread, and forwards each completed line over
//! the dev-server WebSocket to `whisker-cli`.
//!
//! ## What gets captured
//!
//! Anything that eventually writes to fd 1 / fd 2:
//!
//! - `println!` / `eprintln!` / `dbg!`
//! - `log` crate macros (`log::info!` etc.) with the standard backends —
//!   `env_logger`, `simple_logger`, `pretty_env_logger` all emit to
//!   stderr
//! - `tracing` macros with the default `fmt::Subscriber` (writes to
//!   stderr)
//! - Rust's default panic hook (`set_hook` users may override this)
//! - C FFI `printf` / `fprintf(stderr, …)` from third-party native libs
//!
//! Not captured:
//!
//! - Backends that bypass stdio and target the OS log directly
//!   (`tracing-android` / `tracing-oslog` / `android_logger`,
//!   `__android_log_write` / `os_log` called from FFI). Users who pick
//!   those backends already opted into platform-log delivery; they
//!   can read those with `adb logcat` / `simctl log stream`.
//!
//! ## Fan-out
//!
//! Each captured line is also written back to the original
//! stdout/stderr fd (saved via `dup` before the redirect) and pushed
//! to `__android_log_write` / `syslog`. That way `adb logcat -s
//! whisker-stdout` / `simctl spawn booted log stream` still surface
//! the same lines for users who prefer external tools, and the
//! Xcode console attached to a simulator launch keeps working.
//!
//! ## Order
//!
//! [`start_log_capture`] must be called **before** any other code
//! emits to stdout/stderr — calls that fire earlier go to the
//! original (typically /dev/null on Android) destination. The
//! canonical entry is `whisker_driver::lynx::bootstrap` right after
//! the driver attach, before the first user component renders.

use std::collections::VecDeque;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::Notify;

/// Which stream a captured line came from. Carried over the wire so
/// `whisker-cli` can colour-code stdout vs stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    /// Stable on-wire string. Mirrored by the server-side parser.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// One captured line. The reader thread strips the trailing `\n`
/// (and `\r\n` on macOS terminals) before pushing, so `text` is the
/// payload only.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: Stream,
    pub text: String,
    /// Microseconds since UNIX_EPOCH, stamped on the device at the
    /// moment the line was completed. Useful for interleaving with
    /// host-side events. `0` if the system clock is unavailable.
    pub ts_micros: u128,
}

/// Bounded ring buffer shared between the reader threads and the WS
/// session task. Drop-oldest on overflow: pre-connect buffering
/// shouldn't crowd out fresh state when the buffer fills. `Notify`
/// wakes the session task as soon as a line lands.
struct LogBuffer {
    inner: Mutex<VecDeque<LogLine>>,
    notify: Notify,
    capacity: usize,
}

impl LogBuffer {
    fn push(&self, line: LogLine) {
        let Ok(mut g) = self.inner.lock() else {
            // Poisoned mutex — drop the line rather than propagating
            // a panic out of the reader thread.
            return;
        };
        if g.len() >= self.capacity {
            g.pop_front();
        }
        g.push_back(line);
        // `notify_one` is cheap and idempotent — extra wakeups are
        // harmless because the drainer re-checks under lock.
        self.notify.notify_one();
    }

    async fn drain(&self) -> Vec<LogLine> {
        loop {
            let drained: Vec<LogLine> = {
                let Ok(mut g) = self.inner.lock() else {
                    return Vec::new();
                };
                g.drain(..).collect()
            };
            if !drained.is_empty() {
                return drained;
            }
            self.notify.notified().await;
        }
    }
}

static LOG_BUFFER: OnceLock<Arc<LogBuffer>> = OnceLock::new();

/// ~1024 lines × typical short-string ~80 B ≈ 80 KB worst case.
/// Bounded to backstop runaway log spam (panic loop, accidental
/// `loop { println!() }`) before connect.
const BUFFER_CAPACITY: usize = 1024;

/// Install the stdout/stderr capture pipes. Idempotent — subsequent
/// calls are no-ops. Returns `true` the first time, `false` on
/// repeat calls.
///
/// Failures inside the install (pipe / dup / dup2 / thread spawn)
/// are logged through [`super::hot_reload::devlog`] but never
/// panic. A failed install leaves the original fds intact — the dev
/// loop continues to function (just without device-side log capture).
pub fn start_log_capture() -> bool {
    static INITIALIZED: AtomicBool = AtomicBool::new(false);
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return false;
    }

    let buffer = Arc::new(LogBuffer {
        inner: Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
        notify: Notify::new(),
        capacity: BUFFER_CAPACITY,
    });
    // `OnceLock::set` only fails when the slot was already populated.
    // The `INITIALIZED` swap above guarantees we're first, so the
    // `is_err` branch is unreachable in practice; treat it as a
    // defensive no-op.
    let _ = LOG_BUFFER.set(Arc::clone(&buffer));

    if let Err(e) = install_pipe(libc::STDOUT_FILENO, Stream::Stdout, Arc::clone(&buffer)) {
        super::hot_reload::devlog(&format!("stdout capture install failed: {e}"));
    }
    if let Err(e) = install_pipe(libc::STDERR_FILENO, Stream::Stderr, Arc::clone(&buffer)) {
        super::hot_reload::devlog(&format!("stderr capture install failed: {e}"));
    }
    true
}

/// Drain pending captured log lines, awaiting at least one if the
/// buffer is currently empty. Used by the hot-reload WS session task
/// to forward batched log frames to the server.
///
/// If [`start_log_capture`] wasn't called (capture disabled / install
/// failed), this returns a future that never resolves — the session
/// task's `select!` arm parks forever and the other arms keep
/// running unchanged.
pub(crate) async fn drain_pending_logs() -> Vec<LogLine> {
    match LOG_BUFFER.get() {
        Some(b) => b.drain().await,
        None => futures_util::future::pending::<Vec<LogLine>>().await,
    }
}

fn install_pipe(target_fd: c_int, stream: Stream, buffer: Arc<LogBuffer>) -> std::io::Result<()> {
    // POSIX `pipe(2)` — two fds, [0] = read end, [1] = write end.
    let mut fds: [c_int; 2] = [-1, -1];
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Snapshot the original fd so the reader thread can fan-out the
    // captured bytes back to whatever was wired up before us (Xcode
    // console on iOS sim, /dev/null on Android, the host terminal on
    // desktop). `dup` allocates a fresh fd pointing at the same open
    // file description; the original target_fd then becomes safe to
    // replace via `dup2`.
    let original_fd = unsafe { libc::dup(target_fd) };
    if original_fd == -1 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
        return Err(err);
    }

    // Atomically replace target_fd with a duplicate of write_fd.
    // `dup2` closes target_fd first if it's open. After this call,
    // anything that writes to fd `target_fd` (STDOUT_FILENO /
    // STDERR_FILENO) goes into our pipe.
    if unsafe { libc::dup2(write_fd, target_fd) } == -1 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
            libc::close(original_fd);
        }
        return Err(err);
    }
    // We have a duplicate at target_fd now; drop the original
    // write_fd handle so we don't leak it.
    unsafe {
        libc::close(write_fd);
    }

    let thread_name = match stream {
        Stream::Stdout => "whisker-log-stdout",
        Stream::Stderr => "whisker-log-stderr",
    };
    std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || reader_loop(read_fd, original_fd, stream, buffer))
        .map(|_| ())?;
    Ok(())
}

fn reader_loop(read_fd: c_int, original_fd: c_int, stream: Stream, buffer: Arc<LogBuffer>) {
    let mut read_buf = [0u8; 4096];
    // Bytes from a previous read that didn't end on a newline yet.
    // Held across iterations so a `println!` that straddles a 4 KB
    // boundary surfaces as one logical line, not two.
    let mut partial: Vec<u8> = Vec::new();
    loop {
        let n = unsafe { libc::read(read_fd, read_buf.as_mut_ptr() as *mut _, read_buf.len()) };
        if n == -1 {
            // EINTR = signal interrupted the syscall before any bytes
            // were transferred. The POSIX idiom is to retry; without
            // it a stray signal silently kills log capture for the
            // rest of the session. Other errors (EBADF, EFAULT, …)
            // mean the pipe is unrecoverably broken — exit.
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return;
        }
        if n == 0 {
            // EOF: every write end of the pipe is closed. In normal
            // operation this should never fire — the process keeps
            // STDOUT_FILENO / STDERR_FILENO open for life — but
            // returning cleanly is safer than spinning.
            return;
        }
        let chunk = &read_buf[..n as usize];
        // Fan out RAW bytes to the original fd FIRST so consumers of
        // the original stdout/stderr (Xcode console attached to the
        // sim launch, host terminal on a desktop dev build) see them
        // exactly as the producer wrote them — no line-buffering
        // delay, no partial-line withholding.
        unsafe {
            let _ = libc::write(original_fd, chunk.as_ptr() as *const _, chunk.len());
        }

        // Line-split for the WS frame stream. Partial-line carryover
        // means each `println!` reaches the host as one line even if
        // a large payload spans multiple `read` chunks.
        partial.extend_from_slice(chunk);
        while let Some(nl_pos) = partial.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = partial.drain(..=nl_pos).collect();
            // Trim trailing newline + any \r before it.
            while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
                line.pop();
            }
            // Use `from_utf8_lossy` so a non-UTF-8 byte (raw bytes
            // from a C lib, locale mismatch) shows up as U+FFFD
            // rather than silently dropping the line.
            let text = match String::from_utf8(line) {
                Ok(s) => s,
                Err(e) => String::from_utf8_lossy(&e.into_bytes()).into_owned(),
            };
            push_platform_log(stream, &text);
            let ts_micros = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_micros())
                .unwrap_or(0);
            buffer.push(LogLine {
                stream,
                text,
                ts_micros,
            });
        }
    }
}

#[cfg(target_os = "android")]
fn push_platform_log(stream: Stream, line: &str) {
    // Fan out to logcat so external observers (`adb logcat -s
    // whisker-stdout`) see the same lines. Tag chosen distinct from
    // `whisker-dev` (the hot-reload runtime's own diagnostics) so
    // users can filter cleanly.
    unsafe extern "C" {
        fn __android_log_write(
            prio: std::os::raw::c_int,
            tag: *const std::os::raw::c_char,
            text: *const std::os::raw::c_char,
        ) -> std::os::raw::c_int;
    }
    const ANDROID_LOG_INFO: std::os::raw::c_int = 4;
    let tag: &[u8] = match stream {
        Stream::Stdout => b"whisker-stdout\0",
        Stream::Stderr => b"whisker-stderr\0",
    };
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
fn push_platform_log(stream: Stream, line: &str) {
    // Fan out to syslog so `simctl spawn booted log stream` surfaces
    // the same lines. Prefix mirrors the Android tag layout.
    unsafe extern "C" {
        fn syslog(priority: std::os::raw::c_int, fmt: *const std::os::raw::c_char, ...);
    }
    const LOG_INFO: std::os::raw::c_int = 6;
    let prefix: &[u8] = match stream {
        Stream::Stdout => b"[whisker-stdout] ",
        Stream::Stderr => b"[whisker-stderr] ",
    };
    let mut buf: Vec<u8> = Vec::with_capacity(prefix.len() + line.len() + 1);
    buf.extend_from_slice(prefix);
    buf.extend_from_slice(line.as_bytes());
    buf.push(0);
    let fmt = b"%s\0";
    unsafe {
        syslog(LOG_INFO, fmt.as_ptr() as *const _, buf.as_ptr());
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn push_platform_log(_stream: Stream, _line: &str) {
    // Host / Linux desktop: the `libc::write(original_fd, …)` above
    // already lands lines on the user's terminal, which is the only
    // sink that matters off-device. No additional platform log.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, timeout};

    /// Construct a fresh LogBuffer for direct testing — the global
    /// `LOG_BUFFER` static can only be initialised once per process,
    /// so unit tests build their own instance.
    fn fresh_buffer(capacity: usize) -> Arc<LogBuffer> {
        Arc::new(LogBuffer {
            inner: Mutex::new(VecDeque::with_capacity(capacity)),
            notify: Notify::new(),
            capacity,
        })
    }

    #[tokio::test]
    async fn buffer_drains_what_was_pushed() {
        let b = fresh_buffer(8);
        b.push(LogLine {
            stream: Stream::Stdout,
            text: "hello".into(),
            ts_micros: 1,
        });
        b.push(LogLine {
            stream: Stream::Stderr,
            text: "world".into(),
            ts_micros: 2,
        });
        let drained = timeout(Duration::from_secs(1), b.drain()).await.unwrap();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].text, "hello");
        assert_eq!(drained[0].stream, Stream::Stdout);
        assert_eq!(drained[1].text, "world");
        assert_eq!(drained[1].stream, Stream::Stderr);
    }

    #[tokio::test]
    async fn buffer_drops_oldest_when_capacity_exceeded() {
        let b = fresh_buffer(3);
        for i in 0..5u32 {
            b.push(LogLine {
                stream: Stream::Stdout,
                text: format!("line {i}"),
                ts_micros: i as u128,
            });
        }
        let drained = timeout(Duration::from_secs(1), b.drain()).await.unwrap();
        // Capacity 3: lines 0 and 1 should have been dropped.
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].text, "line 2");
        assert_eq!(drained[1].text, "line 3");
        assert_eq!(drained[2].text, "line 4");
    }

    #[tokio::test]
    async fn drain_blocks_until_pushed() {
        let b = fresh_buffer(8);
        // Spawn a producer that pushes after a delay.
        let b_clone = Arc::clone(&b);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            b_clone.push(LogLine {
                stream: Stream::Stdout,
                text: "delayed".into(),
                ts_micros: 0,
            });
        });
        let drained = timeout(Duration::from_secs(2), b.drain()).await.unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].text, "delayed");
    }

    #[test]
    fn stream_as_wire_is_stable() {
        assert_eq!(Stream::Stdout.as_wire(), "stdout");
        assert_eq!(Stream::Stderr.as_wire(), "stderr");
    }
}
