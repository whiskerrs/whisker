//! Full-screen ratatui TUI for `whisker run`.
//!
//! Replaces the inline status bar in #184/#185 with an alternate-screen
//! UI that owns the entire terminal. Reasons:
//!
//! - Inline mode raced against every `eprintln!` from `whisker_build::ui`
//!   and Step::pipe-driven subprocess output, corrupting the viewport on
//!   any heavy log burst.
//! - Sub-step granularity (Sync → Build → Install → Launch) can't fit
//!   in 3 inline rows.
//! - Full-screen lets us own stderr — we `dup2` it through a pipe, read
//!   captured lines on a background thread, and render them in a Logs
//!   pane below the steps. Every output channel funnels through ratatui;
//!   there's nothing to race with.
//!
//! ## State machine
//!
//! [`AppPhase`] tracks where the dev loop is. Transitions are driven by
//! a combination of explicit cli calls (`Setup`, `Initializing` —
//! before dev-server events fire) and `whisker_dev_server::Event`s
//! (`Building`, `Idle`, `Patching`, …) routed through [`apply_event`].
//!
//! ## Lifetime
//!
//! [`Tui::start`] enters the alternate screen, hides the cursor, and
//! `dup2`s `STDERR_FILENO` to a pipe whose read end a dedicated thread
//! drains into the log buffer. [`Tui::shutdown`] reverses each step in
//! the opposite order. Both a panic hook and a `ctrlc::set_handler`
//! emergency-reset stderr + cursor visibility so a hard exit doesn't
//! leave the user's shell hosed.

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{poll, read, Event as CtEvent, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use std::collections::VecDeque;
use std::io::Write;
use std::os::raw::c_int;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ============================================================================
// Public state model
// ============================================================================

/// Which phase of the dev loop the user is currently watching. Drives
/// the steps pane's content and the header's right-hand badge.
#[derive(Debug, Clone)]
pub enum AppPhase {
    /// Pre-dev-server cli work: `sync_for_target` (gen tree + plugin
    /// build). Driven by explicit `TuiHandle::set_phase` calls.
    Setup,
    /// dev-server's setup (WS bind, watcher, capture shim resolve).
    /// Brief; usually flips to `Building` within a hundred ms once
    /// `Event::BuildingFull` arrives.
    Initializing,
    /// `cargo` + `gradle`/`xcodebuild` in flight.
    Building {
        started_at: Instant,
        kind: BuildKind,
    },
    /// dev-server bound, initial build succeeded, watching for source
    /// changes. The header shows ws addr + client count + last
    /// build/patch summaries.
    Idle,
    /// Tier 1 hot-patch in flight. Phase exit is signalled by
    /// `Event::PatchSent` or a Tier 2 fallback's `Event::BuildingFull`.
    Patching { started_at: Instant },
    /// Build failed. Surfaces the cause in the steps pane.
    Failed { phase: String, reason: String },
}

#[derive(Debug, Clone, Copy)]
pub enum BuildKind {
    Initial,
    Rebuild,
}

/// Outcome of a completed step (a single row in the steps pane).
#[derive(Debug, Clone, Copy)]
pub enum StepStatus {
    Pending,
    InProgress,
    Done,
    Failed,
    Skipped,
}

/// One row in the steps pane.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub status: StepStatus,
    pub elapsed_ms: Option<u64>,
}

/// Bounded ring of captured stderr lines for the Logs pane.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    pub lines: VecDeque<String>,
    pub capacity: usize,
}

impl LogBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity),
            capacity,
        }
    }
    pub fn push(&mut self, line: String) {
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}

/// Snapshot the render thread reads on every frame.
#[derive(Debug, Clone)]
pub struct TuiState {
    pub target: String,
    pub bundle: String,
    pub ws_addr: Option<String>,
    pub watching: Vec<String>,
    pub client_count: usize,
    pub phase: AppPhase,
    pub steps: Vec<Step>,
    pub last_build: Option<String>,
    pub last_patch: Option<String>,
    pub logs: LogBuffer,
    pub should_quit: bool,
}

impl TuiState {
    pub fn new(target: impl Into<String>, bundle: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            bundle: bundle.into(),
            ws_addr: None,
            watching: Vec::new(),
            client_count: 0,
            phase: AppPhase::Setup,
            steps: Vec::new(),
            last_build: None,
            last_patch: None,
            logs: LogBuffer::new(500),
            should_quit: false,
        }
    }
}

// ============================================================================
// Step helpers
// ============================================================================

pub fn start_step(state: &mut TuiState, label: impl Into<String>) -> usize {
    state.steps.push(Step {
        label: label.into(),
        status: StepStatus::InProgress,
        elapsed_ms: None,
    });
    state.steps.len() - 1
}

