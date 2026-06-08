//! Inline-viewport TUI for `whisker run`.
//!
//! ## Design (vs. the alternate-screen design in #187)
//!
//! Full-screen ratatui owned the terminal and erased scrollback;
//! cargo / gradle / xcodebuild log bursts during a build no longer
//! fit in any reasonable pane and the user couldn't scroll back
//! through them. The codex-rs TUI solves this by anchoring a small
//! "live region" at the bottom and pushing everything else into the
//! terminal's *normal* scrollback via ANSI scroll-region tricks. We
//! get the same shape for free out of ratatui 0.29's
//! [`Viewport::Inline`] + [`Terminal::insert_before`] (with the
//! `scrolling-regions` feature enabled so we land on the DECSTBM
//! fast path).
//!
//! Layout while the cli is running:
//!
//! ```text
//! ── terminal scrollback (mouse-wheel scrollable) ──────────────────
//!   …earlier shell output…
//!   ▶ Setup
//!   ✓ Sync gen/ios            124ms
//!   ▶ Initial build
//!   warning: unused import: `Foo`   ← captured cargo stderr
//!   ✓ Initial build           6.2s
//!   ▶ Install + launch
//!   …
//! ── live region (LIVE_HEIGHT rows, redraws ~10Hz) ─────────────────
//!    whisker run · iOS Simulator · rs.example.bar · building · 4.1s
//!    ⠋ xcodebuild …
//!
//!    q  quit
//! ──────────────────────────────────────────────────────────────────
//! ```
//!
//! ## Subprocess output → scrollback
//!
//! cargo / gradle / xcodebuild and every `whisker_build::ui::*` call
//! write to stderr. We `dup2` `STDERR_FILENO` to a pipe whose read
//! end a dedicated thread drains line-by-line, strips ANSI escapes
//! from, and sends through an mpsc channel. The render thread drains
//! that channel each frame and calls
//! [`Terminal::insert_before`] per line, so captured output lands
//! above the live region — which the terminal's scrollback keeps for
//! us. ratatui's backend is wired to the *saved* original stderr fd
//! so its own draw escapes don't self-loop into the pipe.
//!
//! Because stderr is no longer a TTY once we `dup2` it, cargo /
//! gradle / xcodebuild automatically fall back to line-based output
//! (no in-place progress bars), which is exactly what we want for
//! scrollback.
//!
//! ## State machine
//!
//! [`AppPhase`] tracks where the dev loop is. The cli calls
//! [`TuiHandle::set_phase`] for phases it drives directly
//! (`Setup`, `Initializing`); dev-server events drive the rest via
//! [`TuiHandle::apply_event`]. Each transition emits a one-line
//! "▶ <phase>" / "✓ <phase>  Xs" history entry plus updates the live
//! header.

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{poll, read, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
    ExecutableCommand,
};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::Write;
use std::os::raw::c_int;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Height of the live region in rows. The header, current step,
/// dev-server info and key hint together comfortably fit in 6 rows;
/// taller cuts scrollback density and saves nothing.
const LIVE_HEIGHT: u16 = 6;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ============================================================================
// Public state model
// ============================================================================

/// Which phase of the dev loop the user is currently watching. Drives
/// the live-region header label + spinner color.
#[derive(Debug, Clone)]
pub enum AppPhase {
    /// Pre-dev-server cli work: `sync_for_target` (gen tree + plugin
    /// build). Driven by explicit `TuiHandle::set_phase` calls.
    Setup,
    /// dev-server's setup (WS bind, watcher, capture shim resolve).
    /// Brief; usually flips to `Building` within ~100ms once
    /// `Event::BuildingFull` arrives.
    Initializing,
    /// `cargo` + `gradle` / `xcodebuild` in flight.
    Building {
        started_at: Instant,
        kind: BuildKind,
    },
    /// dev-server bound, initial build succeeded, watching for source
    /// changes.
    Idle,
    /// Tier 1 hot-patch in flight. Phase exit is signalled by
    /// `Event::PatchSent` or a Tier 2 fallback's `Event::BuildingFull`.
    Patching { started_at: Instant },
    /// Build failed. The live region surfaces the cause and the cli
    /// is about to exit non-zero.
    Failed { phase: String, reason: String },
}

#[derive(Debug, Clone, Copy)]
pub enum BuildKind {
    Initial,
    Rebuild,
}

