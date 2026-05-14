//! File watcher + change classifier for the dev loop.
//!
//! Wraps `notify` so the rest of the dev server (the builder in I4e
//! and the Tier 1 patcher in I4g) doesn't have to deal with raw
//! filesystem events. Three things happen here:
//!
//! 1. **Recursive watch** of a package root.
//! 2. **Debounce** raw notify events for ~200 ms — a single file save
//!    can produce 3-5 notify events on macOS (atomic rename + chmod
//!    + …); we coalesce them into one [`Change`].
//! 3. **Classify** the affected paths into [`ChangeKind`] so callers
//!    can pick a rebuild strategy (Tier 2 cold rebuild for
//!    `RustCode`, full restart for `CargoToml`, etc).

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// What sort of change happened. The classifier picks the *most
/// disruptive* category among the paths in a single debounced batch
/// (Cargo.toml beats Rust code beats anything else).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// `.rs` files inside the watched tree. Tier 2 cold rebuild
    /// today; Tier 1 subsecond patch once I4g lands.
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

/// Spawn a recursive watcher on `root`. Debounced [`Change`] batches
/// arrive on `tx`. The returned [`RecommendedWatcher`] keeps the OS
/// watch alive — drop it to stop watching.
pub fn spawn_watcher(
    root: PathBuf,
    debounce: Duration,
    tx: mpsc::Sender<Change>,
) -> Result<RecommendedWatcher> {
    let (raw_tx, raw_rx) = std_mpsc::channel::<Event>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(ev) = res {
            // Drop send errors silently — receiver gone means the
            // dev loop is shutting down.
            let _ = raw_tx.send(ev);
        }
    })
    .context("create notify watcher")?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .with_context(|| format!("watch {}", root.display()))?;

    // Debounce loop: separate OS thread because notify's callback is
    // synchronous and the std mpsc::Receiver isn't async-friendly.
    std::thread::Builder::new()
        .name("tuft-dev-watch".into())
        .spawn(move || debounce_loop(raw_rx, debounce, tx))
        .context("spawn debounce thread")?;

    Ok(watcher)
}

fn debounce_loop(
    raw_rx: std_mpsc::Receiver<Event>,
    debounce: Duration,
    tx: mpsc::Sender<Change>,
) {
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
        let p =
            std::env::temp_dir().join(format!("tuft-watcher-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn editing_a_rust_file_emits_a_rustcode_change() {
        let dir = unique_tempdir();
        std::fs::write(dir.join("lib.rs"), "fn old() {}").unwrap();

        let (tx, mut rx) = mpsc::channel::<Change>(8);
        let _watcher = spawn_watcher(
            dir.clone(),
            Duration::from_millis(120),
            tx,
        )
        .expect("watcher up");

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
        let _watcher = spawn_watcher(
            dir.clone(),
            Duration::from_millis(120),
            tx,
        )
        .expect("watcher up");

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

    #[tokio::test]
    async fn rapid_edits_get_coalesced_into_one_change() {
        let dir = unique_tempdir();
        std::fs::write(dir.join("a.rs"), "fn a() {}").unwrap();

        let (tx, mut rx) = mpsc::channel::<Change>(8);
        let _watcher = spawn_watcher(
            dir.clone(),
            Duration::from_millis(150),
            tx,
        )
        .expect("watcher up");

        tokio::time::sleep(Duration::from_millis(50)).await;
        // 5 rapid edits within the debounce window.
        for i in 0..5 {
            std::fs::write(dir.join("a.rs"), format!("fn a{i}() {{}}")).unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let first = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("change should arrive")
            .expect("channel closed");
        assert_eq!(first.kind, ChangeKind::RustCode);

        // After waiting > debounce window, no second change should
        // appear (everything coalesced into the first).
        let second =
            tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
        assert!(
            second.is_err(),
            "expected no second change after coalescing, got {second:?}",
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
