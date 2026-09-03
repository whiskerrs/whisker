use super::*;
use whisker_dev_server::Event;

fn s() -> LiveState {
    LiveState::new(WorkflowKind::Run, "iOS Simulator", "rs.whisker.podcast")
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
    // The section header comes from `whisker_build::ui::section`
    // through structured progress, so neither event duplicates it.
    assert!(started.is_empty());
    let done = drain(&mut st, &Event::BuildSucceeded);
    assert!(matches!(st.phase, AppPhase::Idle));
    assert!(st.last_build.is_some());
    assert!(done.is_empty());
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
fn host_lifecycle_tracks_launch_and_connection() {
    let mut st = s();
    drain(&mut st, &Event::HostLaunching);
    assert_eq!(st.host_status, HostStatus::Launching);
    drain(&mut st, &Event::HostLaunched);
    assert_eq!(st.host_status, HostStatus::WaitingForConnection);
    drain(&mut st, &Event::ClientConnected);
    assert_eq!(st.host_status, HostStatus::Connected);
    drain(&mut st, &Event::ClientDisconnected);
    assert_eq!(st.host_status, HostStatus::WaitingForConnection);
}

#[test]
fn run_quit_requests_graceful_dev_server_shutdown() {
    let mut st = s();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    st.command_tx = Some(tx);

    request_user_quit(&mut st);

    assert_eq!(rx.try_recv(), Ok(whisker_dev_server::DevCommand::Shutdown));
    assert!(matches!(st.phase, AppPhase::Stopping));
    assert!(!st.should_quit);
    assert!(!st.force_exit);
}

#[test]
fn build_quit_uses_cancellation_path() {
    let mut st = LiveState::new(WorkflowKind::Build, "Web", "host-smoke");
    request_user_quit(&mut st);
    assert!(st.should_quit);
    assert!(st.force_exit);
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
    // No history row: the dev-server's own `✓ patch hot reload …`
    // step line already covers it.
    assert!(h.is_empty());
}

#[test]
fn patch_building_transitions_phase_to_patching() {
    let mut st = s();
    st.phase = AppPhase::Idle;
    let h = drain(&mut st, &Event::PatchBuilding);
    assert!(
        matches!(st.phase, AppPhase::Patching { .. }),
        "phase should be Patching after PatchBuilding"
    );
    assert!(h.is_empty(), "PatchBuilding shouldn't emit history rows");
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
fn full_reload_required_sets_banner_and_returns_to_idle() {
    let mut st = s();
    // A hot-reload attempt flips into Patching first, then the
    // dev loop discovers it can't proceed.
    drain(&mut st, &Event::PatchBuilding);
    let h = drain(
        &mut st,
        &Event::FullReloadRequired {
            reason: "Cargo.toml changed".into(),
        },
    );
    assert_eq!(st.full_reload_needed.as_deref(), Some("Cargo.toml changed"));
    assert!(
        matches!(st.phase, AppPhase::Idle),
        "spinner must not keep running after a declined reload"
    );
    // The dev-server's own ui::warn event reaches scrollback — no
    // duplicate history row from the lifecycle event.
    assert!(h.is_empty());
}

#[test]
fn building_full_clears_the_full_reload_banner() {
    let mut st = s();
    drain(
        &mut st,
        &Event::FullReloadRequired {
            reason: "Cargo.toml changed".into(),
        },
    );
    assert!(st.full_reload_needed.is_some());
    drain(&mut st, &Event::BuildingFull);
    assert!(
        st.full_reload_needed.is_none(),
        "starting a Full Reload acts on the prompt"
    );
}

#[test]
fn patch_sent_keeps_the_full_reload_banner() {
    // A successful hot reload does NOT satisfy a pending
    // dependency-graph prompt — the new dep still isn't on the
    // device until a Full Reload runs.
    let mut st = s();
    drain(
        &mut st,
        &Event::FullReloadRequired {
            reason: "Cargo.toml changed".into(),
        },
    );
    drain(&mut st, &Event::PatchBuilding);
    drain(&mut st, &Event::PatchSent);
    assert!(st.full_reload_needed.is_some());
}

#[test]
fn build_live_lines_renders_full_reload_banner() {
    let mut st = s();
    st.full_reload_needed = Some("Cargo.toml changed".into());
    let lines = build_live_lines(&st, 0);
    let rendered = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|sp| sp.content.to_string()))
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("Cargo.toml changed"));
    assert!(rendered.contains("Full Reload"));
}

#[test]
fn build_live_lines_footer_lists_reload_shortcuts() {
    let st = s();
    let lines = build_live_lines(&st, 0);
    let rendered = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|sp| sp.content.to_string()))
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("hot reload"));
    assert!(rendered.contains("full reload"));
    assert!(rendered.contains("relaunch"));
    assert!(rendered.contains("quit"));
}

#[test]
fn build_footer_only_offers_cancel() {
    let st = LiveState::new(WorkflowKind::Build, "Web", "host-smoke");
    let lines = build_live_lines(&st, 0);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("cancel"));
    assert!(!rendered.contains("hot reload"));
}

#[test]
fn structured_progress_drives_current_operation() {
    let live = Arc::new(Mutex::new(s()));
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = TuiHandle {
        live: Arc::clone(&live),
        tx,
    };

    handle.apply_progress_event(ProgressEvent::OperationStarted {
        kind: whisker_build::ui::OperationKind::Install,
        detail: "app-debug.apk".into(),
    });
    assert_eq!(
        handle.snapshot().current_step.as_deref(),
        Some("install · app-debug.apk")
    );

    handle.apply_progress_event(ProgressEvent::OperationFinished {
        kind: whisker_build::ui::OperationKind::Install,
        detail: "app-debug.apk".into(),
        outcome: whisker_build::ui::OperationOutcome::Done,
        summary: String::new(),
        elapsed: Duration::from_millis(12),
    });
    assert!(handle.snapshot().current_step.is_none());
    assert!(matches!(rx.try_recv(), Ok(HistoryItem::Step { .. })));
}

#[test]
fn finite_build_progress_keeps_the_workflow_in_building_phase_between_steps() {
    let live = Arc::new(Mutex::new(LiveState::new(
        WorkflowKind::Build,
        "Web",
        "host-smoke",
    )));
    let (tx, _rx) = std::sync::mpsc::channel();
    let handle = TuiHandle {
        live: Arc::clone(&live),
        tx,
    };

    handle.apply_progress_event(ProgressEvent::OperationStarted {
        kind: whisker_build::ui::OperationKind::Compile,
        detail: "host-smoke".into(),
    });
    handle.apply_progress_event(ProgressEvent::OperationFinished {
        kind: whisker_build::ui::OperationKind::Compile,
        detail: "host-smoke".into(),
        outcome: whisker_build::ui::OperationOutcome::Done,
        summary: String::new(),
        elapsed: Duration::from_millis(10),
    });

    assert!(matches!(handle.snapshot().phase, AppPhase::Building { .. }));
    assert!(handle.snapshot().current_step.is_none());
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
    st.target_destination = Some("iPhone 17 Pro · rs.whisker.podcast".into());
    st.client_count = 1;
    st.host_status = HostStatus::Connected;
    let lines = build_live_lines(&st, 0);
    let rendered = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|sp| sp.content.to_string()))
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("iPhone 17 Pro"));
    assert!(rendered.contains("connected (1)"));
}