/// Outcome of a completed step. Pushed to scrollback by
/// [`TuiHandle::finish_step`]; never rendered in the live region.
#[derive(Debug, Clone, Copy)]
pub enum StepStatus {
    Done,
    Failed,
    Skipped,
}

/// Snapshot the render thread reads on every frame to draw the live
/// region. Mutated under a `Mutex` from the cli thread; everything
/// that needs to *enter scrollback* goes through the history channel
/// instead so the render thread can call `insert_before` from the
/// thread that owns the ratatui terminal.
#[derive(Debug, Clone)]
pub struct LiveState {
    pub target: String,
    pub bundle: String,
    pub phase: AppPhase,
    /// Label of the in-progress step (e.g. "xcodebuild …"). Cleared
    /// when the step finishes.
    pub current_step: Option<String>,
    pub ws_addr: Option<String>,
    pub watching: Vec<String>,
    pub client_count: usize,
    pub last_build: Option<String>,
    pub last_patch: Option<String>,
    pub should_quit: bool,
    /// `true` when the quit was triggered by a user keypress
    /// (`q` / Esc / Ctrl-C), `false` when the cli called
    /// `TuiHandle::request_quit` after its own work finished or
    /// failed. The render thread uses this to decide whether to
    /// force-exit the process after shutdown — the dev-server
    /// `rt.block_on(server.run())` call in the main thread otherwise
    /// blocks forever, so without a hard exit `q` would tear down the
    /// TUI but leave the process running.
    pub user_initiated_quit: bool,
}

impl LiveState {
    pub fn new(target: impl Into<String>, bundle: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            bundle: bundle.into(),
            phase: AppPhase::Setup,
            current_step: None,
            ws_addr: None,
            watching: Vec::new(),
            client_count: 0,
            last_build: None,
            last_patch: None,
            should_quit: false,
            user_initiated_quit: false,
        }
    }
}

/// One entry the render thread will push into the terminal's
/// scrollback via [`Terminal::insert_before`].
#[derive(Debug, Clone)]
pub enum HistoryItem {
    /// Phase-transition heading: "▶ Initial build".
    PhaseEnter(String),
    /// Phase-completion summary: "✓ Initial build  6.2s".
    PhaseDone {
        label: String,
        status: StepStatus,
        elapsed: Duration,
    },
    /// A completed step: "✓ Sync gen/ios       124ms".
    Step {
        label: String,
        status: StepStatus,
        elapsed: Duration,
    },
    /// One line captured from the dup2'd stderr pipe (with ANSI
    /// escapes already stripped).
    CapturedStderr(String),
    /// Device log forwarded from the dev-server.
    DeviceLog { stream: String, line: String },
    /// One-shot failure description for the scrollback.
    Failure(String),
}

// ============================================================================
// Event → state machine
// ============================================================================

/// Apply a dev-server event to the live state and emit any history
/// entries the transition implies. Pure — the test suite exercises
/// it without any terminal io.
pub fn apply_event(
    state: &mut LiveState,
    event: &whisker_dev_server::Event,
    history: &mut Vec<HistoryItem>,
) {
    use whisker_dev_server::Event;
    match event {
        Event::Started => {
            // dev-server is up. The cli's explicit `set_phase` calls
            // own the transition out of Initializing — respect that
            // ordering so we don't race the "▶ Initial build" entry.
        }
        Event::BuildingFull => {
            let kind = match state.phase {
                AppPhase::Setup | AppPhase::Initializing => BuildKind::Initial,
                _ => BuildKind::Rebuild,
            };
            state.phase = AppPhase::Building {
                started_at: Instant::now(),
                kind,
            };
            // Phase entry markers come from `whisker_build::ui::section`
            // ("──── Initial build ────"), which lands in scrollback
            // via the stderr capture. Emitting a second "▶ Initial
            // build" line here would just duplicate it. Phase *exit*
            // (the `✓ Initial build  6.2s` summary) is unique to our
            // TUI and is emitted on `BuildSucceeded`.
            state.current_step = None;
        }
        Event::BuildSucceeded => {
            if let AppPhase::Building { started_at, kind } = &state.phase {
                let elapsed = started_at.elapsed();
                let label = match kind {
                    BuildKind::Initial => "Initial build",
                    BuildKind::Rebuild => "Rebuild",
                };
                history.push(HistoryItem::PhaseDone {
                    label: label.into(),
                    status: StepStatus::Done,
                    elapsed,
                });
                state.last_build = Some(format!(
                    "{} · {}",
                    if matches!(kind, BuildKind::Initial) {
                        "initial"
                    } else {
                        "rebuild"
                    },
                    fmt_elapsed(elapsed)
                ));
            }
            state.phase = AppPhase::Idle;
            state.current_step = None;
        }
        Event::BuildFailed(msg) => {
            let phase = "build".to_string();
            history.push(HistoryItem::PhaseDone {
                label: phase.clone(),
                status: StepStatus::Failed,
                elapsed: Duration::ZERO,
            });
            history.push(HistoryItem::Failure(msg.clone()));
            state.phase = AppPhase::Failed {
                phase,
                reason: msg.clone(),
            };
            state.current_step = None;
        }
        Event::ClientConnected => {
            state.client_count = state.client_count.saturating_add(1);
        }
        Event::ClientDisconnected => {
            state.client_count = state.client_count.saturating_sub(1);
        }
        Event::PatchSent => {
            if let AppPhase::Patching { started_at } = &state.phase {
                let elapsed = started_at.elapsed();
                history.push(HistoryItem::PhaseDone {
                    label: "Hot patch".into(),
                    status: StepStatus::Done,
                    elapsed,
                });
                state.last_patch = Some(fmt_elapsed(elapsed));
            }
            state.phase = AppPhase::Idle;
            state.current_step = None;
        }
        Event::DeviceLog { stream, line, .. } => {
            history.push(HistoryItem::DeviceLog {
                stream: stream.clone(),
                line: line.clone(),
            });
        }
    }
}

