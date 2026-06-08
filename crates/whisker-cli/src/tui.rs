//! ratatui-based inline status bar for `whisker run`.
//!
//! `whisker run` is a long-lived dev loop — file watch + rebuild +
//! patch + log forwarding — but until now the only persistent state
//! the user could see was the last printed line of curated terminal
//! output. To check the bind address, current client count, or last
//! patch latency mid-edit, you had to scroll up through `gradle:
//! Task :…` rows and find the most recent `· dev-server · …` entry.
//!
//! This module replaces that with a 3-line region anchored at the
//! bottom of the terminal (ratatui's `Viewport::Inline`). Sections,
//! steps, info lines, and device logs still flow as plain
//! `eprintln!`s above the viewport — fully grep'able, fully
//! scrollable — while the inline area renders a live status bar
//! summarising:
//!
//! - the build target + bind address
//! - the connected client count
//! - the dev-loop's current phase (idle / building / patching) +
//!   elapsed time
//! - the last completed action's outcome
//!
//! ## Coordination with `whisker_build::ui`
//!
//! indicatif's `MultiProgress` (which the rest of the dev loop draws
//! progress through) and ratatui's inline viewport both want
//! exclusive control of the terminal cursor. We resolve the conflict
//! at the source: when this TUI is active, [`crate::run::run`] sets
//! `WHISKER_TUI=1`, which switches `whisker_build::ui` into
//! [`whisker_build::ui::is_tui`] mode — indicatif animations off,
//! curated formatting still on, output plain-`eprintln!`-routed so
//! lines scroll above the viewport.
//!
//! ## Lifetime + shutdown
//!
//! [`Tui::start`] takes ownership of stderr and enters raw mode for
//! the duration of the run. [`Tui::shutdown`] restores cooked mode,
//! clears the viewport, and re-prints a final status summary so the
//! terminal looks sane after the dev loop exits. The struct also
//! installs a panic hook that calls `shutdown` so an unexpected
//! panic doesn't leave the user with a half-rendered TUI.

use anyhow::Result;
use crossterm::{
    cursor,
    terminal::{Clear, ClearType},
    ExecutableCommand,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::Stderr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the loop is currently doing. Surfaces in the status bar.
#[derive(Debug, Clone)]
pub enum Phase {
    /// Initialising; the dev-server hasn't reported any event yet.
    Starting,
    /// A full cargo / cross / gradle / xcodebuild rebuild is running.
    Building { started_at: Instant },
    /// A Tier 1 hot-patch is being computed / sent.
    Patching { started_at: Instant },
    /// Nothing in progress — watching for file changes.
    Idle,
}

impl Phase {
    fn label(&self) -> &'static str {
        match self {
            Phase::Starting => "starting",
            Phase::Building { .. } => "building",
            Phase::Patching { .. } => "patching",
            Phase::Idle => "idle",
        }
    }

    fn elapsed(&self) -> Option<Duration> {
        match self {
            Phase::Building { started_at } | Phase::Patching { started_at } => {
                Some(started_at.elapsed())
            }
            Phase::Starting | Phase::Idle => None,
        }
    }
}

/// Outcome of the most recent build / patch / install / launch.
/// Rendered as the right-aligned trailing summary on the bottom row.
#[derive(Debug, Clone)]
pub enum LastOutcome {
    None,
    Success(String),
    Failure(String),
}

/// Mutable snapshot the renderer reads on every frame.
#[derive(Debug, Clone)]
pub struct TuiState {
    pub target: String,
    pub bind: String,
    pub client_count: usize,
    pub phase: Phase,
    pub last: LastOutcome,
}

impl TuiState {
    pub fn new(target: impl Into<String>, bind: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            bind: bind.into(),
            client_count: 0,
            phase: Phase::Starting,
            last: LastOutcome::None,
        }
    }
}

/// Apply a dev-server event to the in-memory TUI state. Pulled out of
/// the cli so it's unit-testable without a real terminal.
pub fn apply_event(state: &mut TuiState, event: &whisker_dev_server::Event) {
    use whisker_dev_server::Event;
    match event {
        Event::Started => {
            state.phase = Phase::Idle;
        }
        Event::BuildingFull => {
            state.phase = Phase::Building {
                started_at: Instant::now(),
            };
        }
        Event::BuildSucceeded => {
            let detail = match &state.phase {
                Phase::Building { started_at } => {
                    format!("build ok {}", fmt_elapsed(started_at.elapsed()))
                }
                _ => "build ok".to_string(),
            };
            state.phase = Phase::Idle;
            state.last = LastOutcome::Success(detail);
        }
        Event::BuildFailed(msg) => {
            state.phase = Phase::Idle;
            state.last = LastOutcome::Failure(format!("build failed: {msg}"));
        }
        Event::ClientConnected => {
            state.client_count = state.client_count.saturating_add(1);
        }
        Event::ClientDisconnected => {
            state.client_count = state.client_count.saturating_sub(1);
        }
        Event::PatchSent => {
            let detail = match &state.phase {
                Phase::Patching { started_at } => {
                    format!("patch ok {}", fmt_elapsed(started_at.elapsed()))
                }
                _ => "patch ok".to_string(),
            };
            state.phase = Phase::Idle;
            state.last = LastOutcome::Success(detail);
        }
        // Device-log events don't change the persistent status — they
        // flow into scrollback through whisker-cli's separate handler.
        Event::DeviceLog { .. } => {}
    }
}

