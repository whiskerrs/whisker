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
    // The section header comes from `whisker_build::ui::section`
    // via captured stderr, so neither event emits a history row.
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
    // The dev-server's own ui::warn line reaches scrollback via
    // captured stderr — no duplicate history row from the event.
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
    assert!(rendered.contains("quit"));
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