// ============================================================================
// TuiHandle: cli-side facade
// ============================================================================

/// Cheap-to-clone handle the cli code passes around to update the
/// live region and to commit lines to scrollback. Thread-safe;
/// non-blocking on send (a slow render thread can't stall the
/// build).
#[derive(Clone)]
pub struct TuiHandle {
    live: Arc<Mutex<LiveState>>,
    tx: Sender<HistoryItem>,
}

impl TuiHandle {
    fn with<F: FnOnce(&mut LiveState)>(&self, f: F) {
        if let Ok(mut g) = self.live.lock() {
            f(&mut g);
        }
    }
    fn send(&self, item: HistoryItem) {
        // Disconnected receiver is harmless — we just stop emitting.
        let _ = self.tx.send(item);
    }

    /// Enter `phase`. Updates the live region's phase label/spinner
    /// color and clears any in-progress step display. Does NOT push
    /// a scrollback entry — `whisker_build::ui::section` already
    /// prints labeled phase boundaries that flow into scrollback via
    /// the stderr capture, so a duplicate "▶ <label>" line would
    /// just be noise. Event-driven phase completions (build
    /// finished, patch sent) still emit `HistoryItem::PhaseDone`
    /// via [`apply_event`] — those carry elapsed-time data that
    /// `whisker_build::ui` doesn't.
    pub fn set_phase(&self, phase: AppPhase) {
        self.with(|s| {
            s.phase = phase;
            s.current_step = None;
        });
    }

    /// Begin a step. Updates `current_step` in the live region; on
    /// `finish_step` the row gets committed to scrollback.
    pub fn start_step(&self, label: impl Into<String>) {
        let label = label.into();
        self.with(|s| {
            s.current_step = Some(label);
        });
    }

    /// Finish the currently-displayed step. The row is pushed to
    /// scrollback as "✓ <label>  <elapsed>"; the live region clears
    /// `current_step`.
    pub fn finish_step(&self, label: impl Into<String>, status: StepStatus, elapsed: Duration) {
        let label = label.into();
        self.with(|s| s.current_step = None);
        self.send(HistoryItem::Step {
            label,
            status,
            elapsed,
        });
    }

    pub fn apply_event(&self, event: &whisker_dev_server::Event) {
        let mut history: Vec<HistoryItem> = Vec::new();
        self.with(|s| apply_event(s, event, &mut history));
        for h in history {
            self.send(h);
        }
    }

    pub fn set_dev_server(&self, ws_addr: impl Into<String>, watching: Vec<String>) {
        let ws_addr = ws_addr.into();
        self.with(|s| {
            s.ws_addr = Some(ws_addr);
            s.watching = watching;
        });
    }

    pub fn should_quit(&self) -> bool {
        self.live.lock().map(|s| s.should_quit).unwrap_or(false)
    }

    pub fn request_quit(&self) {
        self.with(|s| s.should_quit = true);
    }

