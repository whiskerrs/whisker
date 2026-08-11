//! Capture stdout/stderr from device-side Rust code and forward over
//! the dev-server WebSocket, so a `println!` in user code reaches the
//! `whisker run` terminal (on Android fd 1 goes to `/dev/null`, on iOS
//! into the simulator container).
//!
//! ## What gets captured
//!
//! Anything that eventually writes to fd 1 / fd 2: `println!` /
//! `eprintln!` / `dbg!`, `log` and `tracing` macros with their
//! stdio-based default backends, the default panic hook, and C FFI
//! `printf` / `fprintf(stderr, …)`. Backends that target the OS log
//! directly (`android_logger`, `tracing-oslog`, …) are not captured —
//! those already deliver to `adb logcat` / `log stream`.
//!
//! ## Fan-out
//!
//! Each captured line is also written back to the original
//! stdout/stderr fd (saved via `dup` before the redirect) and pushed
//! to `__android_log_write` / `syslog`, so external tools (`adb
//! logcat`, `simctl … log stream`, the Xcode console) keep seeing the
//! same lines.
//!
//! ## Order
//!
//! [`start_log_capture`] must be called **before** any other code
//! emits to stdout/stderr — earlier writes go to the original
//! destination. The canonical entry is `whisker_driver::lynx::bootstrap`
//! right after the driver attach.

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

/// One captured line, trailing newline (and `\r`) stripped.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: Stream,
    pub text: String,
    /// Microseconds since UNIX_EPOCH, stamped on the device when the
    /// line was completed. `0` if the system clock is unavailable.
    pub ts_micros: u128,
}

/// Bounded ring buffer shared between the reader threads and the WS
/// session task. Drop-oldest on overflow: pre-connect buffering
/// shouldn't crowd out fresh state when the buffer fills.
struct LogBuffer {
    inner: Mutex<VecDeque<LogLine>>,
    notify: Notify,
    capacity: usize,
}

impl LogBuffer {
    fn push(&self, line: LogLine) {
        let Ok(mut g) = self.inner.lock() else {
            // Poisoned mutex — drop the line rather than propagating a
            // panic out of the reader thread.
            return;
        };
        if g.len() >= self.capacity {
            g.pop_front();
        }
        g.push_back(line);
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

/// Bounded to backstop runaway log spam (panic loop, `loop {
/// println!() }`) before the WS session connects.
const BUFFER_CAPACITY: usize = 1024;

/// Install the stdout/stderr capture pipes. Idempotent — returns
/// `true` the first time, `false` on repeat calls.
///
/// Failures inside the install (pipe / dup / dup2 / thread spawn) are
/// logged through [`super::hot_reload::devlog`] but never panic. A
/// failed install leaves the original fds intact.
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
/// buffer is currently empty.
///
/// If [`start_log_capture`] wasn't called (capture disabled / install
/// failed), this returns a future that never resolves — the session
/// task's `select!` arm parks forever and the other arms keep running.
pub(crate) async fn drain_pending_logs() -> Vec<LogLine> {
    match LOG_BUFFER.get() {
        Some(b) => b.drain().await,
        None => futures_util::future::pending::<Vec<LogLine>>().await,
    }
}

fn install_pipe(target_fd: c_int, stream: Stream, buffer: Arc<LogBuffer>) -> std::io::Result<()> {
    let mut fds: [c_int; 2] = [-1, -1];
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Snapshot the original fd so the reader thread can fan the
    // captured bytes back out to whatever was wired up before us.
    let original_fd = unsafe { libc::dup(target_fd) };
    if original_fd == -1 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
        return Err(err);
    }

    if unsafe { libc::dup2(write_fd, target_fd) } == -1 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
            libc::close(original_fd);
        }
        return Err(err);
    }
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
    // Carryover so a line straddling a read boundary surfaces as one
    // logical line.
    let mut partial: Vec<u8> = Vec::new();
    loop {
        let n = unsafe { libc::read(read_fd, read_buf.as_mut_ptr() as *mut _, read_buf.len()) };
        if n == -1 {
            // Retry on EINTR; other errors mean the pipe is
            // unrecoverably broken.
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return;
        }
        if n == 0 {
            return;
        }
        let chunk = &read_buf[..n as usize];
        // Fan out RAW bytes to the original fd FIRST so its consumers
        // see them exactly as written — no line-buffering delay, no
        // partial-line withholding.
        unsafe {
            let _ = libc::write(original_fd, chunk.as_ptr() as *const _, chunk.len());
        }

        partial.extend_from_slice(chunk);
        while let Some(nl_pos) = partial.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = partial.drain(..=nl_pos).collect();
            while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
                line.pop();
            }
            // Lossy so a non-UTF-8 byte shows up as U+FFFD rather than
            // dropping the line.
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
    // Tags are distinct from `whisker-dev` (the runtime's own
    // diagnostics) so users can filter cleanly.
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
    // The write to `original_fd` already lands on the terminal, which
    // is the only sink that matters off-device.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, timeout};

    /// The global `LOG_BUFFER` can only be initialised once per
    /// process, so tests build their own instance.
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
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].text, "line 2");
        assert_eq!(drained[1].text, "line 3");
        assert_eq!(drained[2].text, "line 4");
    }

    #[tokio::test]
    async fn drain_blocks_until_pushed() {
        let b = fresh_buffer(8);
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