pub fn finish_step_at(state: &mut TuiState, idx: usize, status: StepStatus, elapsed_ms: u64) {
    if let Some(step) = state.steps.get_mut(idx) {
        step.status = status;
        step.elapsed_ms = Some(elapsed_ms);
    }
}

pub fn reset_steps(state: &mut TuiState) {
    state.steps.clear();
}

// ============================================================================
// Event → state machine
// ============================================================================

/// Apply a dev-server event to the in-memory TUI state.
pub fn apply_event(state: &mut TuiState, event: &whisker_dev_server::Event) {
    use whisker_dev_server::Event;
    match event {
        Event::Started => {
            // dev-server is up. Phase transition is driven by the
            // cli's explicit `set_phase` calls — respect that ordering.
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
            reset_steps(state);
        }
        Event::BuildSucceeded => {
            if let AppPhase::Building { started_at, kind } = &state.phase {
                let elapsed = started_at.elapsed();
                let label = match kind {
                    BuildKind::Initial => "initial",
                    BuildKind::Rebuild => "rebuild",
                };
                state.last_build = Some(format!("{label} · {}", fmt_elapsed(elapsed)));
            }
            state.phase = AppPhase::Idle;
        }
        Event::BuildFailed(msg) => {
            state.phase = AppPhase::Failed {
                phase: "build".into(),
                reason: msg.clone(),
            };
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
                state.last_patch = Some(fmt_elapsed(elapsed));
            }
            state.phase = AppPhase::Idle;
        }
        Event::DeviceLog { stream, line, .. } => {
            let tag = match stream.as_str() {
                "stderr" => "[device:err]",
                _ => "[device]",
            };
            state.logs.push(format!("{tag} {line}"));
        }
    }
}

// ============================================================================
// Tui: alternate-screen + stderr capture + render loop
// ============================================================================

/// Writer for ratatui's backend — writes directly to the saved
/// (pre-`dup2`) stderr fd so its escape codes don't get folded into
/// the captured log.
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

/// Owns the ratatui terminal + state mutex + the saved stderr fd.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<OriginalStderr>>,
    state: Arc<Mutex<TuiState>>,
    saved_stderr_fd: c_int,
}

impl Tui {
    pub fn start(state: TuiState) -> Result<Self> {
        let (saved_stderr_fd, capture_read_fd) =
            install_stderr_capture().context("install stderr capture")?;
        install_terminal_cleanup_once(saved_stderr_fd);

        enable_raw_mode().context("enable raw mode")?;
        let mut original = OriginalStderr(saved_stderr_fd);
        original
            .execute(EnterAlternateScreen)
            .context("enter alternate screen")?;
        original.execute(cursor::Hide).context("hide cursor")?;

        let backend = CrosstermBackend::new(original);
        let terminal = Terminal::new(backend).context("create ratatui terminal")?;

        let state = Arc::new(Mutex::new(state));
        let state_for_reader = Arc::clone(&state);
        std::thread::Builder::new()
            .name("whisker-tui-capture".into())
            .spawn(move || capture_reader_loop(capture_read_fd, state_for_reader))
            .context("spawn capture reader")?;

        Ok(Self {
            terminal,
            state,
            saved_stderr_fd,
        })
    }

    pub fn handle(&self) -> TuiHandle {
        TuiHandle {
            state: Arc::clone(&self.state),
        }
    }