    /// Test-only: pull a snapshot of the live state. Avoid in
    /// production code — render is the only legitimate reader.
    #[cfg(test)]
    pub fn snapshot(&self) -> LiveState {
        self.live.lock().unwrap().clone()
    }
}

// ============================================================================
// Tui: terminal owner + render loop
// ============================================================================

/// Writer for ratatui's crossterm backend. Writes to the *saved*
/// (pre-dup2) stderr fd so the terminal's draw escapes don't loop
/// back into the capture pipe.
struct OriginalStderr(c_int);

impl Write for OriginalStderr {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::write(self.0, buf.as_ptr() as *const _, buf.len()) };
        if n < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct Tui {
    terminal: Terminal<CrosstermBackend<OriginalStderr>>,
    live: Arc<Mutex<LiveState>>,
    rx: Receiver<HistoryItem>,
    saved_stderr_fd: c_int,
    spinner_idx: usize,
}

impl Tui {
    /// Set up the inline TUI, install the stderr capture, and hand
    /// back a `(Tui, TuiHandle)` pair. The cli keeps `TuiHandle` and
    /// passes `Tui` to a dedicated OS thread that runs the render
    /// loop (ratatui's `Terminal` isn't `Send` once it has a backend
    /// holding raw fds — keep it pinned to one thread).
    pub fn start(target: String, bundle: String) -> Result<(Self, TuiHandle)> {
        let (saved_stderr_fd, capture_read_fd) =
            install_stderr_capture().context("install stderr capture")?;
        install_terminal_cleanup_once(saved_stderr_fd);

        enable_raw_mode().context("enable raw mode")?;
        let mut original = OriginalStderr(saved_stderr_fd);
        original.execute(cursor::Hide).context("hide cursor")?;

        let backend = CrosstermBackend::new(original);
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(LIVE_HEIGHT),
            },
        )
        .context("create ratatui terminal (inline viewport)")?;

        let live = Arc::new(Mutex::new(LiveState::new(target, bundle)));
        let (tx, rx) = channel::<HistoryItem>();

        {
            // stderr capture → channel. Each captured line becomes
            // a `HistoryItem::CapturedStderr` once we've stripped
            // ANSI escape sequences.
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("whisker-tui-stderr-capture".into())
                .spawn(move || capture_reader_loop(capture_read_fd, tx))
                .context("spawn stderr capture reader")?;
        }

        let handle = TuiHandle {
            live: Arc::clone(&live),
            tx,
        };

        Ok((
            Self {
                terminal,
                live,
                rx,
                saved_stderr_fd,
                spinner_idx: 0,
            },
            handle,
        ))
    }

    /// Drive the render loop until `should_quit` flips (either via
    /// `q` / Esc / Ctrl-C in the terminal or via `TuiHandle::request_quit`
    /// from the cli when its work finishes).
    pub fn render_until_quit(&mut self) -> Result<()> {
        let frame_interval = Duration::from_millis(100);
        let mut last_draw = Instant::now() - frame_interval;
        loop {
            self.drain_history_into_scrollback()?;

            if last_draw.elapsed() >= frame_interval {
                self.spinner_idx = self.spinner_idx.wrapping_add(1);
                let snapshot = self.live.lock().ok().map(|g| g.clone());
                if let Some(s) = snapshot {
                    let spinner_idx = self.spinner_idx;
                    self.terminal
                        .draw(|f| render_live(f, &s, spinner_idx))
                        .context("draw live region")?;
                }
                last_draw = Instant::now();
            }

            // Short key poll — must be tight enough that pressing
            // `q` feels responsive but not so tight it pegs a core.
            // 50ms is the same budget the previous full-screen TUI
            // used; works fine.
            if poll(Duration::from_millis(50))? {
                if let CtEvent::Key(key) = read()? {
                    if matches!(key.kind, KeyEventKind::Press) {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => self.user_quit(),
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.user_quit()
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Ok(s) = self.live.lock() {
                if s.should_quit {
                    break;
                }
            }
        }
        Ok(())
    }

    fn drain_history_into_scrollback(&mut self) -> Result<()> {
        loop {
            match self.rx.try_recv() {
                Ok(item) => {
                    let lines = render_history_item(&item);
                    let height = lines.len().min(u16::MAX as usize) as u16;
                    if height == 0 {
                        continue;
                    }
                    // Move `lines` into the closure — ratatui's `Buffer`
                    // borrows the cells we copy from `Line`s.
                    self.terminal
                        .insert_before(height, move |buf| {
                            write_lines_to_buffer(buf, &lines);
                        })
                        .context("insert history line into scrollback")?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok(())
    }

    /// User-initiated quit (q / Esc / Ctrl-C from the TUI).
    /// Distinguished from a cli-initiated quit (`TuiHandle::request_quit`)
    /// so `run_until_quit`'s caller can decide to force-exit the
    /// process — the dev-server's `rt.block_on` would otherwise keep
    /// running after the TUI tears down.
    fn user_quit(&self) {
        if let Ok(mut s) = self.live.lock() {
            s.should_quit = true;
            s.user_initiated_quit = true;
        }
    }

    /// Whether the most recent quit signal came from a user keypress
    /// rather than a `TuiHandle::request_quit` call. Callers use this
    /// after `render_until_quit` returns to decide whether to
    /// `process::exit` (user quit while the dev-server was running) or
    /// to fall through and let the cli's own return path run (cli
    /// finished its work).
    pub fn was_user_quit(&self) -> bool {
        self.live
            .lock()
            .map(|s| s.user_initiated_quit)
            .unwrap_or(false)
    }

    pub fn shutdown(mut self) -> Result<()> {
        // One last drain so any final phase/Step/Failure entry the
        // cli emitted between the last render and quit lands in
        // scrollback before the live region disappears.
        let _ = self.drain_history_into_scrollback();
        // ratatui clears its viewport rows on `clear`; on Inline
        // mode this leaves the scrollback intact and just blanks
        // the LIVE_HEIGHT rows at the cursor. Then we restore the
        // cursor via `Terminal::show_cursor` (not a bare crossterm
        // `cursor::Show` against the saved fd) — going through the
        // terminal clears its internal `hidden_cursor` flag, which
        // otherwise causes ratatui's `Drop` impl to try the same
        // call later when the fd is already closed and we'd see
        // `Failed to show the cursor: Bad file descriptor` printed
        // to the shell.
        let _ = self.terminal.clear();
        let _ = self.terminal.show_cursor();
        let _ = disable_raw_mode();
        // Restore STDERR_FILENO to the saved fd so callers can
        // continue to `eprintln!` after the TUI is gone; close the
        // duplicated saved fd afterward. `Tui`'s drop order then
        // unwinds the ratatui Terminal cleanly (hidden_cursor is
        // already false, so Drop is a no-op).
        unsafe {
            libc::dup2(self.saved_stderr_fd, libc::STDERR_FILENO);
            libc::close(self.saved_stderr_fd);
        }
        Ok(())
    }
}

// ============================================================================
// Stderr capture
// ============================================================================

fn install_stderr_capture() -> Result<(c_int, c_int)> {
    let mut fds: [c_int; 2] = [-1, -1];
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("pipe(2)");
    }
    let read_fd = fds[0];
    let write_fd = fds[1];
    let saved_fd = unsafe { libc::dup(libc::STDERR_FILENO) };
    if saved_fd == -1 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
        return Err(e).context("dup STDERR_FILENO");
    }
    if unsafe { libc::dup2(write_fd, libc::STDERR_FILENO) } == -1 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
            libc::close(saved_fd);
        }
        return Err(e).context("dup2 over STDERR_FILENO");
    }
    unsafe {
        libc::close(write_fd);
    }
    Ok((saved_fd, read_fd))
}

fn capture_reader_loop(read_fd: c_int, tx: Sender<HistoryItem>) {
    let mut buf = [0u8; 4096];
    let mut partial: Vec<u8> = Vec::new();
    loop {
        let n = unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n == -1 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return;
        }
        if n == 0 {
            return;
        }
        let chunk = &buf[..n as usize];
        partial.extend_from_slice(chunk);
        while let Some(nl_pos) = partial.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = partial.drain(..=nl_pos).collect();
            while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
                line.pop();
            }
            let text = match String::from_utf8(line) {
                Ok(s) => s,
                Err(e) => String::from_utf8_lossy(&e.into_bytes()).into_owned(),
            };
            let text = strip_ansi(&text);
            if !text.is_empty() && tx.send(HistoryItem::CapturedStderr(text)).is_err() {
                return;
            }
        }
    }
}

