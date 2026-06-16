//! Regression tests for the reactive-loop wedge (edge-triggered lost
//! wakeup).
//!
//! ## The bug being guarded
//!
//! The render loop is wake-driven: `signal.set()` → `schedule()` pushes
//! a node onto `rt.pending` and calls `host_wake::wake_runtime()` ONLY on
//! the empty→non-empty edge. The host's vsync loop (CADisplayLink) pauses
//! whenever a frame reports idle. A native-view layout/measure callback
//! can re-enter Rust DURING the final `renderer_flush` of a frame and
//! `schedule()` a signal write; that node lands in `rt.pending` but is
//! past the frame's drain. If idle were judged purely on "dispatch
//! completed", the host would pause with work still queued — and because
//! the queue is now non-empty, no later `set()` fires a wake → permanent
//! wedge (values change, screen never repaints).
//!
//! The fix is two-fold and these two tests **contrast the wedge**:
//!
//! - [`fixed_loop_drains_settle_and_tap_renders`] runs the loop with the
//!   PRODUCTION idle rule — idle == `!has_pending_work()`. A commit-time
//!   re-entry that schedules a settle node keeps the loop busy until the
//!   queue drains, so a subsequent "tap" re-renders. The loop makes
//!   progress.
//! - [`unfixed_loop_wedges_when_pending_left_dirty`] runs the SAME
//!   scenario with the PRE-FIX idle rule — idle is hard-coded `true` (the
//!   frame "always completed"). The commit-time re-entry leaves a node in
//!   `rt.pending`, the host pauses, and because `schedule()` only wakes on
//!   the empty→non-empty edge the later "tap" fires no wake. The render
//!   counter never advances → the wedge is reproduced.

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::MutexGuard;

use crate::reactive::effect::effect;
use crate::reactive::{__reset_for_tests, flush, has_pending_work, Owner, RwSignal};
use crate::tasks;

// These tests reach into process-global host state (the request-frame
// callback). Serialize on the shared host-test lock so sibling modules
// can't clear our wiring mid-test.
fn lock<'a>() -> MutexGuard<'a, ()> {
    crate::main_thread::host_test_lock()
}

// ----- Host model -----------------------------------------------------------
//
// Thread-locals modelling the host's vsync loop state and a wake counter.
// The request-frame callback (CADisplayLink unpause) sets `VSYNC_RUNNING`
// true and bumps `WAKE_COUNT`, exactly like the driver's host_wake →
// CADisplayLink.invalidate=false path.

