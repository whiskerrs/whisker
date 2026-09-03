//! Best-effort reaping of long-running build children when `whisker
//! run` exits.
//!
//! The `whisker run` TUI quits (`q` / Ctrl-C) by calling
//! [`std::process::exit`], which skips destructors — so `kill_on_drop`
//! and any RAII cleanup never fires and an in-flight `cargo` /
//! `gradle` / `xcodebuild` keeps running headless, holding build locks
//! and racing the next `whisker run`. Every long-running build spawn
//! therefore registers its PID via [`track`], and the quit path calls
//! [`kill_all`] right before `std::process::exit`.
//!
//! **SIGTERM, not SIGKILL** — gives the build tool a chance to remove
//! partial artifacts. **The gradle daemon is intentionally spared**:
//! it double-forks into its own session at startup, so it is never one
//! of *our* direct children. We only signal the `gradlew` wrapper we
//! launched; the daemon stays warm for the next run.

use std::sync::Mutex;

/// PIDs of build children currently in flight. A `static` `Mutex` (both
/// `Mutex::new` and `Vec::new` are `const`) so there's no init step.
static TRACKED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Register a freshly-spawned child PID. Hold the returned guard for the
/// child's lifetime (typically a local `let _guard = track(child.id())`
/// right after spawn); dropping it unregisters the PID so a child that
/// finished normally isn't signalled by a later [`kill_all`].
#[must_use = "dropping the guard immediately unregisters the child"]
pub fn track(pid: u32) -> TrackGuard {
    if let Ok(mut g) = TRACKED.lock() {
        g.push(pid);
    }
    TrackGuard { pid }
}

/// Unregisters its PID on drop. See [`track`].
pub struct TrackGuard {
    pid: u32,
}

impl Drop for TrackGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = TRACKED.lock()
            && let Some(i) = g.iter().position(|&p| p == self.pid)
        {
            g.swap_remove(i);
        }
    }
}

/// SIGTERM every still-tracked child. Call once, right before
/// `std::process::exit`, from the `whisker run` quit path. Best-effort:
/// a child that already exited just yields `ESRCH`, which we ignore.
pub fn kill_all() {
    let pids: Vec<u32> = TRACKED.lock().map(|g| g.clone()).unwrap_or_default();
    for pid in pids {
        // SAFETY: `kill(2)` with any pid value is memory-safe; a stale
        // or already-reaped pid returns `ESRCH` rather than affecting an
        // unrelated process (PIDs aren't reused within a single run).
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_registers_then_unregisters() {
        // `TRACKED` is a process-global the parallel sibling tests also
        // write to, so assert on this pid only, never on the length.
        {
            let _g = track(999_999_001);
            assert!(TRACKED.lock().unwrap().contains(&999_999_001));
        }
        assert!(!TRACKED.lock().unwrap().contains(&999_999_001));
    }

    #[test]
    fn kill_all_tolerates_stale_pid() {
        let g = track(999_999_002);
        std::mem::forget(g);
        kill_all();
        // Unregister by hand — the leaked guard never will, and the
        // registry is shared with the sibling tests.
        if let Ok(mut t) = TRACKED.lock() {
            t.retain(|&p| p != 999_999_002);
        }
    }
}