/// Strip ECMA-48 CSI (`\x1b[…<final>`) and OSC (`\x1b]…\x07` or
/// `\x1b]…\x1b\\`) escapes from `s`. cargo / gradle write colored
/// output via SGR (CSI ending in `m`); without this, the captured
/// line would render as visible `^[[33mwarning…^[[0m` in the
/// scrollback. Iterates over `chars()` so multi-byte UTF-8
/// sequences (`whisker_build::ui` decorations like `▶ ✓ ·`) survive
/// intact.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut iter = s.chars().peekable();
    while let Some(c) = iter.next() {
        if c == '\x1b' {
            match iter.peek().copied() {
                Some('[') => {
                    iter.next();
                    // Consume CSI parameter bytes until a final
                    // byte in the 0x40..=0x7e range (the SGR
                    // terminator `m` lives here).
                    for ch in iter.by_ref() {
                        if matches!(ch as u32, 0x40..=0x7e) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    iter.next();
                    // Consume the OSC string until BEL (`\x07`) or
                    // ST (`ESC \`).
                    while let Some(ch) = iter.next() {
                        if ch == '\x07' {
                            break;
                        }
                        if ch == '\x1b' {
                            if matches!(iter.peek(), Some('\\')) {
                                iter.next();
                            }
                            break;
                        }
                    }
                }
                _ => {
                    // Lone ESC or unknown introducer — drop it.
                }
            }
            continue;
        }
        // Drop other C0 control characters except tab. Multi-byte
        // UTF-8 characters have a `u32` value ≥ 0x80, so they pass
        // the `>= 0x20` check unconditionally.
        if c == '\t' || (c as u32) >= 0x20 {
            out.push(c);
        }
    }
    out
}

