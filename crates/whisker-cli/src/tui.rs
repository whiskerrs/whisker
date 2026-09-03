//! Inline-viewport TUI for the top-level `whisker run` and
//! `whisker build` workflows.
//!
//! ## Design
//!
//! A small "live region" is anchored at the bottom and everything
//! else is pushed into the terminal's *normal* scrollback, so a
//! cargo / gradle / xcodebuild log burst stays scrollable instead of
//! being trapped in a pane a full-screen TUI would own. ratatui
//! 0.29's [`Viewport::Inline`] + [`Terminal::insert_before`] give
//! that shape directly (with the `scrolling-regions` feature enabled
//! so we land on the DECSTBM fast path).
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
//!    r  hot reload   R  full reload   o  relaunch   q  quit
//! ──────────────────────────────────────────────────────────────────
//! ```
//!
//! ## Subprocess output → scrollback
//!
//! `whisker_build::ui::*` calls arrive as typed [`ProgressEvent`]s.
//! Unstructured diagnostics from cargo / gradle / xcodebuild and
//! other libraries may still write to stderr, so we `dup2`
//! `STDERR_FILENO` to a pipe whose read end a dedicated thread drains
//! line-by-line, strips ANSI escapes from, and sends through an mpsc
//! channel. The render thread drains that channel each frame and calls
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
//! [`TuiHandle::apply_event`]. Build operations and section history
//! come from [`ProgressEvent`], so rendered terminal strings are never
//! parsed to infer workflow state.

