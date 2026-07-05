//! File watcher + change classifier for the dev loop.
//!
//! Wraps `notify` so the rest of the dev server (the builder in I4e
//! and the hot-reload patcher in I4g) doesn't have to deal with raw
//! filesystem events. Three things happen here:
//!
//! 1. **Recursive watch** of a package root.
//! 2. **Debounce** raw notify events for ~200 ms — a single file save
//!    can produce 3-5 notify events on macOS (atomic rename + chmod
//!    + …); we coalesce them into one [`Change`].
//! 3. **Classify** the affected paths into [`ChangeKind`] so callers
//!    can pick a rebuild strategy (full reload for
//!    `RustCode`, full restart for `CargoToml`, etc).

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// What sort of change happened. The classifier picks the *most
/// disruptive* category among the paths in a single debounced batch
/// (Cargo.toml beats Rust code beats anything else).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// `.rs` files inside the watched tree. full reload
    /// today; hot reload subsecond patch once I4g lands.
    RustCode,
    /// `Cargo.toml` (or `Cargo.lock`) — needs a full
    /// `cargo build` and re-launch; subsecond can't reload deps.
    CargoToml,
    /// Anything else (assets, README edits while watching too wide a
    /// tree, …). Callers may choose to ignore.
    Other,
}

/// One debounced change batch.
#[derive(Debug, Clone)]
pub struct Change {
    pub kind: ChangeKind,
    pub paths: Vec<PathBuf>,
}

impl Change {
    /// Classify a batch of paths. The most disruptive category wins.
    pub fn classify(paths: Vec<PathBuf>) -> Self {
        let kind = if paths.iter().any(|p| {
            matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some("Cargo.toml") | Some("Cargo.lock"),
            )
        }) {
            ChangeKind::CargoToml
        } else if paths
            .iter()
            .any(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        {
            ChangeKind::RustCode
        } else {
            ChangeKind::Other
        };
        Self { kind, paths }
    }
}

/// Spawn a recursive watcher rooted at each path in `roots`.
/// Debounced [`Change`] batches arrive on `tx`. The returned
/// [`RecommendedWatcher`] keeps the OS watches alive — drop it to
/// stop watching.
///
/// Roots that fail to attach (don't exist, or notify rejects them)
/// log a warning and are skipped; the watcher still returns as long
/// as at least one root attached. Sub-crates without a `src/` are
/// the common case — `cargo metadata` lists every workspace member,
/// not every member ships a `src/` (proc-macro-only crates, virtual
/// manifest stubs, …).
pub fn spawn_watcher(
    roots: Vec<PathBuf>,
    debounce: Duration,
    tx: mpsc::Sender<Change>,
) -> Result<RecommendedWatcher> {
    if roots.is_empty() {
        anyhow::bail!("spawn_watcher: no roots to watch");
    }
    let (raw_tx, raw_rx) = std_mpsc::channel::<Event>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(ev) = res {
            // Drop send errors silently — receiver gone means the
            // dev loop is shutting down.
            let _ = raw_tx.send(ev);
        }
    })
    .context("create notify watcher")?;
    let mut attached = 0;
    for root in &roots {
        match watcher.watch(root, RecursiveMode::Recursive) {
            Ok(()) => attached += 1,
            Err(e) => whisker_build::ui::warn(format!("skip watch {}: {e}", root.display())),
        }
    }
    if attached == 0 {
        anyhow::bail!(
            "spawn_watcher: no roots successfully attached (of {})",
            roots.len()
        );
    }

    // Debounce loop: separate OS thread because notify's callback is
    // synchronous and the std mpsc::Receiver isn't async-friendly.
    std::thread::Builder::new()
        .name("whisker-dev-watch".into())
        .spawn(move || debounce_loop(raw_rx, debounce, tx))
        .context("spawn debounce thread")?;

    Ok(watcher)
}