thread_local! {
    static VSYNC_RUNNING: Cell<bool> = const { Cell::new(false) };
    static WAKE_COUNT: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn on_request_frame(_user_data: *mut c_void) {
    VSYNC_RUNNING.with(|v| v.set(true));
    WAKE_COUNT.with(|c| c.set(c.get() + 1));
}

fn install_host() {
    VSYNC_RUNNING.with(|v| v.set(false));
    WAKE_COUNT.with(|c| c.set(0));
    crate::host_wake::set_request_frame_callback(Some(on_request_frame), std::ptr::null_mut());
}

fn reset_all() {
    __reset_for_tests();
    tasks::__reset_for_tests();
    crate::host_wake::__reset_for_tests();
    VSYNC_RUNNING.with(|v| v.set(false));
    WAKE_COUNT.with(|c| c.set(0));
}

// ----- Frame model ----------------------------------------------------------
//
// `tick_frame_body` mirrors the driver's `tick_frame`: a reactive flush,
// a task drain, a second flush, then the "renderer_flush" — modelled by a
// re-entry hook that schedules `reentry` settle writes (the native-view
// layout callback re-entering Rust during commit), then the SAME-frame
// bounded settle loop the production `tick_frame` runs after
// `renderer_flush`.
//
// `reentry` is a Cell so the hook fires its re-entrant writes exactly
// ONCE (the first frame after a write), modelling a layout pass that
// settles after a single feedback iteration — not an infinite generator.

const SETTLE_CAP: usize = 16;

fn tick_frame_body(settle_signal: RwSignal<i32>, reentry: &Cell<u32>) {
    flush();
    tasks::run_until_stalled();
    flush();
    // "renderer_flush" commit: a native-view layout/measure callback
    // re-enters Rust and writes a signal. Fire the queued re-entries
    // exactly once.
    let pending_reentry = reentry.replace(0);
    for _ in 0..pending_reentry {
        // Off-by-one bump so the value genuinely changes each commit.
        settle_signal.set(settle_signal.get_untracked() + 1);
    }
    // SAME-frame settle loop (the production fix in `tick_frame`).
    let mut settle = 0;
    while has_pending_work() && settle < SETTLE_CAP {
        settle += 1;
        flush();
    }
}

/// One host vsync `tick()` using the PRODUCTION idle rule. Returns true
/// when the host may pause: idle == the queue is genuinely drained.
/// Mirrors `whisker-driver`'s `tick()` (`!dispatch_pending &&
/// !has_pending_work()`), where the synchronous frame makes
/// `!dispatch_pending` always hold on the TASM==main thread.
fn tick_fixed(settle_signal: RwSignal<i32>, reentry: &Cell<u32>) -> bool {
    tick_frame_body(settle_signal, reentry);
    !has_pending_work()
}

/// One host vsync `tick()` using the PRE-FIX idle rule: the frame
/// "always completed", so it reports idle unconditionally — IGNORING any
/// node a commit-time re-entry left in `rt.pending`. This is the buggy
/// `!dispatch_pending` (with no `has_pending_work` term) and no
/// same-frame settle loop.
fn tick_unfixed(settle_signal: RwSignal<i32>, reentry: &Cell<u32>) {
    // No settle loop: drain only what the in-frame flush sees, then leave
    // any commit-time re-entry node dirty in the queue.
    flush();
    tasks::run_until_stalled();
    flush();
    let pending_reentry = reentry.replace(0);
    for _ in 0..pending_reentry {
        settle_signal.set(settle_signal.get_untracked() + 1);
    }
    // NOTE: deliberately NO settle loop and the caller treats idle as a
    // hard-coded `true` — the node scheduled above is left undrained.
}

// ----- Tests ----------------------------------------------------------------

/// FIXED loop: with the production level-triggered idle + same-frame
/// settle loop, a commit-time re-entry never wedges the loop and a later
/// "tap" re-renders. Contrast with
/// [`unfixed_loop_wedges_when_pending_left_dirty`], which runs the same
/// scenario under the pre-fix idle rule and stays frozen.
#[test]
fn fixed_loop_drains_settle_and_tap_renders() {
    let _g = lock();
    reset_all();
    install_host();

    let owner = Owner::new(None);
    owner.with(|| {
        let count = RwSignal::new(0_i32);
        let settle_signal = RwSignal::new(0_i32);

        // The render binding: re-runs (increments `render_count`) whenever
        // `count` OR `settle_signal` changes — modelling a `{expr}` text
        // binding subscribing to both.
        let render_count = Rc::new(Cell::new(0u32));
        let rc = render_count.clone();
        effect(move || {
            count.get();
            settle_signal.get();
            rc.set(rc.get() + 1);
        });
        // Initial effect run.
        flush();
        let baseline = render_count.get();

        // A native-view layout callback will re-enter and write
        // `settle_signal` ONCE during the next frame's commit.
        let reentry = Cell::new(1u32);

        // ----- Drive the host loop (bounded iterations) -----
        // The vsync loop ticks while running; on idle it pauses and only a
        // wake (from a `set()` empty→non-empty edge) resumes it.
        let mut vsync_idle = false;
        for _ in 0..50 {
            if VSYNC_RUNNING.with(|v| v.get()) {
                vsync_idle = tick_fixed(settle_signal, &reentry);
                if vsync_idle {
                    VSYNC_RUNNING.with(|v| v.set(false));
                }
            } else {
                // Vsync paused. The settle re-entry must already have been
                // drained in-frame, so pausing here is correct. Fire the
                // "tap" exactly once: a user write that must wake the loop.
                break;
            }
        }
        assert!(
            vsync_idle,
            "fixed loop should reach idle after draining settle"
        );
        // The commit-time re-entry rendered in the same frame.
        assert!(
            render_count.get() > baseline,
            "settle re-entry must have re-rendered (render_count advanced past baseline)"
        );

        let after_settle = render_count.get();

        // ----- Tap while paused ----- a single user `set()` must wake the
        // host (empty→non-empty edge) and the resumed loop must re-render.
        let wakes_before = WAKE_COUNT.with(|c| c.get());
        count.set(1);
        assert!(
            WAKE_COUNT.with(|c| c.get()) > wakes_before,
            "tap's set() must fire a host wake (queue was empty → non-empty)"
        );
        assert!(
            VSYNC_RUNNING.with(|v| v.get()),
            "wake must unpause the vsync loop"
        );

        // Drain the resumed loop.
        let mut settled = false;
        for _ in 0..50 {
            if VSYNC_RUNNING.with(|v| v.get()) {
                if tick_fixed(settle_signal, &reentry) {
                    VSYNC_RUNNING.with(|v| v.set(false));
                    settled = true;
                    break;
                }
            } else {
                settled = true;
                break;
            }
        }
        assert!(settled, "resumed loop should settle");
        assert!(
            render_count.get() > after_settle,
            "tap must re-render: render_count advanced after the user set()"
        );
    });
    owner.dispose();
    reset_all();
}

/// UNFIXED loop: with the pre-fix idle rule (frame always reports idle,
/// no same-frame settle), a commit-time re-entry leaves a node dirty in
/// `rt.pending`. The host pauses; because `schedule()` only wakes on the
/// empty→non-empty edge and the queue is already non-empty, a later "tap"
/// `set()` fires NO wake — the loop stays frozen and never re-renders.
/// This reproduces the wedge that
/// [`fixed_loop_drains_settle_and_tap_renders`] proves the fix closes.
#[test]
fn unfixed_loop_wedges_when_pending_left_dirty() {
    let _g = lock();
    reset_all();
    install_host();

    let owner = Owner::new(None);
    owner.with(|| {
        let count = RwSignal::new(0_i32);
        let settle_signal = RwSignal::new(0_i32);

        let render_count = Rc::new(Cell::new(0u32));
        let rc = render_count.clone();
        effect(move || {
            count.get();
            settle_signal.get();
            rc.set(rc.get() + 1);
        });
        flush();

        // Commit-time re-entry writes `settle_signal` ONCE during the next
        // frame's commit, leaving its subscriber node dirty in `pending`.
        let reentry = Cell::new(1u32);

        // Drive the host loop with the PRE-FIX idle (hard-coded true: the
        // frame "always completed"). After one tick the re-entry node sits
        // undrained in `pending`, and the host pauses.
        let mut vsync_idle = false;
        for _ in 0..50 {
            if VSYNC_RUNNING.with(|v| v.get()) {
                tick_unfixed(settle_signal, &reentry);
                // PRE-FIX: idle is unconditionally true (no
                // `has_pending_work` term, no settle loop).
                vsync_idle = true;
                VSYNC_RUNNING.with(|v| v.set(false));
            } else {
                break;
            }
        }
        assert!(vsync_idle, "unfixed loop pauses (frame reports idle)");

        // The wedge precondition: a node was left dirty in `pending`.
        assert!(
            has_pending_work(),
            "pre-fix frame must leave the commit-time re-entry node undrained"
        );

        let render_before_tap = render_count.get();

        // ----- Tap while wedged ----- the queue is already non-empty, so
        // `schedule()` takes the was_empty == false branch and fires NO
        // wake. The vsync loop stays paused.
        let wakes_before = WAKE_COUNT.with(|c| c.get());
        count.set(1);
        assert_eq!(
            WAKE_COUNT.with(|c| c.get()),
            wakes_before,
            "WEDGE: with a non-empty queue, tap's set() fires no edge-triggered wake"
        );
        assert!(
            !VSYNC_RUNNING.with(|v| v.get()),
            "WEDGE: vsync loop stays paused after the tap"
        );

        // The loop never resumes → render counter never advances.
        for _ in 0..50 {
            if VSYNC_RUNNING.with(|v| v.get()) {
                tick_unfixed(settle_signal, &reentry);
                VSYNC_RUNNING.with(|v| v.set(false));
            } else {
                break;
            }
        }
        assert_eq!(
            render_count.get(),
            render_before_tap,
            "WEDGE: render counter never advances — the loop is permanently frozen"
        );
    });
    owner.dispose();
    reset_all();
}