/// Owns the ratatui terminal + the shared state Mutex. The renderer
/// thread reads the state on every frame and re-draws.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stderr>>,
    state: Arc<Mutex<TuiState>>,
}

impl Tui {
    /// Initialise the TUI: hide the cursor, allocate an inline
    /// viewport at the bottom of the terminal. Returns the handle the
    /// rest of the cli uses to push state updates.
    ///
    /// **Cooked mode, not raw.** A previous iteration called
    /// `enable_raw_mode()` here — that's what `ratatui` examples do
    /// because they're handling keyboard input. We don't, and raw
    /// mode breaks the rest of the dev loop: every `eprintln!` that
    /// other crates use to write sections / steps / device logs
    /// emits a bare `\n` (no `\r`), which under raw-mode tty
    /// settings moves the cursor down one row but leaves it at the
    /// same column the previous line ended at. The visible result
    /// is a staircase of `⏵`/`✓` rows drifting rightward across the
    /// screen. Skipping `enable_raw_mode` keeps `\n` → CRLF
    /// translation on, so all of the legacy line-based output
    /// scrolls cleanly at column 0 above the viewport.
    pub fn start(state: TuiState) -> Result<Self> {
        let mut stderr = std::io::stderr();
        stderr.execute(cursor::Hide)?;
        // 3-line viewport: 1 line for a separator, 2 lines of content.
        // Lean compact — power users want screen real estate; a wider
        // status surface is its own opt-in feature later.
        let backend = CrosstermBackend::new(stderr);
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(3),
            },
        )?;
        let state = Arc::new(Mutex::new(state));

        // Cleanup hooks for the two ways a `whisker run` can exit
        // without our normal `Tui::shutdown` running:
        //
        // 1. **Panic** — `std::panic::set_hook` chains: our hook
        //    restores cursor visibility + clears below it before
        //    chaining to whatever was installed before us (typically
        //    the default panic printer, which writes the message to
        //    stderr — that lands in a clean terminal because we
        //    cleaned up first).
        // 2. **Ctrl-C** — the default SIGINT handler kills the
        //    process before any Drop / shutdown code runs, leaving
        //    the cursor hidden. `ctrlc::set_handler` installs an
        //    OS-level handler that runs our cleanup then exits.
        install_terminal_cleanup_once();

        Ok(Self { terminal, state })
    }

    /// Cheap-to-clone handle for pushing state updates from event
    /// callbacks running on background threads / tasks.
    pub fn state_handle(&self) -> Arc<Mutex<TuiState>> {
        Arc::clone(&self.state)
    }

    /// Draw one frame using the current state snapshot.
    pub fn draw(&mut self) -> Result<()> {
        let snapshot = {
            let g = self.state.lock().expect("tui state mutex poisoned");
            g.clone()
        };
        self.terminal.draw(|frame| {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(frame.area());
            // Separator line — visually anchors the bar against the
            // scrollback content above.
            let sep = Paragraph::new("─".repeat(frame.area().width as usize))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(sep, layout[0]);
            // Row 1: target · bind · clients.
            let top = Line::from(vec![
                Span::styled(
                    " whisker run ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::raw(snapshot.target.clone()),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::raw(snapshot.bind.clone()),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{} client(s)", snapshot.client_count)),
            ]);
            frame.render_widget(Paragraph::new(top), layout[1]);
            // Row 2: phase + elapsed, then trailing outcome.
            let phase_span = match snapshot.phase.elapsed() {
                Some(d) => Span::styled(
                    format!(" {} · {} ", snapshot.phase.label(), fmt_elapsed(d)),
                    Style::default().fg(Color::Yellow),
                ),
                None => Span::styled(
                    format!(" {} ", snapshot.phase.label()),
                    Style::default().fg(Color::DarkGray),
                ),
            };
            let last_span = match &snapshot.last {
                LastOutcome::None => Span::styled("", Style::default()),
                LastOutcome::Success(s) => {
                    Span::styled(format!("✓ {s}"), Style::default().fg(Color::Green))
                }
                LastOutcome::Failure(s) => {
                    Span::styled(format!("✗ {s}"), Style::default().fg(Color::Red))
                }
            };
            let bottom = Line::from(vec![phase_span, Span::raw("  "), last_span]);
            frame.render_widget(Paragraph::new(bottom), layout[2]);
        })?;
        Ok(())
    }

    /// Drop the inline viewport + restore the cursor. We never
    /// flipped to raw mode (see [`Tui::start`]), so there's nothing
    /// to undo there — just clear the viewport region and re-show
    /// the cursor so the user's shell prompt lands on a clean line.
    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.terminal.clear();
        let mut stderr = std::io::stderr();
        let _ = stderr.execute(cursor::Show);
        let _ = stderr.execute(Clear(ClearType::FromCursorDown));
        Ok(())
    }
}