fn debounce_loop(raw_rx: std_mpsc::Receiver<Event>, debounce: Duration, tx: mpsc::Sender<Change>) {
    let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
    let mut deadline: Option<Instant> = None;

    loop {
        let block_for = match deadline {
            // We have pending events — wait at most until the deadline
            // for any new event to coalesce.
            Some(d) => d.saturating_duration_since(Instant::now()),
            // Idle — block indefinitely for the next event.
            None => Duration::from_secs(60 * 60),
        };

        match raw_rx.recv_timeout(block_for) {
            Ok(ev) if is_interesting(&ev.kind) => {
                for p in ev.paths {
                    pending.insert(p);
                }
                deadline = Some(Instant::now() + debounce);
            }
            Ok(_) => {} // notify event we don't care about
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                if pending.is_empty() {
                    continue;
                }
                let paths: Vec<_> = std::mem::take(&mut pending).into_iter().collect();
                deadline = None;
                let change = Change::classify(paths);
                if tx.blocking_send(change).is_err() {
                    return; // receiver dropped, we're done
                }
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn is_interesting(k: &EventKind) -> bool {
    use notify::event::{CreateKind, ModifyKind, RemoveKind};
    matches!(
        k,
        EventKind::Create(CreateKind::File)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Any)
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Remove(RemoveKind::File)
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ----- classifier (pure) -------------------------------------------

    #[test]
    fn classify_picks_cargo_toml_over_rust_code() {
        let c = Change::classify(vec![
            "/tmp/foo/src/lib.rs".into(),
            "/tmp/foo/Cargo.toml".into(),
        ]);
        assert_eq!(c.kind, ChangeKind::CargoToml);
    }

    #[test]
    fn classify_picks_rust_code_when_no_cargo_toml() {
        let c = Change::classify(vec![
            "/tmp/foo/src/lib.rs".into(),
            "/tmp/foo/src/app.rs".into(),
        ]);
        assert_eq!(c.kind, ChangeKind::RustCode);
    }

    #[test]
    fn classify_falls_through_to_other() {
        let c = Change::classify(vec![
            "/tmp/foo/README.md".into(),
            "/tmp/foo/static/logo.png".into(),
        ]);
        assert_eq!(c.kind, ChangeKind::Other);
    }

    #[test]
    fn classify_handles_cargo_lock_too() {
        let c = Change::classify(vec!["/tmp/foo/Cargo.lock".into()]);
        assert_eq!(c.kind, ChangeKind::CargoToml);
    }

    // ----- end-to-end (notify + debounce) ------------------------------

    /// Each test gets its own tempdir so concurrent test runs don't
    /// see each other's events.
    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-watcher-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn editing_a_rust_file_emits_a_rustcode_change() {
        let dir = unique_tempdir();
        std::fs::write(dir.join("lib.rs"), "fn old() {}").unwrap();

        let (tx, mut rx) = mpsc::channel::<Change>(8);
        let _watcher =
            spawn_watcher(vec![dir.clone()], Duration::from_millis(120), tx).expect("watcher up");

        // Give notify a moment to register the watch.
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(dir.join("lib.rs"), "fn new() {}").unwrap();

        let change = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("debounced change should arrive within 3s")
            .expect("channel closed");

        assert_eq!(change.kind, ChangeKind::RustCode);
        assert!(
            change.paths.iter().any(|p| p.ends_with("lib.rs")),
            "paths={:?}",
            change.paths,
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn editing_cargo_toml_classifies_as_cargo_toml() {
        let dir = unique_tempdir();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();

        let (tx, mut rx) = mpsc::channel::<Change>(8);
        let _watcher =
            spawn_watcher(vec![dir.clone()], Duration::from_millis(120), tx).expect("watcher up");

        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();

        let change = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("change should arrive")
            .expect("channel closed");
        assert_eq!(change.kind, ChangeKind::CargoToml);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Synthesise a `Modify(Data(Content))` event for the given path.
    /// Used by the debounce tests so they don't depend on real
    /// filesystem timings — the e2e flavor (`editing_a_rust_file_*`
    /// above) already covers the notify-to-debouncer wiring.
    fn synth_modify(path: impl Into<PathBuf>) -> Event {
        use notify::event::{DataChange, ModifyKind};
        Event {
            kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            paths: vec![path.into()],
            attrs: notify::event::EventAttributes::new(),
        }
    }

    #[tokio::test]
    async fn rapid_edits_get_coalesced_into_one_change() {
        // Drive `debounce_loop` directly with synthetic events
        // instead of touching the filesystem + notify. The earlier
        // e2e version was flaky on slow CI runners (#30/#33/#34):
        // each `std::fs::write` + `tokio::time::sleep(20ms)` could
        // stretch past the 150 ms debounce window, splitting the
        // batch into two `Change`s and tripping the "expect no
        // second change" assertion. Feeding events through the std
        // channel keeps the test deterministic — the only timing
        // dependency left is the debounce window itself, which is
        // the thing under test.
        let debounce = Duration::from_millis(100);
        let (raw_tx, raw_rx) = std_mpsc::channel::<Event>();
        let (tx, mut rx) = mpsc::channel::<Change>(8);
        std::thread::spawn(move || debounce_loop(raw_rx, debounce, tx));

        // 5 rapid events back-to-back. No inter-send sleep — every
        // send hits the debouncer before the deadline fires, so they
        // coalesce into one `Change`.
        for i in 0..5 {
            raw_tx
                .send(synth_modify(PathBuf::from(format!("a{i}.rs"))))
                .unwrap();
        }

        let first = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("debounced change should arrive within 2s")
            .expect("channel closed");
        assert_eq!(first.kind, ChangeKind::RustCode);
        assert_eq!(
            first.paths.len(),
            5,
            "all 5 events should coalesce into one batch, got {:?}",
            first.paths,
        );

        // After waiting > debounce window, no second change should
        // appear — pending is drained, no new events fed.
        let second = tokio::time::timeout(debounce * 3, rx.recv()).await;
        assert!(
            second.is_err(),
            "expected no second change after coalescing, got {second:?}",
        );
    }

    #[tokio::test]
    async fn events_outside_debounce_window_split_into_two_changes() {
        // Inverse of the coalesce test: events separated by more
        // than the debounce window MUST surface as two distinct
        // `Change`s. Keeps us honest if someone "fixes" the
        // coalesce test by making the debouncer too greedy.
        let debounce = Duration::from_millis(80);
        let (raw_tx, raw_rx) = std_mpsc::channel::<Event>();
        let (tx, mut rx) = mpsc::channel::<Change>(8);
        std::thread::spawn(move || debounce_loop(raw_rx, debounce, tx));

        raw_tx.send(synth_modify("first.rs")).unwrap();
        let first = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("first change should arrive")
            .expect("channel closed");
        assert_eq!(first.paths.len(), 1);

        // Wait well past the debounce window so the deadline fires
        // before the next event, then send a second event.
        tokio::time::sleep(debounce * 3).await;
        raw_tx.send(synth_modify("second.rs")).unwrap();
        let second = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("second change should arrive")
            .expect("channel closed");
        assert_eq!(second.paths.len(), 1);
        assert!(second.paths[0].ends_with("second.rs"));
    }
}