// ============================================================================
// Cleanup hooks
// ============================================================================

fn emergency_terminal_reset(original_stderr_fd: c_int) {
    let mut o = OriginalStderr(original_stderr_fd);
    let _ = o.execute(cursor::Show);
    let _ = disable_raw_mode();
}

fn install_terminal_cleanup_once(original_stderr_fd: c_int) {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    static SAVED_FD: AtomicI32 = AtomicI32::new(-1);
    SAVED_FD.store(original_stderr_fd, Ordering::Release);
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return;
    }
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        emergency_terminal_reset(SAVED_FD.load(Ordering::Acquire));
        prev_hook(info);
    }));
    let _ = ctrlc::set_handler(|| {
        emergency_terminal_reset(SAVED_FD.load(Ordering::Acquire));
        std::process::exit(130);
    });
}

// ============================================================================
// Rendering
// ============================================================================

fn render_live(frame: &mut ratatui::Frame, state: &LiveState, spinner_idx: usize) {
    let area = frame.area();
    let lines = build_live_lines(state, spinner_idx);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn build_live_lines(state: &LiveState, spinner_idx: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header line: ` whisker run · target · bundle · phase ` with
    // phase-elapsed when the phase is wall-clock-meaningful.
    let mut header: Vec<Span<'static>> = vec![
        Span::styled(
            " whisker run ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            state.target.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::raw(state.bundle.clone()),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            phase_label(&state.phase),
            Style::default()
                .fg(phase_color(&state.phase))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(extra) = phase_elapsed(&state.phase) {
        header.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
        header.push(Span::styled(extra, Style::default().fg(Color::DarkGray)));
    }
    lines.push(Line::from(header));

    // Current step line (spinner + label) OR phase-specific info.
    match (&state.current_step, &state.phase) {
        (Some(label), _) => {
            let spinner = SPINNER_FRAMES[spinner_idx % SPINNER_FRAMES.len()];
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    spinner.to_string(),
                    Style::default().fg(phase_color(&state.phase)),
                ),
                Span::raw("  "),
                Span::raw(label.clone()),
            ]));
        }
        (None, AppPhase::Failed { reason, .. }) => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(reason.clone(), Style::default().fg(Color::Red)),
            ]));
        }
        (None, _) => {
            lines.push(Line::from(""));
        }
    }

    // Dev-server / clients info — shown once dev-server is bound,
    // regardless of phase.
    if let Some(addr) = &state.ws_addr {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("dev server  ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("ws://{addr}")),
        ]));
        let clients = format!("{} connected", state.client_count);
        let mut watching = vec![
            Span::raw(" "),
            Span::styled("clients     ", Style::default().fg(Color::DarkGray)),
            Span::raw(clients),
        ];
        if !state.watching.is_empty() {
            watching.push(Span::styled(
                "   ·   ",
                Style::default().fg(Color::DarkGray),
            ));
            watching.push(Span::styled(
                format!("watching {} path(s)", state.watching.len()),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(watching));
    } else {
        // Reserve one row so the layout doesn't jiggle when the
        // dev-server comes online mid-build.
        lines.push(Line::from(""));
    }

    // Spacer + footer hint.
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled(
            " q ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  quit", Style::default().fg(Color::DarkGray)),
    ]));

    // Truncate / pad to LIVE_HEIGHT so the viewport renders cleanly.
    lines.truncate(LIVE_HEIGHT as usize);
    while lines.len() < LIVE_HEIGHT as usize {
        lines.push(Line::from(""));
    }
    lines
}