    /// Drive the render loop until either the user presses `q` or
    /// `should_quit` is flipped externally.
    pub fn render_until_quit(&mut self) -> Result<()> {
        let frame_interval = Duration::from_millis(100);
        let mut last_draw = Instant::now() - frame_interval;
        loop {
            if poll(Duration::from_millis(10))? {
                if let CtEvent::Key(key) = read()? {
                    if matches!(key.kind, KeyEventKind::Press) {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                if let Ok(mut s) = self.state.lock() {
                                    s.should_quit = true;
                                }
                            }
                            KeyCode::Char('c')
                                if key
                                    .modifiers
                                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
                            {
                                if let Ok(mut s) = self.state.lock() {
                                    s.should_quit = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            if last_draw.elapsed() >= frame_interval {
                self.draw()?;
                last_draw = Instant::now();
            }
            if let Ok(s) = self.state.lock() {
                if s.should_quit {
                    break;
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self) -> Result<()> {
        let snapshot = match self.state.lock() {
            Ok(g) => g.clone(),
            Err(_) => return Ok(()),
        };
        self.terminal.draw(|frame| render(frame, &snapshot))?;
        Ok(())
    }

    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.terminal.clear();
        let mut original = OriginalStderr(self.saved_stderr_fd);
        let _ = original.execute(cursor::Show);
        let _ = original.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
        unsafe {
            libc::dup2(self.saved_stderr_fd, libc::STDERR_FILENO);
            libc::close(self.saved_stderr_fd);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct TuiHandle {
    state: Arc<Mutex<TuiState>>,
}

impl TuiHandle {
    pub fn with<F: FnOnce(&mut TuiState)>(&self, f: F) {
        if let Ok(mut g) = self.state.lock() {
            f(&mut g);
        }
    }
    pub fn set_phase(&self, phase: AppPhase) {
        self.with(|s| {
            s.phase = phase;
            reset_steps(s);
        });
    }
    pub fn start_step(&self, label: impl Into<String>) -> usize {
        let label = label.into();
        let mut idx = 0;
        self.with(|s| {
            idx = start_step(s, label);
        });
        idx
    }
    pub fn finish_step(&self, idx: usize, status: StepStatus, elapsed_ms: u64) {
        self.with(|s| finish_step_at(s, idx, status, elapsed_ms));
    }
    pub fn apply_event(&self, event: &whisker_dev_server::Event) {
        self.with(|s| apply_event(s, event));
    }
    pub fn set_dev_server(&self, ws_addr: impl Into<String>, watching: Vec<String>) {
        self.with(|s| {
            s.ws_addr = Some(ws_addr.into());
            s.watching = watching;
        });
    }
    pub fn should_quit(&self) -> bool {
        self.state.lock().map(|s| s.should_quit).unwrap_or(false)
    }
    pub fn request_quit(&self) {
        self.with(|s| s.should_quit = true);
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

fn capture_reader_loop(read_fd: c_int, state: Arc<Mutex<TuiState>>) {
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
            if !text.is_empty() {
                if let Ok(mut g) = state.lock() {
                    g.logs.push(text);
                }
            }
        }
    }
}

// ============================================================================
// Cleanup hooks
// ============================================================================

fn emergency_terminal_reset(original_stderr_fd: c_int) {
    let mut o = OriginalStderr(original_stderr_fd);
    let _ = o.execute(cursor::Show);
    let _ = o.execute(LeaveAlternateScreen);
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

fn render(frame: &mut ratatui::Frame, state: &TuiState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),      // header
            Constraint::Min(8),         // mid pane (steps OR status)
            Constraint::Percentage(40), // logs
            Constraint::Length(1),      // footer
        ])
        .split(area);

    render_header(frame, chunks[0], state);
    render_middle(frame, chunks[1], state);
    render_logs(frame, chunks[2], state);
    render_footer(frame, chunks[3], state);
}

fn render_header(frame: &mut ratatui::Frame, area: Rect, state: &TuiState) {
    let line = Line::from(vec![
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
    ]);
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn render_middle(frame: &mut ratatui::Frame, area: Rect, state: &TuiState) {
    let mut lines: Vec<Line> = Vec::new();
    match &state.phase {
        AppPhase::Idle => {
            lines.push(Line::from(Span::styled(
                "Status",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            if let Some(addr) = &state.ws_addr {
                lines.push(kv_line("Dev server", addr));
            }
            lines.push(kv_line(
                "Clients",
                &format!("{} connected", state.client_count),
            ));
            if !state.watching.is_empty() {
                lines.push(kv_line(
                    "Watching",
                    &format!("{} path(s)", state.watching.len()),
                ));
            }
            if let Some(b) = &state.last_build {
                lines.push(kv_line("Last build", b));
            }
            if let Some(p) = &state.last_patch {
                lines.push(kv_line("Last patch", p));
            }
        }
        AppPhase::Failed { phase, reason } => {
            lines.push(Line::from(Span::styled(
                format!("{phase} failed"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(reason.clone()));
        }
        _ => {
            lines.push(Line::from(Span::styled(
                phase_section_title(&state.phase),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            for step in &state.steps {
                lines.push(render_step_row(step));
            }
        }
    }
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_logs(frame: &mut ratatui::Frame, area: Rect, state: &TuiState) {
    let visible_rows = area.height.saturating_sub(2) as usize;
    let total = state.logs.lines.len();
    let start = total.saturating_sub(visible_rows);
    let lines: Vec<Line> = state
        .logs
        .lines
        .iter()
        .skip(start)
        .map(|l| Line::from(l.clone()))
        .collect();
    let title = format!(" Logs ({}/{}) ", lines.len(), total);
    let block = Block::default()
        .borders(Borders::TOP)
        .title(title)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_footer(frame: &mut ratatui::Frame, area: Rect, _state: &TuiState) {
    let hint = Line::from(vec![
        Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
        Span::raw(" quit"),
    ]);
    frame.render_widget(Paragraph::new(hint), area);
}

fn render_step_row(step: &Step) -> Line<'static> {
    let glyph = match step.status {
        StepStatus::Pending => Span::styled("·", Style::default().fg(Color::DarkGray)),
        StepStatus::InProgress => Span::styled("⠋", Style::default().fg(Color::Yellow)),
        StepStatus::Done => Span::styled("✓", Style::default().fg(Color::Green)),
        StepStatus::Failed => Span::styled("✗", Style::default().fg(Color::Red)),
        StepStatus::Skipped => Span::styled("○", Style::default().fg(Color::DarkGray)),
    };
    let label_style = match step.status {
        StepStatus::Pending | StepStatus::Skipped => Style::default().fg(Color::DarkGray),
        StepStatus::InProgress | StepStatus::Done => Style::default(),
        StepStatus::Failed => Style::default().fg(Color::Red),
    };
    let mut spans = vec![
        Span::raw("  "),
        glyph,
        Span::raw("  "),
        Span::styled(step.label.clone(), label_style),
    ];
    if let Some(elapsed) = step.elapsed_ms {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            fmt_elapsed(Duration::from_millis(elapsed)),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

fn kv_line(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:<12}"), Style::default().fg(Color::DarkGray)),
        Span::raw(value.to_string()),
    ])
}

fn phase_label(phase: &AppPhase) -> String {
    match phase {
        AppPhase::Setup => "setup".into(),
        AppPhase::Initializing => "initializing".into(),
        AppPhase::Building { started_at, .. } => {
            format!("building · {}", fmt_elapsed(started_at.elapsed()))
        }
        AppPhase::Idle => "idle".into(),
        AppPhase::Patching { started_at } => {
            format!("patching · {}", fmt_elapsed(started_at.elapsed()))
        }
        AppPhase::Failed { phase, .. } => format!("{phase} failed"),
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

fn phase_section_title(phase: &AppPhase) -> &'static str {
    match phase {
        AppPhase::Setup => "Setup",
        AppPhase::Initializing => "Initializing dev server",
        AppPhase::Building { kind, .. } => match kind {
            BuildKind::Initial => "Initial build",
            BuildKind::Rebuild => "Rebuild",
        },
        AppPhase::Idle => "Status",
        AppPhase::Patching { .. } => "Patch (tier 1)",
        AppPhase::Failed { .. } => "Failed",
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

    fn s() -> TuiState {
        TuiState::new("iOS Simulator", "rs.whisker.podcast")
    }

    #[test]
    fn build_lifecycle_records_outcome() {
        let mut st = s();
        apply_event(&mut st, &Event::BuildingFull);
        assert!(matches!(st.phase, AppPhase::Building { .. }));
        apply_event(&mut st, &Event::BuildSucceeded);
        assert!(matches!(st.phase, AppPhase::Idle));
        assert!(st.last_build.is_some());
    }

    #[test]
    fn client_counter_saturates() {
        let mut st = s();
        apply_event(&mut st, &Event::ClientConnected);
        apply_event(&mut st, &Event::ClientConnected);
        assert_eq!(st.client_count, 2);
        apply_event(&mut st, &Event::ClientDisconnected);
        apply_event(&mut st, &Event::ClientDisconnected);
        apply_event(&mut st, &Event::ClientDisconnected);
        assert_eq!(st.client_count, 0);
    }

    #[test]
    fn device_log_goes_into_log_buffer_with_tag() {
        let mut st = s();
        apply_event(
            &mut st,
            &Event::DeviceLog {
                stream: "stdout".into(),
                line: "hello".into(),
                ts_micros: 0,
            },
        );
        assert_eq!(st.logs.lines.len(), 1);
        assert!(st.logs.lines[0].contains("[device]"));
        assert!(st.logs.lines[0].contains("hello"));
    }

    #[test]
    fn log_buffer_drops_oldest_at_capacity() {
        let mut b = LogBuffer::new(3);
        for i in 0..5 {
            b.push(format!("line {i}"));
        }
        assert_eq!(b.lines.len(), 3);
        assert_eq!(b.lines.front().unwrap(), "line 2");
        assert_eq!(b.lines.back().unwrap(), "line 4");
    }

    #[test]
    fn start_finish_step_round_trips() {
        let mut st = s();
        let i = start_step(&mut st, "Sync gen/ios");
        assert_eq!(st.steps.len(), 1);
        finish_step_at(&mut st, i, StepStatus::Done, 124);
        assert!(matches!(st.steps[0].status, StepStatus::Done));
        assert_eq!(st.steps[0].elapsed_ms, Some(124));
    }

    #[test]
    fn patch_sent_records_elapsed_and_resets_phase() {
        let mut st = s();
        st.phase = AppPhase::Patching {
            started_at: Instant::now() - Duration::from_millis(615),
        };
        apply_event(&mut st, &Event::PatchSent);
        assert!(matches!(st.phase, AppPhase::Idle));
        assert!(st.last_patch.is_some());
    }
}
