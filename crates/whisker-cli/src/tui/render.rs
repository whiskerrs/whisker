use super::*;

pub(super) fn render_live(frame: &mut ratatui::Frame, state: &LiveState, spinner_idx: usize) {
    let area = frame.area();
    let lines = build_live_lines(state, spinner_idx);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

pub(super) fn build_live_lines(state: &LiveState, spinner_idx: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header line: ` <STATUS>  <target> · <bundle> [· <elapsed>] `.
    let (chip_label, chip_bg, chip_fg) = status_chip(state);
    let mut header: Vec<Span<'static>> = vec![
        Span::styled(
            format!(" {chip_label} "),
            Style::default()
                .fg(chip_fg)
                .bg(chip_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            state.target.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::raw(state.bundle.clone()),
    ];
    if let Some(extra) = phase_elapsed(&state.phase) {
        header.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
        header.push(Span::styled(extra, Style::default().fg(Color::DarkGray)));
    }
    lines.push(Line::from(header));

    match (&state.current_step, &state.phase) {
        (Some(label), _) => {
            let spinner = SPINNER_FRAMES[spinner_idx % SPINNER_FRAMES.len()];
            // Spinner takes the chip's background colour so the header
            // and step row read as one indicator.
            let (_, chip_bg, _) = status_chip(state);
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(spinner.to_string(), Style::default().fg(chip_bg)),
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

    if state.workflow == WorkflowKind::Run {
        if let Some(destination) = &state.target_destination {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("target      ", Style::default().fg(Color::DarkGray)),
                Span::raw(destination.clone()),
            ]));
            let host = match &state.host_status {
                HostStatus::NotStarted => "not started".to_string(),
                HostStatus::Launching => "launching".to_string(),
                HostStatus::WaitingForConnection => "waiting for connection".to_string(),
                HostStatus::Connected => format!("connected ({})", state.client_count),
                HostStatus::Failed(reason) => format!("failed · {reason}"),
            };
            let mut watching = vec![
                Span::raw(" "),
                Span::styled("host        ", Style::default().fg(Color::DarkGray)),
                Span::raw(host),
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
    } else if let Some(artifact) = &state.artifact {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("artifact    ", Style::default().fg(Color::DarkGray)),
            Span::raw(artifact.clone()),
        ]));
    } else {
        // Reserve one row so the layout doesn't jiggle when the
        // artifact appears at the end of the build.
        lines.push(Line::from(""));
    }

    // Also the spacer row when no prompt is pending, so the layout
    // doesn't jiggle.
    match (&state.workflow, &state.full_reload_needed) {
        (WorkflowKind::Run, Some(reason)) => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("⚠ {reason} — press "),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    " R ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to Full Reload", Style::default().fg(Color::Yellow)),
            ]));
        }
        _ => lines.push(Line::from("")),
    }

    // Footer hint. The key chips use `White` (not `Black`) on the
    // dark-gray background so they stay legible in dark-themed
    // terminals where ANSI color 0 (`Color::Black`) resolves to the
    // terminal's *background* hue and visually disappears against
    // the chip's fill.
    let key_chip = |label: &str| {
        Span::styled(
            format!(" {label} "),
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
    };
    let key_desc =
        |text: &str| Span::styled(text.to_string(), Style::default().fg(Color::DarkGray));
    let footer = match state.workflow {
        WorkflowKind::Run => vec![
            Span::raw(" "),
            key_chip("r"),
            key_desc("  hot reload   "),
            key_chip("R"),
            key_desc("  full reload   "),
            key_chip("o"),
            key_desc("  relaunch   "),
            key_chip("q"),
            key_desc("  quit"),
        ],
        WorkflowKind::Build => vec![Span::raw(" "), key_chip("q"), key_desc("  cancel")],
    };
    lines.push(Line::from(footer));

    // Truncate / pad to LIVE_HEIGHT so the viewport renders cleanly.
    lines.truncate(LIVE_HEIGHT as usize);
    while lines.len() < LIVE_HEIGHT as usize {
        lines.push(Line::from(""));
    }
    lines
}

pub(super) fn render_history_item(item: &HistoryItem) -> Vec<Line<'static>> {
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
        HistoryItem::Message { level, text } => {
            let (glyph, color) = match level {
                MessageLevel::Info => ("·", Color::DarkGray),
                MessageLevel::Warning => ("⚠", Color::Yellow),
                MessageLevel::Error => ("✗", Color::Red),
                MessageLevel::Debug => ("·", Color::DarkGray),
                MessageLevel::Log => ("", Color::Reset),
            };
            if glyph.is_empty() {
                vec![Line::from(Span::raw(text.clone()))]
            } else {
                vec![Line::from(vec![
                    Span::styled(format!("{glyph} "), Style::default().fg(color)),
                    Span::raw(text.clone()),
                ])]
            }
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
pub(super) fn write_lines_to_buffer(buf: &mut Buffer, lines: &[Line<'static>]) {
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

/// Picks the leading status chip's (label, background, foreground)
/// triple for the current live state. `current_step` is consulted as
/// well as `phase` so a step running after `Event::BuildSucceeded`
/// (`xcodebuild` inside `installer.install_and_launch`, with the
/// phase already `Idle`) still reads as `BUILDING`.
///
/// Check order is significant: `Failed` outranks everything,
/// `Patching` outranks Building and Idle, an in-flight step outranks
/// bare Idle.
pub(super) fn status_chip(state: &LiveState) -> (&'static str, Color, Color) {
    if matches!(state.phase, AppPhase::Failed { .. }) {
        return ("FAILED", Color::Red, Color::White);
    }
    if matches!(state.phase, AppPhase::Patching { .. }) {
        return ("PATCHING", Color::Magenta, Color::Black);
    }
    if matches!(state.phase, AppPhase::Building { .. }) {
        return ("BUILDING", Color::Yellow, Color::Black);
    }
    if matches!(state.phase, AppPhase::Completed) {
        return ("BUILT", Color::Green, Color::Black);
    }
    if matches!(state.phase, AppPhase::Stopping) {
        return ("STOPPING", Color::DarkGray, Color::White);
    }
    if state.current_step.is_some() {
        return ("BUILDING", Color::Yellow, Color::Black);
    }
    // Idle with a step in flight is still install / launch work.
    if matches!(state.phase, AppPhase::Idle) {
        return match (&state.workflow, &state.host_status) {
            (WorkflowKind::Run, HostStatus::NotStarted | HostStatus::Launching) => {
                ("LAUNCHING", Color::Yellow, Color::Black)
            }
            (WorkflowKind::Run, HostStatus::WaitingForConnection) => {
                ("WAITING", Color::Yellow, Color::Black)
            }
            (WorkflowKind::Run, HostStatus::Failed(_)) => ("FAILED", Color::Red, Color::White),
            _ => ("RUNNING", Color::Green, Color::Black),
        };
    }
    // Setup / Initializing.
    ("STARTING", Color::DarkGray, Color::White)
}

pub(super) fn phase_elapsed(phase: &AppPhase) -> Option<String> {
    match phase {
        AppPhase::Building { started_at, .. } | AppPhase::Patching { started_at } => {
            Some(fmt_elapsed(started_at.elapsed()))
        }
        _ => None,
    }
}

pub(super) fn fmt_elapsed(d: Duration) -> String {
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