/// Restore the cursor + clear anything below it. Used by both the
/// panic hook and the Ctrl-C handler.
fn emergency_terminal_reset() {
    let mut stderr = std::io::stderr();
    let _ = stderr.execute(cursor::Show);
    let _ = stderr.execute(Clear(ClearType::FromCursorDown));
}

/// Install the panic hook + SIGINT handler. Idempotent — only the
/// first `Tui::start` of the process installs anything; later calls
/// are no-ops. The handlers stay registered for the rest of the
/// process lifetime (no good way to unregister either), but that's
/// fine: their cleanup is a no-op on already-cooked stderr.
fn install_terminal_cleanup_once() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return;
    }
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        emergency_terminal_reset();
        prev_hook(info);
    }));
    // `ctrlc::set_handler` errors if a handler is already registered.
    // We treat that as benign — somebody else cared about the same
    // cleanup, our reset would just run twice in the worst case.
    let _ = ctrlc::set_handler(|| {
        emergency_terminal_reset();
        std::process::exit(130); // 128 + SIGINT
    });
}

/// `1.2s` / `730ms` / `2m05s` formatter mirroring
/// `whisker_build::ui::format_elapsed`. Local copy because the cli's
/// status surface owes its formatting consistency to the same
/// magnitude breakpoints, but the ui crate's version is `pub(crate)`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_dev_server::Event;

    #[test]
    fn apply_started_moves_to_idle() {
        let mut s = TuiState::new("iOS", "127.0.0.1:9876");
        apply_event(&mut s, &Event::Started);
        assert!(matches!(s.phase, Phase::Idle));
    }

    #[test]
    fn building_then_succeeded_records_a_success_outcome() {
        let mut s = TuiState::new("iOS", "127.0.0.1:9876");
        apply_event(&mut s, &Event::BuildingFull);
        assert!(matches!(s.phase, Phase::Building { .. }));
        apply_event(&mut s, &Event::BuildSucceeded);
        assert!(matches!(s.phase, Phase::Idle));
        assert!(matches!(s.last, LastOutcome::Success(_)));
    }

    #[test]
    fn build_failed_records_failure_with_message() {
        let mut s = TuiState::new("Android", "127.0.0.1:9876");
        apply_event(&mut s, &Event::BuildingFull);
        apply_event(&mut s, &Event::BuildFailed("rustc died".into()));
        assert!(matches!(s.phase, Phase::Idle));
        match s.last {
            LastOutcome::Failure(msg) => assert!(msg.contains("rustc died")),
            other => panic!("expected Failure, got {other:?}"),
        }
    }

    #[test]
    fn client_counter_increments_and_decrements_with_saturation() {
        let mut s = TuiState::new("iOS", "127.0.0.1:9876");
        apply_event(&mut s, &Event::ClientConnected);
        apply_event(&mut s, &Event::ClientConnected);
        assert_eq!(s.client_count, 2);
        apply_event(&mut s, &Event::ClientDisconnected);
        assert_eq!(s.client_count, 1);
        // Underflow is saturating — never panics.
        apply_event(&mut s, &Event::ClientDisconnected);
        apply_event(&mut s, &Event::ClientDisconnected);
        assert_eq!(s.client_count, 0);
    }

    #[test]
    fn patch_sent_records_success_with_elapsed_from_patching_phase() {
        let mut s = TuiState::new("iOS", "127.0.0.1:9876");
        s.phase = Phase::Patching {
            started_at: Instant::now() - Duration::from_millis(615),
        };
        apply_event(&mut s, &Event::PatchSent);
        assert!(matches!(s.phase, Phase::Idle));
        match s.last {
            LastOutcome::Success(msg) => assert!(msg.starts_with("patch ok ")),
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn device_log_does_not_modify_state() {
        let mut s = TuiState::new("iOS", "127.0.0.1:9876");
        s.phase = Phase::Building {
            started_at: Instant::now(),
        };
        let original_phase = matches!(s.phase, Phase::Building { .. });
        apply_event(
            &mut s,
            &Event::DeviceLog {
                stream: "stdout".into(),
                line: "hello".into(),
                ts_micros: 0,
            },
        );
        assert!(original_phase && matches!(s.phase, Phase::Building { .. }));
    }

    #[test]
    fn fmt_elapsed_chooses_right_unit_by_magnitude() {
        assert_eq!(fmt_elapsed(Duration::from_millis(42)), "42ms");
        assert_eq!(fmt_elapsed(Duration::from_millis(1_500)), "1.5s");
        assert_eq!(fmt_elapsed(Duration::from_secs(125)), "2m05s");
    }
}