fn render_history_item(item: &HistoryItem) -> Vec<Line<'static>> {
    match item {
        HistoryItem::PhaseEnter(label) => vec![Line::from(vec![
            Span::styled("▶ ", Style::default().fg(Color::Cyan)),
            Span::styled(label.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ])],
        HistoryItem::PhaseDone {
            label,
            status,
            elapsed,
        } => {
            let (glyph, color) = match status {
                StepStatus::Done => ("✓ ", Color::Green),
                StepStatus::Failed => ("✗ ", Color::Red),
                StepStatus::Skipped => ("○ ", Color::DarkGray),
            };
            vec![Line::from(vec![
                Span::styled(glyph, Style::default().fg(color)),
                Span::styled(label.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(fmt_elapsed(*elapsed), Style::default().fg(Color::DarkGray)),
            ])]
        }
        HistoryItem::Step {
            label,
            status,
            elapsed,
        } => {
            let (glyph, color) = match status {
                StepStatus::Done => ("✓", Color::Green),
                StepStatus::Failed => ("✗", Color::Red),
                StepStatus::Skipped => ("○", Color::DarkGray),
            };
            vec![Line::from(vec![
                Span::raw("  "),
                Span::styled(glyph, Style::default().fg(color)),
                Span::raw("  "),
                Span::raw(label.clone()),
                Span::raw("  "),
                Span::styled(fmt_elapsed(*elapsed), Style::default().fg(Color::DarkGray)),
            ])]
        }
        HistoryItem::CapturedStderr(text) => {
            vec![Line::from(Span::raw(text.clone()))]
        }
        HistoryItem::DeviceLog { stream, line } => {
            let tag = match stream.as_str() {
                "stderr" => "[device:err]",
                _ => "[device]",
            };
            vec![Line::from(vec![
                Span::styled(tag, Style::default().fg(Color::Magenta)),
                Span::raw(" "),
                Span::raw(line.clone()),
            ])]
        }
        HistoryItem::Failure(reason) => vec![Line::from(vec![
            Span::styled(
                "✗ ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(reason.clone(), Style::default().fg(Color::Red)),
        ])],
    }
}

/// Paint `lines` into `buf` starting at the buffer's top-left.
/// Used by `insert_before`'s draw_fn, which gives us a buffer that
/// is exactly the height we asked for and the terminal's full
/// width.
fn write_lines_to_buffer(buf: &mut Buffer, lines: &[Line<'static>]) {
    for (i, line) in lines.iter().enumerate() {
        let area = Rect {
            x: buf.area.x,
            y: buf.area.y + i as u16,
            width: buf.area.width,
            height: 1,
        };
        if area.y >= buf.area.bottom() {
            break;
        }
        Paragraph::new(line.clone()).render(area, buf);
    }
}

fn phase_label(phase: &AppPhase) -> String {
    match phase {
        AppPhase::Setup => "setup".into(),
        AppPhase::Initializing => "initializing".into(),
        AppPhase::Building { .. } => "building".into(),
        AppPhase::Idle => "idle".into(),
        AppPhase::Patching { .. } => "patching".into(),
        AppPhase::Failed { phase, .. } => format!("{phase} failed"),
    }
}

fn phase_elapsed(phase: &AppPhase) -> Option<String> {
    match phase {
        AppPhase::Building { started_at, .. } | AppPhase::Patching { started_at } => {
            Some(fmt_elapsed(started_at.elapsed()))
        }
        _ => None,
    }
}

fn phase_color(phase: &AppPhase) -> Color {
    match phase {
        AppPhase::Setup | AppPhase::Initializing => Color::DarkGray,
        AppPhase::Building { .. } | AppPhase::Patching { .. } => Color::Yellow,
        AppPhase::Idle => Color::Green,
        AppPhase::Failed { .. } => Color::Red,
    }
}

fn fmt_elapsed(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        let secs = ms / 1_000;
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_dev_server::Event;

    fn s() -> LiveState {
        LiveState::new("iOS Simulator", "rs.whisker.podcast")
    }

    fn drain(state: &mut LiveState, e: &Event) -> Vec<HistoryItem> {
        let mut h = Vec::new();
        apply_event(state, e, &mut h);
        h
    }

    #[test]
    fn build_lifecycle_records_outcome() {
        let mut st = s();
        let started = drain(&mut st, &Event::BuildingFull);
        assert!(matches!(st.phase, AppPhase::Building { .. }));
        // BuildingFull no longer emits a PhaseEnter — that's
        // delegated to `whisker_build::ui::section` which lands in
        // scrollback via stderr capture.
        assert!(started.is_empty());
        let done = drain(&mut st, &Event::BuildSucceeded);
        assert!(matches!(st.phase, AppPhase::Idle));
        assert!(st.last_build.is_some());
        assert!(matches!(done[0], HistoryItem::PhaseDone { .. }));
    }

    #[test]
    fn client_counter_saturates() {
        let mut st = s();
        drain(&mut st, &Event::ClientConnected);
        drain(&mut st, &Event::ClientConnected);
        assert_eq!(st.client_count, 2);
        drain(&mut st, &Event::ClientDisconnected);
        drain(&mut st, &Event::ClientDisconnected);
        drain(&mut st, &Event::ClientDisconnected);
        assert_eq!(st.client_count, 0);
    }

    #[test]
    fn device_log_becomes_history_item() {
        let mut st = s();
        let h = drain(
            &mut st,
            &Event::DeviceLog {
                stream: "stdout".into(),
                line: "hello".into(),
                ts_micros: 0,
            },
        );
        assert_eq!(h.len(), 1);
        match &h[0] {
            HistoryItem::DeviceLog { stream, line } => {
                assert_eq!(stream, "stdout");
                assert_eq!(line, "hello");
            }
            other => panic!("expected DeviceLog, got {other:?}"),
        }
    }

    #[test]
    fn patch_sent_records_elapsed_and_resets_phase() {
        let mut st = s();
        st.phase = AppPhase::Patching {
            started_at: Instant::now() - Duration::from_millis(615),
        };
        let h = drain(&mut st, &Event::PatchSent);
        assert!(matches!(st.phase, AppPhase::Idle));
        assert!(st.last_patch.is_some());
        assert!(h.iter().any(|i| matches!(i, HistoryItem::PhaseDone { .. })));
    }

    #[test]
    fn build_failed_emits_failure_history() {
        let mut st = s();
        drain(&mut st, &Event::BuildingFull);
        let h = drain(&mut st, &Event::BuildFailed("link error".into()));
        assert!(matches!(st.phase, AppPhase::Failed { .. }));
        assert!(h.iter().any(|i| matches!(i, HistoryItem::Failure(_))));
    }

    #[test]
    fn strip_ansi_removes_csi_sgr() {
        let s = "\x1b[33mwarning\x1b[0m: \x1b[1munused\x1b[0m";
        assert_eq!(strip_ansi(s), "warning: unused");
    }

    #[test]
    fn strip_ansi_preserves_utf8_glyphs() {
        let s = "\x1b[32m✓\x1b[0m Sync gen/ios";
        assert_eq!(strip_ansi(s), "✓ Sync gen/ios");
    }

    #[test]
    fn strip_ansi_drops_osc_titles() {
        let s = "\x1b]0;title\x07hello";
        assert_eq!(strip_ansi(s), "hello");
    }

    #[test]
    fn build_live_lines_has_fixed_height() {
        let st = s();
        let lines = build_live_lines(&st, 0);
        assert_eq!(lines.len(), LIVE_HEIGHT as usize);
    }

    #[test]
    fn build_live_lines_shows_current_step() {
        let mut st = s();
        st.current_step = Some("xcodebuild WhiskerDriver-Debug".into());
        let lines = build_live_lines(&st, 0);
        let rendered = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.content.to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(rendered.contains("xcodebuild"));
    }

    #[test]
    fn build_live_lines_shows_dev_server_when_set() {
        let mut st = s();
        st.ws_addr = Some("127.0.0.1:9090".into());
        st.client_count = 1;
        let lines = build_live_lines(&st, 0);
        let rendered = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.content.to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(rendered.contains("127.0.0.1:9090"));
        assert!(rendered.contains("1 connected"));
    }
}