use anyhow::{Context, Result};
use crossterm::{
    ExecutableCommand, cursor,
    event::{Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers, poll, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::Write;
use std::io::{IsTerminal, stderr, stdin};
use std::os::raw::c_int;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use whisker_build::ui::{
    MessageLevel, OperationOutcome, ProgressEvent, ReporterGuard, install_reporter,
};

/// Height of the live region in rows. The header, current step,
/// dev-server info and key hint together comfortably fit in 6 rows;
/// taller cuts scrollback density and saves nothing.
const LIVE_HEIGHT: u16 = 6;

mod render;

use render::*;

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
    /// hot-reload patch in flight. Phase exit is signalled by
    /// `Event::PatchSent` or a full reload fallback's `Event::BuildingFull`.
    Patching { started_at: Instant },
    /// A finite build workflow produced its artifact successfully.
    Completed,
    /// A user requested a graceful dev-server shutdown.
    Stopping,
    /// Build failed. The live region surfaces the cause and the cli
    /// is about to exit non-zero.
    Failed { phase: String, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowKind {
    Run,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStatus {
    NotStarted,
    Launching,
    WaitingForConnection,
    Connected,
    Failed(String),
}

#[derive(Debug, Clone, Copy)]
pub enum BuildKind {
    Initial,
    Rebuild,
}

/// Outcome of a completed step in terminal scrollback.
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
    pub workflow: WorkflowKind,
    pub target: String,
    pub bundle: String,
    pub phase: AppPhase,
    /// Label of the in-progress step (e.g. "xcodebuild …"). Cleared
    /// when the step finishes.
    pub current_step: Option<String>,
    pub target_destination: Option<String>,
    pub host_status: HostStatus,
    pub watching: Vec<String>,
    pub client_count: usize,
    pub last_build: Option<String>,
    pub last_patch: Option<String>,
    pub artifact: Option<String>,
    pub should_quit: bool,
    /// `true` when the render loop must terminate the process because
    /// no workflow command channel exists to perform an orderly stop.
    /// Normal `run` shutdown leaves this `false`: the dev server owns
    /// cleanup and the cli asks the TUI to finish after `run()` returns.
    pub force_exit: bool,
    /// Persistent "press R to Full Reload" banner. Set by
    /// `Event::FullReloadRequired` (the dev loop hit a change it
    /// can't hot-reload), cleared when a Full Reload actually starts
    /// (`Event::BuildingFull`).
    pub full_reload_needed: Option<String>,
    /// Channel into the dev loop for the `r` / `R` shortcuts. `None`
    /// until the cli wires the dev-server up (early keypresses
    /// during setup are dropped).
    pub command_tx: Option<tokio::sync::mpsc::UnboundedSender<whisker_dev_server::DevCommand>>,
}

impl LiveState {
    pub fn new(
        workflow: WorkflowKind,
        target: impl Into<String>,
        bundle: impl Into<String>,
    ) -> Self {
        Self {
            workflow,
            target: target.into(),
            bundle: bundle.into(),
            phase: AppPhase::Setup,
            current_step: None,
            target_destination: None,
            host_status: HostStatus::NotStarted,
            watching: Vec::new(),
            client_count: 0,
            last_build: None,
            last_patch: None,
            artifact: None,
            should_quit: false,
            force_exit: false,
            full_reload_needed: None,
            command_tx: None,
        }
    }
}

/// One message the render thread receives from upstream producers
/// (cli code, dev-server events, and the stderr capture thread).
/// Variants paint a row into the terminal's scrollback via
/// [`Terminal::insert_before`]. Live-only workflow state is updated
/// directly through [`TuiHandle`].
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
    DeviceLog {
        stream: String,
        line: String,
    },
    Message {
        level: MessageLevel,
        text: String,
    },
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
            // A Full Reload is running — the pending prompt (if any)
            // is being acted on.
            state.full_reload_needed = None;
            let kind = match state.phase {
                AppPhase::Setup | AppPhase::Initializing => BuildKind::Initial,
                _ => BuildKind::Rebuild,
            };
            state.phase = AppPhase::Building {
                started_at: Instant::now(),
                kind,
            };
            // No history row: `whisker_build::ui::section` already emits
            // the structured section entry, and a second phase line would
            // duplicate it.
            state.current_step = None;
        }
        Event::BuildSucceeded => {
            if let AppPhase::Building { started_at, kind } = &state.phase {
                let elapsed = started_at.elapsed();
                // No `HistoryItem::PhaseDone` here: on iOS this event
                // fires after `builder.build()` (Swift staging, ~100ms)
                // while cargo + xcodebuild are still ahead inside
                // `installer.install_and_launch`, so an aggregate
                // "✓ Initial build XXms" row would misstate the time.
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
        Event::HostLaunching => {
            state.host_status = HostStatus::Launching;
        }
        Event::HostLaunched => {
            state.host_status = if state.client_count > 0 {
                HostStatus::Connected
            } else {
                HostStatus::WaitingForConnection
            };
            state.phase = AppPhase::Idle;
        }
        Event::HostLaunchFailed(reason) => {
            state.host_status = HostStatus::Failed(reason.clone());
            history.push(HistoryItem::Failure(reason.clone()));
        }
        Event::ClientConnected => {
            state.client_count = state.client_count.saturating_add(1);
            state.host_status = HostStatus::Connected;
        }
        Event::ClientDisconnected => {
            state.client_count = state.client_count.saturating_sub(1);
            if state.client_count == 0 {
                state.host_status = HostStatus::WaitingForConnection;
            }
        }
        Event::PatchBuilding => {
            // Exits are `Event::PatchSent` (→ Idle) or, when the loop
            // falls back to a cold rebuild, `Event::BuildingFull`
            // (→ Building).
            state.phase = AppPhase::Patching {
                started_at: Instant::now(),
            };
        }
        Event::PatchSent => {
            if let AppPhase::Patching { started_at } = &state.phase {
                let elapsed = started_at.elapsed();
                state.last_patch = Some(fmt_elapsed(elapsed));
            }
            state.phase = AppPhase::Idle;
            state.current_step = None;
        }
        Event::FullReloadRequired { reason } => {
            // Banner only — the dev-server's own `ui::warn` event is
            // already in scrollback.
            state.full_reload_needed = Some(reason.clone());
            // An earlier `PatchBuilding` may have set Patching; reset
            // or the spinner runs forever.
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
    /// a scrollback entry — `whisker_build::ui::section` already emits
    /// labeled phase boundaries through the progress reporter, so a
    /// duplicate `▶ label` line would just be noise. Only a failed
    /// build emits a `HistoryItem::PhaseDone` summary, from [`apply_event`].
    pub fn set_phase(&self, phase: AppPhase) {
        self.with(|s| {
            s.phase = phase;
            s.current_step = None;
        });
    }

    pub fn apply_event(&self, event: &whisker_dev_server::Event) {
        let mut history: Vec<HistoryItem> = Vec::new();
        self.with(|s| apply_event(s, event, &mut history));
        for h in history {
            self.send(h);
        }
    }

    /// Apply one structured build/run progress event. This is the only place
    /// the terminal model learns about build operations; rendered stderr is
    /// retained for diagnostics but is never parsed to infer workflow state.
    pub fn apply_progress_event(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::Section(label) => self.send(HistoryItem::PhaseEnter(label)),
            ProgressEvent::OperationStarted { kind, detail } => {
                let label = format!("{} · {detail}", kind.label());
                self.with(|state| {
                    if state.workflow == WorkflowKind::Build
                        && !matches!(state.phase, AppPhase::Building { .. })
                    {
                        state.phase = AppPhase::Building {
                            started_at: Instant::now(),
                            kind: BuildKind::Initial,
                        };
                    }
                    state.current_step = Some(label);
                });
            }
            ProgressEvent::OperationProgress { kind, message } => {
                self.with(|state| {
                    state.current_step = Some(format!("{} · {message}", kind.label()));
                });
            }
            ProgressEvent::OperationFinished {
                kind,
                detail,
                outcome,
                summary,
                elapsed,
            } => {
                self.with(|state| {
                    state.current_step = None;
                    if state.workflow == WorkflowKind::Build && outcome == OperationOutcome::Failed
                    {
                        state.phase = AppPhase::Failed {
                            phase: kind.label().to_string(),
                            reason: if summary.is_empty() {
                                detail.clone()
                            } else {
                                summary.clone()
                            },
                        };
                    }
                });
                self.send(HistoryItem::Step {
                    label: format!("{} · {detail}", kind.label()),
                    status: match outcome {
                        OperationOutcome::Done => StepStatus::Done,
                        OperationOutcome::Failed => StepStatus::Failed,
                    },
                    elapsed,
                });
            }
            ProgressEvent::Message { level, text } => {
                self.send(HistoryItem::Message { level, text });
            }
            // Run status is represented by target-aware dev-server events
            // in the live region. The generic plain-output status string
            // would duplicate that information in scrollback.
            ProgressEvent::Status(_) => {}
        }
    }

    pub fn set_artifact(&self, artifact: impl Into<String>) {
        self.with(|state| state.artifact = Some(artifact.into()));
    }

    pub fn set_dev_server(&self, destination: impl Into<String>, watching: Vec<String>) {
        let destination = destination.into();
        self.with(|s| {
            s.target_destination = Some(destination);
            s.watching = watching;
        });
    }

    pub fn should_quit(&self) -> bool {
        self.live.lock().map(|s| s.should_quit).unwrap_or(false)
    }

    pub fn request_quit(&self) {
        self.with(|s| s.should_quit = true);
    }

    /// Wire the `r` / `R` shortcuts to the dev loop. Called by the
    /// cli once the dev-server's command channel exists; presses
    /// before that are dropped silently.
    pub fn set_command_sender(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<whisker_dev_server::DevCommand>,
    ) {
        self.with(|s| s.command_tx = Some(tx));
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
    _reporter: ReporterGuard,
}

/// Owns the terminal render thread for one top-level CLI workflow.
/// Nested build helpers never construct this type.
pub struct TuiSession {
    handle: TuiHandle,
    render_thread: Option<std::thread::JoinHandle<()>>,
}

impl TuiSession {
    pub fn start(
        workflow: WorkflowKind,
        target: impl Into<String>,
        bundle: impl Into<String>,
    ) -> Result<Self> {
        let (tui, handle) = Tui::start(workflow, target.into(), bundle.into())?;
        let render_thread = std::thread::Builder::new()
            .name("whisker-tui-render".into())
            .spawn(move || run_render_loop(tui))
            .context("spawn TUI render thread")?;
        Ok(Self {
            handle,
            render_thread: Some(render_thread),
        })
    }

    pub fn handle(&self) -> TuiHandle {
        self.handle.clone()
    }

    pub fn finish(mut self) {
        self.handle.request_quit();
        if let Some(thread) = self.render_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        self.handle.request_quit();
        if let Some(thread) = self.render_thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn should_start(disabled: bool) -> bool {
    !disabled && !whisker_build::ui::is_verbose() && stderr().is_terminal() && stdin().is_terminal()
}

fn run_render_loop(mut tui: Tui) {
    let _ = tui.render_until_quit();
    let force_exit = tui.requires_force_exit();
    let _ = tui.shutdown();
    if force_exit {
        whisker_build::child_guard::kill_all();
        std::process::exit(130);
    }
}

impl Tui {
    /// Set up the inline TUI, install the stderr capture, and hand
    /// back a `(Tui, TuiHandle)` pair. The cli keeps `TuiHandle` and
    /// passes `Tui` to a dedicated OS thread that runs the render
    /// loop (ratatui's `Terminal` isn't `Send` once it has a backend
    /// holding raw fds — keep it pinned to one thread).
    pub fn start(
        workflow: WorkflowKind,
        target: String,
        bundle: String,
    ) -> Result<(Self, TuiHandle)> {
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

        let live = Arc::new(Mutex::new(LiveState::new(workflow, target, bundle)));
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
        let progress_handle = handle.clone();
        let reporter = install_reporter(move |event| progress_handle.apply_progress_event(event))
            .context("install structured progress reporter")?;

        Ok((
            Self {
                terminal,
                live,
                rx,
                saved_stderr_fd,
                spinner_idx: 0,
                _reporter: reporter,
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

            // Tight enough that `q` feels responsive, loose enough not
            // to peg a core.
            if poll(Duration::from_millis(50))? {
                if let CtEvent::Key(key) = read()? {
                    if matches!(key.kind, KeyEventKind::Press) {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => self.user_quit(),
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.user_quit()
                            }
                            // A Full Reload only ever happens on `R`;
                            // the dev loop never triggers one itself.
                            KeyCode::Char('r') => {
                                self.send_command(whisker_dev_server::DevCommand::HotReload)
                            }
                            KeyCode::Char('R') => {
                                self.send_command(whisker_dev_server::DevCommand::FullReload)
                            }
                            KeyCode::Char('o') => {
                                self.send_command(whisker_dev_server::DevCommand::Relaunch)
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

    /// Forward a reload shortcut to the dev loop. Dropped silently
    /// when the dev-server hasn't wired its command channel yet
    /// (setup phase) or has shut down.
    fn send_command(&self, cmd: whisker_dev_server::DevCommand) {
        if let Ok(s) = self.live.lock() {
            if let Some(tx) = &s.command_tx {
                let _ = tx.send(cmd);
            }
        }
    }

    /// User-initiated quit (q / Esc / Ctrl-C from the TUI).
    /// Run workflows ask the dev server to shut down; finite build
    /// workflows fall back to process cancellation after terminal cleanup.
    fn user_quit(&self) {
        if let Ok(mut s) = self.live.lock() {
            request_user_quit(&mut s);
        }
    }

    /// Whether orderly workflow shutdown was unavailable and the
    /// render thread must terminate the process after restoring the
    /// terminal. This is primarily the finite build workflow, where
    /// the main thread can be blocked in an external build command.
    pub fn requires_force_exit(&self) -> bool {
        self.live.lock().map(|s| s.force_exit).unwrap_or(false)
    }

    pub fn shutdown(mut self) -> Result<()> {
        // One last drain so any final phase/Step/Failure entry the
        // cli emitted between the last render and quit lands in
        // scrollback before the live region disappears.
        let _ = self.drain_history_into_scrollback();
        // `Terminal::show_cursor`, not a bare crossterm `cursor::Show`
        // against the saved fd: going through the terminal clears its
        // `hidden_cursor` flag, which otherwise makes ratatui's `Drop`
        // retry the call on an already-closed fd and print `Failed to
        // show the cursor: Bad file descriptor` to the shell.
        let _ = self.terminal.clear();
        let _ = self.terminal.show_cursor();
        // Hand the terminal back with the cursor at column 0 of a
        // fresh row: when a libc-delivered Ctrl-C beats the `KeyEvent`
        // handler, the shell's PS1 otherwise inherits the in-flight
        // render's cursor position. Written to the saved stderr fd,
        // which still reaches the real terminal while STDERR_FILENO
        // points at the capture pipe.
        let mut original = OriginalStderr(self.saved_stderr_fd);
        let _ = original.write_all(b"\r\n");
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

fn request_user_quit(state: &mut LiveState) {
    if state.workflow == WorkflowKind::Run {
        if let Some(tx) = &state.command_tx {
            if tx.send(whisker_dev_server::DevCommand::Shutdown).is_ok() {
                state.phase = AppPhase::Stopping;
                return;
            }
        }
    }
    state.force_exit = true;
    state.should_quit = true;
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
    // Mirror `Tui::shutdown`'s cursor reset (see the comment
    // there): without `\r\n` after `cursor::Show`, the shell
    // prompt that takes over on `process::exit(130)` starts at
    // whatever column the last live-region draw left the cursor.
    let _ = o.write_all(b"\r\n");
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
        // SIGTERM any in-flight build before the hard-exit skips Drop,
        // so Ctrl-C during a build doesn't orphan cargo / gradle /
        // xcodebuild.
        whisker_build::child_guard::kill_all();
        std::process::exit(130);
    });
}

// ============================================================================
// Rendering
// ============================================================================

#[cfg(test)]
mod tests;
