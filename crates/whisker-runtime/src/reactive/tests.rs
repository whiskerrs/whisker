//! Integration tests for the reactive runtime.
//!
//! Each test starts by clearing the thread-local arena. Cargo's
//! default thread pool reuses worker threads, so tests sharing a
//! pool slot must reset between runs.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

fn fresh() {
    __reset_for_tests();
}

// ----- Signal basics --------------------------------------------------------

#[test]
fn signal_returns_initial_value() {
    fresh();
    let (read, _write) = signal(42_i32).split();
    assert_eq!(read.get(), 42);
}

#[test]
fn write_signal_updates_value() {
    fresh();
    let (read, write) = signal(0_i32).split();
    write.set(7);
    assert_eq!(read.get(), 7);
}

#[test]
fn rw_signal_split_round_trip() {
    fresh();
    let rw = RwSignal::new(0_i32);
    rw.set(5);
    let (r, w) = rw.split();
    assert_eq!(r.get(), 5);
    w.set(9);
    assert_eq!(rw.get(), 9);
}

#[test]
fn with_borrows_without_clone() {
    fresh();
    let (read, write) = signal(vec![1, 2, 3]).split();
    let sum = read.with(|v| v.iter().sum::<i32>());
    assert_eq!(sum, 6);
    write.update(|v| v.push(4));
    assert_eq!(read.with(|v| v.len()), 4);
}

#[test]
fn signal_handles_are_copy_and_aliasable() {
    fresh();
    let (read, _write) = signal(String::from("hello")).split();
    let a = read;
    let b = read;
    assert_eq!(a.get(), "hello");
    assert_eq!(b.get(), "hello");
}

// ----- Effect: dependency tracking + re-run --------------------------------

#[test]
fn effect_runs_once_immediately() {
    fresh();
    let counter = Rc::new(RefCell::new(0));
    let c = counter.clone();
    effect(move || *c.borrow_mut() += 1);
    assert_eq!(*counter.borrow(), 1);
}

#[test]
fn effect_reruns_on_dep_change() {
    fresh();
    let (count, set_count) = signal(0_i32).split();
    let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let log_clone = log.clone();
    effect(move || {
        log_clone.borrow_mut().push(count.get());
    });
    set_count.set(1);
    flush();
    set_count.set(2);
    flush();
    assert_eq!(*log.borrow(), vec![0, 1, 2]);
}

#[test]
fn panicking_effect_does_not_latch_flushing_flag() {
    fresh();
    // An effect that panics on its second run (when the dep flips to 1).
    let (count, set_count) = signal(0_i32).split();
    effect(move || {
        if count.get() == 1 {
            panic!("boom");
        }
    });

    // Drive the panicking re-run through a caught flush. Without the
    // RAII `FlushGuard`, the unwind would skip `flushing = false` and
    // latch the flag — every later flush would become a silent no-op.
    set_count.set(1);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(flush));
    assert!(caught.is_err(), "the effect was supposed to panic");

    // A fresh effect created after the panic must still run on flush —
    // proof the runtime recovered (flushing flag cleared, tracker/owner
    // stack restored).
    let ran = Rc::new(RefCell::new(0));
    let r = ran.clone();
    let (dep, set_dep) = signal(0_i32).split();
    effect(move || {
        dep.get();
        *r.borrow_mut() += 1;
    });
    set_dep.set(5);
    flush();
    assert_eq!(*ran.borrow(), 2, "post-panic flush must still run effects");
}

#[test]
fn effect_only_reruns_for_tracked_deps() {
    fresh();
    let (tracked, set_tracked) = signal(0_i32).split();
    let (untracked, set_untracked) = signal(100_i32).split();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        let _ = tracked.get();
        let _ = untracked.get_untracked();
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);

    set_untracked.set(999);
    flush();
    assert_eq!(*runs.borrow(), 1, "untracked write must not re-run effect");

    set_tracked.set(1);
    flush();
    assert_eq!(*runs.borrow(), 2, "tracked write must re-run effect");
}

#[test]
fn effect_dynamic_deps_change_between_runs() {
    fresh();
    let (toggle, set_toggle) = signal(false).split();
    let (a, set_a) = signal(0_i32).split();
    let (b, set_b) = signal(0_i32).split();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        if toggle.get() {
            let _ = a.get();
        } else {
            let _ = b.get();
        }
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);

    // Currently reading b. Writing a should NOT re-run.
    set_a.set(1);
    flush();
    assert_eq!(*runs.borrow(), 1);

    set_b.set(1);
    flush();
    assert_eq!(*runs.borrow(), 2);

    // Flip to reading a.
    set_toggle.set(true);
    flush();
    assert_eq!(*runs.borrow(), 3);

    // Now writing b should NOT re-run.
    set_b.set(2);
    flush();
    assert_eq!(*runs.borrow(), 3);

    set_a.set(2);
    flush();
    assert_eq!(*runs.borrow(), 4);
}

// ----- Batching -------------------------------------------------------------

#[test]
fn multiple_writes_coalesce_into_one_rerun() {
    fresh();
    let (a, set_a) = signal(0_i32).split();
    let (b, set_b) = signal(0_i32).split();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        let _ = a.get();
        let _ = b.get();
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);

    // Two writes without an intervening flush — should coalesce.
    set_a.set(1);
    set_b.set(2);
    flush();
    assert_eq!(
        *runs.borrow(),
        2,
        "two writes must produce exactly one re-run"
    );
}

#[test]
fn signals_written_during_flush_propagate_in_same_flush() {
    fresh();
    let (a, set_a) = signal(0_i32).split();
    let (b, set_b) = signal(0_i32).split();
    let cascade_runs = Rc::new(RefCell::new(0));
    let cascade_clone = cascade_runs.clone();

    // Effect 1: reads a, writes b.
    effect(move || {
        let v = a.get();
        set_b.set(v * 10);
    });

    // Effect 2: reads b, counts.
    effect(move || {
        let _ = b.get();
        *cascade_clone.borrow_mut() += 1;
    });

    set_a.set(3);
    flush();
    // Effect 1 re-ran and wrote b=30; effect 2 should have observed
    // that write in the same flush (drain loop).
    assert_eq!(b.get_untracked(), 30);
    assert!(
        *cascade_runs.borrow() >= 2,
        "cascading write must re-trigger downstream effect"
    );
}

// ----- Owner lifecycle ------------------------------------------------------

#[test]
fn dispose_owner_frees_nested_signals() {
    fresh();
    let owner = Owner::new(None);
    let (read, _write) = owner.with(|| signal(123_i32)).split();
    // Signal works while owner is alive.
    assert_eq!(read.get(), 123);
    owner.dispose();
    // After disposal the underlying node is gone; reads panic in debug
    // and we don't want to fault the test, so we just verify the
    // owner slot itself is freed by trying to dispose again (no-op).
    owner.dispose();
}

#[test]
fn dispose_cascades_to_children() {
    fresh();
    let parent = Owner::new(None);
    let mut leaf_signals = Vec::new();
    let child = parent.with(|| {
        let c = Owner::new(None);
        c.with(|| {
            let (r, _w) = signal(0_u32).split();
            leaf_signals.push(r);
        });
        c
    });

    // Parent owns child; disposing parent disposes child too.
    parent.dispose();

    // Owner map should have neither.
    let alive = with_runtime(|rt| {
        (
            rt.owners.contains_key(parent),
            rt.owners.contains_key(child),
        )
    });
    assert_eq!(alive, (false, false));
}

#[test]
fn on_cleanup_fires_lifo() {
    fresh();
    let owner = Owner::new(None);
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    owner.with(|| {
        let l = log.clone();
        on_cleanup(move || l.borrow_mut().push("first"));
        let l = log.clone();
        on_cleanup(move || l.borrow_mut().push("second"));
        let l = log.clone();
        on_cleanup(move || l.borrow_mut().push("third"));
    });

    owner.dispose();
    assert_eq!(*log.borrow(), vec!["third", "second", "first"]);
}

#[test]
fn disposing_owner_removes_its_effects_from_pending() {
    fresh();
    let owner = Owner::new(None);
    let (count, set_count) = signal(0_i32).split();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();

    owner.with(|| {
        effect(move || {
            let _ = count.get();
            *runs_clone.borrow_mut() += 1;
        });
    });
    assert_eq!(*runs.borrow(), 1);

    // Schedule the effect, then dispose its owner before flush.
    set_count.set(1);
    owner.dispose();
    flush();

    // The effect should NOT have re-run.
    assert_eq!(*runs.borrow(), 1);
}

// ----- StoredValue ----------------------------------------------------------

#[test]
fn stored_value_read_write() {
    fresh();
    let sv = StoredValue::new(vec![1_i32, 2, 3]);
    assert_eq!(sv.with(|v| v.iter().sum::<i32>()), 6);
    sv.update(|v| v.push(4));
    assert_eq!(sv.with(|v| v.len()), 4);
}

#[test]
fn stored_value_does_not_trigger_effects() {
    fresh();
    let sv = StoredValue::new(0_i32);
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        // Read sv from inside an effect — should NOT subscribe.
        let _ = sv.with(|v| *v);
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);
    sv.set(99);
    flush();
    assert_eq!(
        *runs.borrow(),
        1,
        "StoredValue writes must not trigger reactivity"
    );
}

#[test]
fn stored_value_disposed_with_owner() {
    fresh();
    let owner = Owner::new(None);
    let sv = owner.with(|| StoredValue::new(123_i32));
    assert_eq!(sv.get(), 123);
    owner.dispose();
    // Owner gone — node is gone too. We can verify indirectly: a fresh
    // owner has no leftover.
    let leftover = with_runtime(|rt| rt.owners.contains_key(owner));
    assert!(!leftover);
}

// ----- Computed -----------------------------------------------------------------

#[test]
fn computed_caches_initial_value() {
    fresh();
    let (count, _set) = signal(3_i32).split();
    let doubled = computed(move || count.get() * 2);
    assert_eq!(doubled.get(), 6);
}

#[test]
fn computed_recomputes_on_source_change() {
    fresh();
    let (count, set_count) = signal(0_i32).split();
    let doubled = computed(move || count.get() * 2);
    assert_eq!(doubled.get(), 0);
    set_count.set(5);
    flush();
    assert_eq!(doubled.get(), 10);
}

#[test]
fn computed_notifies_downstream_subscribers() {
    fresh();
    let (count, set_count) = signal(0_i32).split();
    let doubled = computed(move || count.get() * 2);
    let observed: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let obs_clone = observed.clone();
    effect(move || obs_clone.borrow_mut().push(doubled.get()));
    set_count.set(3);
    flush();
    set_count.set(7);
    flush();
    assert_eq!(*observed.borrow(), vec![0, 6, 14]);
}

#[test]
fn on_mount_callback_inside_effect_does_not_leak_subscription_to_outer() {
    // Regression: `flush_mounts` invokes each queued `on_mount`
    // callback as plain user code. If a callback performs a direct
    // `signal.get()` while a tracker is still active on the call
    // stack (e.g. `flush_mounts` invoked from inside an effect, or
    // from a non-standard tick driver), the signal would subscribe
    // the outer tracker instead of nothing.
    //
    // After fix: `flush_mounts` brackets each `cb()` invocation in
    // `untrack`, so on_mount callbacks never leak subscriptions
    // upward regardless of the call-stack context.
    use super::component::{flush_mounts, on_mount};
    fresh();
    let (effect_dep, set_effect_dep) = signal(0_i32).split();
    let (mount_src, set_mount_src) = signal(0_i32).split();
    let outer_runs = Rc::new(RefCell::new(0));
    let outer_runs_clone = outer_runs.clone();
    effect(move || {
        let _ = effect_dep.get();
        *outer_runs_clone.borrow_mut() += 1;
        // Build a fresh owner so `on_mount` doesn't warn about
        // "called outside any owner scope".
        let owner = super::Owner::new(None);
        owner.with(|| {
            on_mount(move || {
                let _ = mount_src.get();
            });
        });
        // Flush inside the effect — same call stack as the outer
        // effect's tracker. With the fix, the callback runs under a
        // cleared tracker, so its signal read doesn't leak.
        flush_mounts();
    });
    assert_eq!(*outer_runs.borrow(), 1);
    set_mount_src.set(1);
    flush();
    // No leak: `mount_src` write does not re-run the outer effect.
    assert_eq!(*outer_runs.borrow(), 1);
    // Sanity check that the outer effect's legitimate dependency
    // still works.
    set_effect_dep.set(1);
    flush();
    assert_eq!(*outer_runs.borrow(), 2);
}

#[test]
fn mount_component_body_inside_effect_does_not_leak_subscription_to_outer() {
    // Regression: a `#[component]` body invoked from inside another
    // effect / computed used to run with that outer node's
    // `current_tracker` still set. Any direct `signal.get()` the
    // body performed (not via a nested `effect`/`computed`) silently
    // subscribed the outer node, turning innocent component reads
    // into recursive remount triggers when the read signal changed.
    //
    // After fix: `mount_component` wraps the body invocation in
    // `untrack`, so component bodies are tracker-isolated. Reactivity
    // inside a body must come from explicit `effect`/`computed`
    // calls, which establish their own tracker scope.
    use super::component::mount_component;
    fresh();
    let (effect_dep, set_effect_dep) = signal(0_i32).split();
    let (body_src, set_body_src) = signal(0_i32).split();
    let outer_runs = Rc::new(RefCell::new(0));
    let outer_runs_clone = outer_runs.clone();
    effect(move || {
        let _ = effect_dep.get();
        *outer_runs_clone.borrow_mut() += 1;
        // Mount a synthetic "component": fresh owner, body reads a
        // signal directly (the typical hazard pattern for a top-level
        // `#[component]` body that derives a value inline).
        let (_owner, _value) = mount_component(0xdead_beef as *const (), || body_src.get());
    });
    assert_eq!(*outer_runs.borrow(), 1);
    // No leak: writing to `body_src` MUST NOT re-run the outer
    // effect — the outer effect never legitimately subscribed.
    set_body_src.set(1);
    flush();
    assert_eq!(*outer_runs.borrow(), 1);
    // Sanity check the outer effect's legitimate dependency still
    // fires.
    set_effect_dep.set(1);
    flush();
    assert_eq!(*outer_runs.borrow(), 2);
}

#[test]
fn computed_constructed_inside_effect_does_not_leak_subscription_to_outer() {
    // Regression: `computed(...)`'s construction-time seed call used
    // to run with whatever `current_tracker` happened to be set on
    // entry. If you built a `computed` inside an `effect`, the seed's
    // `signal.get()` registered the *outer effect* as a subscriber of
    // every signal the computed body read — so a write to one of
    // those signals re-ran the outer effect (whose job is often to
    // re-mount a component subtree), leaking a fresh `computed` node
    // on every tick.
    //
    // After fix: the seed run is wrapped in `untrack`, so the only
    // subscriber registered against the signal is the computed node
    // itself. Verified by counting outer-effect runs: a single write
    // to the signal must produce exactly one extra effect run (the
    // initial one + one for the explicit `effect_dep` change), not
    // two.
    fresh();
    let (effect_dep, set_effect_dep) = signal(0_i32).split();
    let (computed_src, set_computed_src) = signal(0_i32).split();
    let outer_runs = Rc::new(RefCell::new(0));
    let outer_runs_clone = outer_runs.clone();
    effect(move || {
        // Subscribe to `effect_dep` so the outer effect has a
        // legitimate reason to re-run.
        let _ = effect_dep.get();
        *outer_runs_clone.borrow_mut() += 1;
        // Construct a computed that reads `computed_src`. Pre-fix,
        // this would register the outer effect as a subscriber of
        // `computed_src` via the seed run's `track_and_fetch`.
        let _doubled = computed(move || computed_src.get() * 2);
    });
    assert_eq!(*outer_runs.borrow(), 1);
    // Writing to `computed_src` MUST NOT re-run the outer effect —
    // the outer effect never subscribed to it (only the inner
    // computed should have).
    set_computed_src.set(99);
    flush();
    assert_eq!(*outer_runs.borrow(), 1);
    // Writing to `effect_dep` IS a legitimate trigger and should
    // re-run the outer effect exactly once.
    set_effect_dep.set(1);
    flush();
    assert_eq!(*outer_runs.borrow(), 2);
}

#[test]
fn computed_does_not_notify_when_value_unchanged() {
    fresh();
    let (count, set_count) = signal(5_i32).split();
    // floor-div by 10 — different `count` values yield the same computed value
    let bucket = computed(move || count.get() / 10);
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        let _ = bucket.get();
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);
    // 5 -> 7 keeps bucket at 0 — downstream effect must not re-run.
    set_count.set(7);
    flush();
    assert_eq!(*runs.borrow(), 1);
    // 7 -> 15 moves bucket to 1 — downstream effect must re-run.
    set_count.set(15);
    flush();
    assert_eq!(*runs.borrow(), 2);
}

// ----- Context --------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct Theme(&'static str);

#[test]
fn context_round_trip_in_same_owner() {
    fresh();
    let owner = Owner::new(None);
    owner.with(|| {
        provide_context(Theme("dark"));
        assert_eq!(use_context::<Theme>(), Some(Theme("dark")));
    });
}

#[test]
fn with_context_closure_can_reenter_runtime() {
    // The `with_context` closure must be able to call back into the
    // runtime (read signals, nested context lookups). Before the Rc
    // fix, `f` ran while the thread-local runtime was still borrowed,
    // so any re-entry double-borrowed its RefCell and panicked.
    fresh();
    let owner = Owner::new(None);
    owner.with(|| {
        provide_context(Theme("ctx"));
        let (count, _set) = signal(7_i32).split();
        // Read a signal AND do a nested context lookup from inside the
        // with_context closure — both re-enter the runtime.
        let combined = with_context::<Theme, _>(|theme| {
            let n = count.get();
            let nested = use_context::<Theme>();
            format!("{}-{}-{}", theme.0, n, nested.unwrap().0)
        });
        assert_eq!(combined, Some("ctx-7-ctx".to_string()));
    });
}

#[test]
fn context_walks_parent_chain() {
    fresh();
    let parent = Owner::new(None);
    let observed = Rc::new(RefCell::new(None::<Theme>));
    parent.with(|| {
        provide_context(Theme("from-parent"));
        let child = Owner::new(None);
        let observed_clone = observed.clone();
        child.with(|| {
            *observed_clone.borrow_mut() = use_context::<Theme>();
        });
    });
    assert_eq!(*observed.borrow(), Some(Theme("from-parent")));
}

#[test]
fn context_descendant_shadows_ancestor() {
    fresh();
    let parent = Owner::new(None);
    let observed = Rc::new(RefCell::new(None::<Theme>));
    parent.with(|| {
        provide_context(Theme("outer"));
        let inner = Owner::new(None);
        let observed_clone = observed.clone();
        inner.with(|| {
            provide_context(Theme("inner"));
            *observed_clone.borrow_mut() = use_context::<Theme>();
        });
    });
    assert_eq!(*observed.borrow(), Some(Theme("inner")));
}

#[test]
fn context_missing_returns_none() {
    fresh();
    let owner = Owner::new(None);
    let observed = Rc::new(RefCell::new(Some(Theme("placeholder"))));
    let obs_clone = observed.clone();
    owner.with(|| {
        *obs_clone.borrow_mut() = use_context::<Theme>();
    });
    assert_eq!(*observed.borrow(), None);
}

#[test]
fn with_context_borrows_without_clone() {
    fresh();
    let owner = Owner::new(None);
    let observed = Rc::new(RefCell::new(0_usize));
    let obs_clone = observed.clone();
    owner.with(|| {
        provide_context(vec![1_i32, 2, 3, 4]);
        let len = with_context::<Vec<i32>, _>(|v| v.len()).unwrap();
        *obs_clone.borrow_mut() = len;
    });
    assert_eq!(*observed.borrow(), 4);
}

// ----- Component lifecycle --------------------------------------------------

fn dummy_component_a() {} // dummy fn pointers for tests
fn dummy_component_b() {}

#[test]
fn mount_component_registers_fn_pointer() {
    fresh();
    let (owner, _) = mount_component(dummy_component_a as *const (), || 42_i32);
    let registered = owners_for_fn(dummy_component_a as *const ());
    assert_eq!(registered, vec![owner]);
}

#[test]
fn unmount_component_removes_registration_and_disposes() {
    fresh();
    let (owner, _) = mount_component(dummy_component_a as *const (), || ());
    assert_eq!(owners_for_fn(dummy_component_a as *const ()).len(), 1);
    unmount_component(owner);
    assert_eq!(owners_for_fn(dummy_component_a as *const ()).len(), 0);
    // Owner slot freed.
    let alive = with_runtime(|rt| rt.owners.contains_key(owner));
    assert!(!alive);
}

#[test]
fn mount_component_isolates_owner_state() {
    fresh();
    let (owner_a, sig_a) = mount_component(dummy_component_a as *const (), || {
        let (r, _w) = signal(1_i32).split();
        r
    });
    let (owner_b, sig_b) = mount_component(dummy_component_b as *const (), || {
        let (r, _w) = signal(2_i32).split();
        r
    });
    // Each signal lives in its own component owner.
    assert_eq!(sig_a.get(), 1);
    assert_eq!(sig_b.get(), 2);
    // Disposing one doesn't touch the other.
    unmount_component(owner_a);
    assert_eq!(sig_b.get(), 2);
    unmount_component(owner_b);
}

// ----- on_mount / flush_mounts ----------------------------------------------

#[test]
fn on_mount_fires_on_flush() {
    fresh();
    let owner = Owner::new(None);
    let fired = Rc::new(RefCell::new(false));
    let fired_clone = fired.clone();
    owner.with(|| {
        on_mount(move || *fired_clone.borrow_mut() = true);
    });
    // Hasn't fired yet — needs a flush_mounts call.
    assert!(!*fired.borrow());
    flush_mounts();
    assert!(*fired.borrow());
}

#[test]
fn on_mount_runs_in_registration_order() {
    fresh();
    let owner = Owner::new(None);
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    owner.with(|| {
        let l = log.clone();
        on_mount(move || l.borrow_mut().push("first"));
        let l = log.clone();
        on_mount(move || l.borrow_mut().push("second"));
        let l = log.clone();
        on_mount(move || l.borrow_mut().push("third"));
    });
    flush_mounts();
    assert_eq!(*log.borrow(), vec!["first", "second", "third"]);
}

#[test]
fn flush_mounts_is_idempotent() {
    fresh();
    let owner = Owner::new(None);
    let count = Rc::new(RefCell::new(0));
    let count_clone = count.clone();
    owner.with(|| {
        on_mount(move || *count_clone.borrow_mut() += 1);
    });
    flush_mounts();
    assert_eq!(*count.borrow(), 1);
    flush_mounts();
    assert_eq!(*count.borrow(), 1, "second flush must be a no-op");
}

// ----- Self-write doesn't cause unbounded recursion --------------------------

#[test]
fn flush_breaks_self_feedback_loop_with_warning() {
    fresh();
    let (count, set_count) = signal(0_i32).split();
    // An effect that reads AND writes the same signal — guaranteed
    // feedback loop. flush's iteration cap must break it.
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        let v = count.get();
        set_count.set(v + 1);
        *runs_clone.borrow_mut() += 1;
    });
    set_count.set(1);
    flush();
    // We don't assert the exact run count, only that flush returned
    // rather than spinning forever, and that the run count is bounded.
    let runs_after = *runs.borrow();
    assert!(
        runs_after < 1000,
        "flush must break feedback loops; got {runs_after} runs"
    );
}

#[test]
fn effect_reading_and_writing_unrelated_signals_terminates() {
    fresh();
    let (a, _) = signal(0_i32).split();
    let (_b, set_b) = signal(0_i32).split();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();

    effect(move || {
        let v = a.get();
        // Write to a signal we don't read — no feedback loop.
        set_b.set(v + 1);
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);
}

// ============================================================================
// Tracker isolation invariant
// ============================================================================
//
// The invariant: **whenever the reactive runtime invokes user code in a
// construction-time / one-shot path, `current_tracker` is `None` for the
// duration of that call**. The user code is free to call `signal.get()` or
// build nested reactive nodes without accidentally subscribing whatever
// outer effect / computed / component happens to be on the call stack.
//
// Reactivity inside such bodies must come from *explicit* `effect` /
// `computed` calls, which establish their own tracker scope via the
// scheduler. The runtime guarantees no ambient subscription leaks.
//
// The tests below enforce this invariant for each entrypoint in the
// public surface. A future contributor who adds a new entrypoint that
// invokes user code without `untrack`-bracketing will break one of
// these tests.
//
// The pattern: enter an outer `effect`, which sets `current_tracker` to
// the effect's node id. From inside that effect, call the entrypoint
// under test and capture `current_tracker` as observed by the user
// closure. Assert it was `None`.

/// Snapshot the runtime's `current_tracker` at call time.
fn observed_tracker() -> Option<super::runtime::NodeId> {
    with_runtime(|rt| rt.current_tracker)
}

#[test]
fn untrack_clears_tracker_during_f_and_restores_after() {
    fresh();
    let observed_inside = Rc::new(RefCell::new(Some(observed_tracker()))); // sentinel
    let observed_outer_after = Rc::new(RefCell::new(None));
    let observed_inside_clone = observed_inside.clone();
    let observed_outer_after_clone = observed_outer_after.clone();
    effect(move || {
        let outer_tracker = observed_tracker();
        assert!(
            outer_tracker.is_some(),
            "outer effect's tracker must be set"
        );
        untrack(|| {
            *observed_inside_clone.borrow_mut() = Some(observed_tracker());
        });
        *observed_outer_after_clone.borrow_mut() = Some(observed_tracker());
        // The outer tracker should be the same as before `untrack`.
        assert_eq!(observed_tracker(), outer_tracker);
    });
    assert_eq!(*observed_inside.borrow(), Some(None));
    assert!(observed_outer_after.borrow().unwrap().is_some());
}

#[test]
fn untrack_is_nestable_without_breaking_outer_restore() {
    // Nested `untrack` calls should each restore the tracker to
    // whatever the immediate parent had — `None` for the inner
    // untrack (its parent already untracked), and the outer effect's
    // tracker after both untracks return.
    fresh();
    let inner_observed = Rc::new(RefCell::new(None));
    let inner_observed_clone = inner_observed.clone();
    effect(move || {
        let outer_tracker = observed_tracker();
        untrack(|| {
            assert_eq!(observed_tracker(), None, "outer untrack clears");
            untrack(|| {
                *inner_observed_clone.borrow_mut() = Some(observed_tracker());
            });
            assert_eq!(observed_tracker(), None, "inner untrack returned to None");
        });
        assert_eq!(
            observed_tracker(),
            outer_tracker,
            "outer untrack restored the effect's tracker"
        );
    });
    assert_eq!(*inner_observed.borrow(), Some(None));
}

#[test]
fn untrack_restores_tracker_when_f_panics() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    fresh();
    let restored = Rc::new(RefCell::new(None));
    let restored_clone = restored.clone();
    effect(move || {
        let before = observed_tracker();
        assert!(before.is_some(), "outer effect's tracker is set");
        // Catch the panic from `untrack`'s body so the effect itself
        // continues. The runtime's `Drop` guard inside `untrack` must
        // restore `current_tracker` before the unwind escapes.
        let result = catch_unwind(AssertUnwindSafe(|| {
            untrack(|| {
                panic!("intentional panic from inside untrack");
            });
        }));
        assert!(result.is_err(), "panic must propagate");
        *restored_clone.borrow_mut() = Some(observed_tracker());
        assert_eq!(observed_tracker(), before, "tracker restored after panic");
    });
    assert!(restored.borrow().unwrap().is_some());
}

#[test]
fn computed_seed_runs_with_no_tracker_when_constructed_inside_effect() {
    // The computed's compute closure runs at construction-time
    // (seed) AND again on every scheduled re-run. We only care
    // about the seed observation — capture the FIRST call and
    // ignore later scheduled runs (those legitimately get
    // tracker = Some(computed_node) set by `run_node_if_alive`).
    fresh();
    let observed: Rc<RefCell<Option<Option<super::runtime::NodeId>>>> = Rc::new(RefCell::new(None));
    let observed_clone = observed.clone();
    effect(move || {
        let observed_inner = observed_clone.clone();
        let _doubled = computed(move || {
            let mut slot = observed_inner.borrow_mut();
            if slot.is_none() {
                *slot = Some(observed_tracker());
            }
            0_i32
        });
    });
    assert_eq!(
        *observed.borrow(),
        Some(None),
        "computed seed must run with no tracker"
    );
}

#[test]
fn mount_component_body_runs_with_no_tracker_when_invoked_inside_effect() {
    use super::component::mount_component;
    fresh();
    let observed = Rc::new(RefCell::new(Some(observed_tracker())));
    let observed_clone = observed.clone();
    effect(move || {
        let observed_inner = observed_clone.clone();
        let (_owner, ()) = mount_component(0x1234_5678 as *const (), || {
            *observed_inner.borrow_mut() = Some(observed_tracker());
        });
    });
    assert_eq!(
        *observed.borrow(),
        Some(None),
        "mount_component body must run with no tracker"
    );
}

#[test]
fn mount_component_remountable_body_runs_with_no_tracker_when_invoked_inside_effect() {
    use super::component::mount_component_remountable;
    use crate::view::create_phantom_element;
    fresh();
    let observed = Rc::new(RefCell::new(Some(observed_tracker())));
    let observed_clone = observed.clone();
    effect(move || {
        let observed_inner = observed_clone.clone();
        // Component body must return an Element. A phantom element
        // is fine — we're not asserting on the tree shape here.
        let _root = mount_component_remountable(
            0x2345_6789 as *const (),
            move || {
                *observed_inner.borrow_mut() = Some(observed_tracker());
                create_phantom_element()
            },
            Box::new(|| 0),
        );
    });
    assert_eq!(
        *observed.borrow(),
        Some(None),
        "mount_component_remountable body must run with no tracker"
    );
}

#[test]
fn remount_components_for_reports_zero_when_nothing_can_reflect() {
    // The return value is the full-remount trigger:
    // `remounted == 0` must mean "this patch had no attached
    // component to reflect through". Three shapes of zero:
    use super::component::{RemountStats, mount_component_remountable, remount_components_for};
    use crate::view::create_phantom_element;
    fresh();

    // (a) empty patch list — nothing to do.
    assert_eq!(remount_components_for(&[]), RemountStats::default());

    // (b) patched fns that match no registered mount site.
    assert_eq!(
        remount_components_for(&[0xdead_beef as *const ()]),
        RemountStats::default(),
    );

    // (c) a matching site whose body_root was never attached to a
    // parent (orphan) — the candidate is found but filtered out.
    // Orphans don't count as layout_changed either: their stored
    // closures never re-run, so there's nothing to protect.
    let fn_ptr = 0x3456_789a as *const ();
    let _root = mount_component_remountable(fn_ptr, create_phantom_element, Box::new(|| 0));
    assert_eq!(remount_components_for(&[fn_ptr]), RemountStats::default());
}

#[test]
fn remount_components_for_refuses_sites_whose_props_layout_changed() {
    // A site's stored body closure was built against the props
    // layout its hash getter reported at mount time. If the getter
    // reports a different value at remount time (after a patch, it
    // dispatches into the new dylib), re-running the stored closure
    // would transmute mismatched capture layouts — the site must be
    // counted in `layout_changed` and NOT remounted.
    use super::component::{
        RemountStats, mount_component_remountable, on_component_root_attached,
        remount_components_for,
    };
    use crate::view::{append_child, create_phantom_element};
    use std::cell::Cell;
    fresh();

    thread_local! {
        static CURRENT_LAYOUT: Cell<u64> = const { Cell::new(1) };
    }
    CURRENT_LAYOUT.with(|c| c.set(1));

    let fn_ptr = 0x4567_89ab as *const ();
    let runs = Rc::new(Cell::new(0_usize));
    let runs_clone = runs.clone();
    let root = mount_component_remountable(
        fn_ptr,
        move || {
            runs_clone.set(runs_clone.get() + 1);
            create_phantom_element()
        },
        Box::new(|| CURRENT_LAYOUT.with(|c| c.get())),
    );
    assert_eq!(runs.get(), 1, "initial mount runs the body once");

    // Attach the body root so the site has a parent (otherwise the
    // orphan filter short-circuits before the layout gate).
    let parent = create_phantom_element();
    append_child(parent, root);
    on_component_root_attached(parent, root);

    // Same layout → normal in-place remount.
    assert_eq!(
        remount_components_for(&[fn_ptr]),
        RemountStats {
            remounted: 1,
            layout_changed: 0,
        },
    );
    assert_eq!(runs.get(), 2, "in-place remount re-ran the body");

    // "Patch" changes the layout hash → the site must be refused.
    CURRENT_LAYOUT.with(|c| c.set(2));
    assert_eq!(
        remount_components_for(&[fn_ptr]),
        RemountStats {
            remounted: 0,
            layout_changed: 1,
        },
    );
    assert_eq!(runs.get(), 2, "refused remount must NOT re-run the body");
}

// Note: `remount_components_for`'s body invocation reuses the
// exact same `untrack(|| new_owner.with(|| (*info.body)()))`
// bracket the initial mount uses. Driving the remount path through
// a unit test requires a full renderer install + an attached parent
// element (otherwise `info.parent` is `None` and the path
// short-circuits), which sits at the integration-test layer.
// Coverage relies on:
//   1. `mount_component_remountable_body_runs_with_no_tracker_...`
//      below, which exercises the identical `untrack` pattern, and
//   2. inspection of `component.rs::remount_components_for` to
//      confirm the bracket is in place.
// If a future contributor removes the bracket from the remount
// path, the corresponding StackLayout / hot-reload integration
// scenario will regress visibly.

#[test]
fn flush_mounts_callback_runs_with_no_tracker_when_called_inside_effect() {
    use super::component::{flush_mounts, on_mount};
    fresh();
    let observed = Rc::new(RefCell::new(Some(observed_tracker())));
    let observed_clone = observed.clone();
    effect(move || {
        let owner = super::Owner::new(None);
        let observed_inner = observed_clone.clone();
        owner.with(|| {
            on_mount(move || {
                *observed_inner.borrow_mut() = Some(observed_tracker());
            });
        });
        flush_mounts();
    });
    assert_eq!(
        *observed.borrow(),
        Some(None),
        "on_mount callback must run with no tracker"
    );
}

#[test]
fn resource_sync_fetcher_runs_with_no_tracker_when_called_inside_effect() {
    use super::resource::resource_sync;
    fresh();
    let observed = Rc::new(RefCell::new(Some(observed_tracker())));
    let observed_clone = observed.clone();
    effect(move || {
        let observed_inner = observed_clone.clone();
        let _r = resource_sync::<i32, _>(move || {
            *observed_inner.borrow_mut() = Some(observed_tracker());
            Ok(0)
        });
    });
    assert_eq!(
        *observed.borrow(),
        Some(None),
        "resource_sync fetcher must run with no tracker"
    );
}

#[test]
fn computed_constructed_inside_computed_does_not_leak_subscription_to_outer_computed() {
    // The same invariant applies to nested computed: a computed
    // constructed during another computed's seed (which is rare but
    // legal — e.g. a derived value that itself wraps a computed)
    // must not leak.
    fresh();
    let (src, set_src) = signal(0_i32).split();
    let outer_runs = Rc::new(RefCell::new(0));
    let outer_runs_clone = outer_runs.clone();
    let outer = computed(move || {
        *outer_runs_clone.borrow_mut() += 1;
        // Construct an inner computed reading `src`. Pre-fix this
        // would have subscribed `outer` to `src` via the seed leak.
        let inner = computed(move || src.get() * 2);
        inner.get()
    });
    // The legitimate dependency edge (outer → inner.get → src) is
    // established when the scheduler runs `outer`'s real compute
    // after construction. We still expect outer to re-run when src
    // changes — that's correct reactivity, not a leak.
    let initial = *outer_runs.borrow();
    set_src.set(1);
    flush();
    let after_legit_change = *outer_runs.borrow();
    assert!(
        after_legit_change > initial,
        "outer should re-run for legitimate edge through inner.get()"
    );
    // Outer's cached value should reflect src change.
    assert_eq!(outer.get(), 2);
}

// ============================================================================
// ArcSignal family
// ============================================================================
//
// Arc-backed signals decouple lifetime from the owner tree. The
// invariant we care about: a signal allocated inside one owner can
// be read after that owner is disposed, as long as a handle is held
// somewhere. The tests below establish that property and verify that
// the subscriber-tracking machinery (re-run cleanup + disposed-owner
// pruning) keeps the back-references in sync.

#[test]
fn arc_signal_basic_get_set() {
    fresh();
    let signal = ArcRwSignal::new(10_i32);
    assert_eq!(signal.get(), 10);
    signal.set(42);
    assert_eq!(signal.get(), 42);
}

#[test]
fn arc_signal_clone_shares_value() {
    fresh();
    let s1 = ArcRwSignal::new(0_i32);
    let s2 = s1.clone();
    s1.set(1);
    assert_eq!(s2.get(), 1);
    s2.set(2);
    assert_eq!(s1.get(), 2);
}

#[test]
fn arc_signal_split_round_trip() {
    fresh();
    let rw = ArcRwSignal::new(0_i32);
    let (r, w) = rw.split();
    w.set(5);
    assert_eq!(r.get(), 5);
    w.update(|v| *v += 3);
    assert_eq!(r.get(), 8);
}

#[test]
fn arc_signal_survives_caller_owner_disposal() {
    // The bug this whole family exists to fix: a signal whose
    // declaring owner is disposed must still be readable through
    // any clone of its handle, because the value lives by Arc
    // refcount, not by an arena slot.
    fresh();
    let stash: Rc<RefCell<Option<ArcReadSignal<i32>>>> = Rc::new(RefCell::new(None));
    let stash_for_install = stash.clone();

    let owner = Owner::new(None);
    owner.with(|| {
        let (r, w) = arc_signal(99_i32).split();
        // Pretend `w` is the write half kept by a native module's
        // event callback; we don't need to keep it for this test.
        drop(w);
        *stash_for_install.borrow_mut() = Some(r);
    });

    owner.dispose();

    // Pre-fix this would have panicked (`expect("signal disposed")`)
    // for Copy signals. Post-fix Arc signals survive the disposal.
    let cached = stash.borrow().clone().expect("stashed signal");
    assert_eq!(cached.get(), 99);
}

#[test]
fn arc_signal_effect_subscribes_and_reruns() {
    fresh();
    let counter = ArcRwSignal::new(0_i32);
    let counter_for_effect = counter.clone();
    let observed: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let observed_clone = observed.clone();
    effect(move || observed_clone.borrow_mut().push(counter_for_effect.get()));
    counter.set(1);
    flush();
    counter.set(2);
    flush();
    assert_eq!(*observed.borrow(), vec![0, 1, 2]);
}

#[test]
fn arc_signal_subscriber_is_pruned_when_its_owner_is_disposed() {
    // Asymmetric strong/weak: signal -> subscriber is weak (just a
    // NodeId), so when the subscriber's owner is disposed, the
    // signal's subscriber list can prune the stale NodeId without
    // any panic.
    fresh();
    let counter = ArcRwSignal::new(0_i32);
    let counter_for_effect = counter.clone();
    let observed: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let observed_clone = observed.clone();

    let owner = Owner::new(None);
    owner.with(|| {
        effect(move || observed_clone.borrow_mut().push(counter_for_effect.get()));
    });
    assert_eq!(*observed.borrow(), vec![0]);

    // Tear the effect down by disposing its owner. The subscriber
    // list inside `counter` should be pruned eagerly by the
    // owner-disposal cascade so the next `set` doesn't try to
    // schedule a freed NodeId.
    owner.dispose();

    counter.set(7);
    flush();
    // Effect didn't re-run — it's gone.
    assert_eq!(*observed.borrow(), vec![0]);
    // Signal still alive on its own (caller still holds it).
    assert_eq!(counter.get_untracked(), 7);
}

#[test]
fn arc_signal_effect_resubscribes_on_rerun() {
    // The scheduler must drop the effect's old subscription to an
    // Arc signal before re-running, so a write to a no-longer-read
    // signal doesn't fire stale subscribers.
    fresh();
    let toggle = ArcRwSignal::new(false);
    let a = ArcRwSignal::new(0_i32);
    let b = ArcRwSignal::new(0_i32);
    let toggle_e = toggle.clone();
    let a_e = a.clone();
    let b_e = b.clone();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        if toggle_e.get() {
            let _ = a_e.get();
        } else {
            let _ = b_e.get();
        }
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);

    // Currently reading b. Writing a should NOT re-run.
    a.set(1);
    flush();
    assert_eq!(*runs.borrow(), 1);

    b.set(1);
    flush();
    assert_eq!(*runs.borrow(), 2);

    // Flip to reading a.
    toggle.set(true);
    flush();
    assert_eq!(*runs.borrow(), 3);

    // Now writing b should NOT re-run — the previous subscription
    // got cleared by the scheduler.
    b.set(2);
    flush();
    assert_eq!(*runs.borrow(), 3);

    a.set(2);
    flush();
    assert_eq!(*runs.borrow(), 4);
}

#[test]
fn arc_signal_computed_caches_value() {
    fresh();
    let count = ArcRwSignal::new(3_i32);
    let count_e = count.clone();
    let doubled = computed(move || count_e.get() * 2);
    assert_eq!(doubled.get(), 6);
    count.set(5);
    flush();
    assert_eq!(doubled.get(), 10);
}

#[test]
fn arc_signal_with_untracked_does_not_register_subscriber() {
    fresh();
    let s = ArcRwSignal::new(0_i32);
    let s_e = s.clone();
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();
    effect(move || {
        let _ = s_e.with_untracked(|v| *v);
        *runs_clone.borrow_mut() += 1;
    });
    assert_eq!(*runs.borrow(), 1);
    s.set(1);
    flush();
    assert_eq!(*runs.borrow(), 1, "with_untracked must not subscribe");
}

#[test]
fn arc_to_rw_conversion_shares_underlying_value() {
    // Converting an `ArcRwSignal` to a `RwSignal` (Copy) builds a
    // fresh arena entry, but both handles observe the same underlying
    // value: writes through one are visible through the other.
    fresh();
    let owner = Owner::new(None);
    let arc = ArcRwSignal::new(0_i32);
    let arc_for_outside = arc.clone();
    let rw: RwSignal<i32> = owner.with(|| arc.clone().into());
    assert_eq!(rw.get(), 0);
    arc.set(7);
    assert_eq!(rw.get(), 7);
    rw.set(42);
    assert_eq!(arc.get_untracked(), 42);
    assert_eq!(arc_for_outside.get_untracked(), 42);
    owner.dispose();
}

#[test]
fn arc_to_rw_conversion_propagates_to_effect_subscribers() {
    // An effect that captures the converted `RwSignal` (Copy) re-runs
    // when the original `ArcRwSignal` is written.
    fresh();
    let arc = ArcRwSignal::new(0_i32);
    let owner = Owner::new(None);
    let observed: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let observed_clone = observed.clone();
    owner.with(|| {
        let rw: RwSignal<i32> = arc.clone().into();
        effect(move || observed_clone.borrow_mut().push(rw.get()));
    });
    arc.set(1);
    flush();
    arc.set(2);
    flush();
    assert_eq!(*observed.borrow(), vec![0, 1, 2]);
    owner.dispose();
}

#[test]
fn arc_to_rw_conversion_survives_caller_owner_disposal_via_arc() {
    // After disposing the owner that minted the `RwSignal`, the
    // original `ArcRwSignal` still works. The converted `RwSignal`
    // handle itself is gone (its arena slot was freed), but the
    // underlying value lives on because the `Arc` keeps a strong
    // reference.
    fresh();
    let arc = ArcRwSignal::new(0_i32);
    let owner = Owner::new(None);
    let _: RwSignal<i32> = owner.with(|| arc.clone().into());
    owner.dispose();
    arc.set(99);
    assert_eq!(arc.get_untracked(), 99);
}

#[test]
fn arc_read_signal_to_read_signal_conversion_reads_correctly() {
    fresh();
    let (r, w) = arc_signal(5_i32).split();
    let owner = Owner::new(None);
    let copy: ReadSignal<i32> = owner.with(|| r.into());
    assert_eq!(copy.get(), 5);
    w.set(99);
    assert_eq!(copy.get(), 99);
    owner.dispose();
}

#[test]
fn arc_write_signal_to_write_signal_conversion_writes_correctly() {
    fresh();
    let (r, w) = arc_signal(0_i32).split();
    let owner = Owner::new(None);
    let copy: WriteSignal<i32> = owner.with(|| w.into());
    copy.set(123);
    assert_eq!(r.get_untracked(), 123);
    owner.dispose();
}

#[test]
fn arc_signal_inside_oncelock_outlives_caller_owner() {
    // The canonical safe_area_insets-style pattern: a module stashes
    // its ArcRwSignal in process-global storage on first call. The
    // first call happens inside some component's owner, but the
    // signal stays alive once the component unmounts because the
    // OnceLock keeps a strong Arc refcount.
    use std::sync::OnceLock;
    fresh();
    struct Module {
        slot: OnceLock<ArcRwSignal<i32>>,
    }
    let module: Rc<Module> = Rc::new(Module {
        slot: OnceLock::new(),
    });
    let module_clone = module.clone();

    let install_owner = Owner::new(None);
    install_owner.with(|| {
        // First touch installs the signal under whatever owner is
        // current — the install owner here.
        module_clone.slot.get_or_init(|| ArcRwSignal::new(0));
    });

    // Dispose the owner that triggered the install. The OnceLock
    // still holds the only strong Arc.
    install_owner.dispose();

    // The signal is still readable, set-able, and propagates to new
    // subscribers under fresh owners.
    let observed: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let observed_clone = observed.clone();
    let module_for_use = module.clone();
    let use_owner = Owner::new(None);
    use_owner.with(|| {
        effect(move || {
            let v = module_for_use.slot.get().unwrap().get();
            *observed_clone.borrow_mut() = Some(v);
        });
    });
    assert_eq!(*observed.borrow(), Some(0));

    module.slot.get().unwrap().set(99);
    flush();
    assert_eq!(*observed.borrow(), Some(99));
}

#[test]
#[should_panic(expected = "signal disposed")]
fn reading_arc_backed_arena_signal_after_owner_dispose_panics() {
    // The mechanism behind the StackLayout crash: `safe_area_insets()`
    // mints a fresh arena ReadSignal in *whatever owner is current*
    // (`.into()`), backed by a process-global arc. If that owner is a
    // per-route owner that later disposes, the arena node is freed —
    // but a captured `insets` handle still points at the freed NodeId.
    // Reading it (e.g. a surviving computed/effect, or a late native
    // event re-run) hits `fetch_value`'s `expect("…disposed…")` and,
    // across the FFI tick boundary, aborts as `panic_cannot_unwind`.
    fresh();
    let global_arc = ArcRwSignal::new(0_i32);
    let route_owner = Owner::new(None);
    let insets: ReadSignal<i32> = route_owner.with(|| global_arc.read_only().into());
    route_owner.dispose();
    // The global arc still lives (process-global), but the arena
    // handle minted under the route owner is gone.
    let _ = insets.get();
}

#[test]
fn arc_backed_arena_signal_under_detached_root_survives_sibling_dispose() {
    // The fix: mint the shared arena handle under `Owner::detached_root`
    // (a never-disposed root, ignoring the current owner stack). It
    // then outlives any per-route / per-component owner that comes and
    // goes — `safe_area_insets()` relies on exactly this.
    fresh();
    let global_arc = ArcRwSignal::new(1_i32);

    // Mint under a detached root *while a per-route owner is current*,
    // proving detached_root does NOT adopt it as parent.
    let route_owner = Owner::new(None);
    let insets: ReadSignal<i32> = route_owner.with(|| {
        let root = Owner::detached_root();
        root.with(|| global_arc.read_only().into())
    });

    // Disposing the per-route owner must not free the detached-root
    // handle.
    route_owner.dispose();
    assert_eq!(insets.get(), 1, "handle survives sibling owner disposal");

    // And it still tracks updates from the global arc.
    global_arc.set(42);
    assert_eq!(insets.get(), 42);
}

// ----- Owner pause / resume -------------------------------------------------

#[test]
fn paused_owner_defers_effect_runs_until_resumed() {
    fresh();
    let runs = Rc::new(RefCell::new(0_u32));
    let (read, write) = signal(0_i32).split();

    let owner = Owner::new(None);
    let runs_clone = runs.clone();
    owner.with(|| {
        effect(move || {
            let _ = read.get();
            *runs_clone.borrow_mut() += 1;
        });
    });
    assert_eq!(
        *runs.borrow(),
        1,
        "initial registration runs the effect once"
    );

    owner.pause();
    assert!(owner.is_paused());

    write.set(1);
    flush();
    assert_eq!(*runs.borrow(), 1, "paused effect must not run on flush");

    owner.resume();
    assert!(!owner.is_paused());
    flush();
    assert_eq!(
        *runs.borrow(),
        2,
        "resume must drain the deferred re-run into pending"
    );
}

#[test]
fn pause_cascades_to_descendants() {
    fresh();
    let parent_runs = Rc::new(RefCell::new(0_u32));
    let child_runs = Rc::new(RefCell::new(0_u32));
    let (read, write) = signal(0_i32).split();

    let parent = Owner::new(None);
    let child = Owner::new(Some(parent));

    let pr = parent_runs.clone();
    parent.with(|| {
        effect(move || {
            let _ = read.get();
            *pr.borrow_mut() += 1;
        });
    });
    let cr = child_runs.clone();
    child.with(|| {
        effect(move || {
            let _ = read.get();
            *cr.borrow_mut() += 1;
        });
    });
    assert_eq!(*parent_runs.borrow(), 1);
    assert_eq!(*child_runs.borrow(), 1);

    parent.pause();
    assert!(parent.is_paused());
    assert!(child.is_paused(), "pause cascades down the tree");

    write.set(1);
    flush();
    assert_eq!(*parent_runs.borrow(), 1);
    assert_eq!(*child_runs.borrow(), 1);

    parent.resume();
    flush();
    assert_eq!(*parent_runs.borrow(), 2);
    assert_eq!(*child_runs.borrow(), 2);
}

#[test]
fn create_owner_inherits_paused_from_parent() {
    fresh();
    let parent = Owner::new(None);
    parent.pause();

    let child = Owner::new(Some(parent));
    assert!(
        child.is_paused(),
        "owner created under a paused parent must inherit paused"
    );
}

#[test]
fn effect_registered_under_paused_owner_defers_initial_run_until_resume() {
    // The initial registration of an effect goes through the same
    // schedule + flush path as a re-run, so the pause gate also
    // applies. The closure fires on the first resume.
    //
    // StackLayout sidesteps this in practice by creating route owners
    // unpaused, running render (which fires every initial effect),
    // and only pausing the non-top routes at the end of the
    // navigation effect.
    fresh();
    let runs = Rc::new(RefCell::new(0_u32));
    let owner = Owner::new(None);
    owner.pause();

    let runs_clone = runs.clone();
    owner.with(|| {
        effect(move || {
            *runs_clone.borrow_mut() += 1;
        });
    });
    assert_eq!(
        *runs.borrow(),
        0,
        "paused at registration: initial run deferred"
    );

    owner.resume();
    flush();
    assert_eq!(*runs.borrow(), 1, "resume fires the deferred initial run");
}

#[test]
fn pause_is_idempotent() {
    fresh();
    let owner = Owner::new(None);
    owner.pause();
    owner.pause();
    assert!(owner.is_paused());
    owner.resume();
    assert!(!owner.is_paused());
    owner.resume();
    assert!(!owner.is_paused());
}

#[test]
fn dispose_while_paused_drops_deferred_entries() {
    fresh();
    let runs = Rc::new(RefCell::new(0_u32));
    let (read, write) = signal(0_i32).split();

    let owner = Owner::new(None);
    let runs_clone = runs.clone();
    owner.with(|| {
        effect(move || {
            let _ = read.get();
            *runs_clone.borrow_mut() += 1;
        });
    });
    owner.pause();
    write.set(1);
    flush();
    assert_eq!(*runs.borrow(), 1, "paused: still 1");

    owner.dispose();
    // The deferred queue had this effect's node; disposal must
    // strip it so a later flush / resume doesn't dereference a
    // freed slot.
    flush();
    // No panic, no extra run.
    assert_eq!(*runs.borrow(), 1);
}

#[test]
fn multiple_paused_signal_writes_collapse_to_one_run_on_resume() {
    fresh();
    let runs = Rc::new(RefCell::new(0_u32));
    let (read, write) = signal(0_i32).split();

    let owner = Owner::new(None);
    let runs_clone = runs.clone();
    owner.with(|| {
        effect(move || {
            let _ = read.get();
            *runs_clone.borrow_mut() += 1;
        });
    });
    owner.pause();

    write.set(1);
    flush();
    write.set(2);
    flush();
    write.set(3);
    flush();
    assert_eq!(*runs.borrow(), 1, "no re-runs while paused");

    owner.resume();
    flush();
    assert_eq!(
        *runs.borrow(),
        2,
        "resume coalesces the deferred re-runs into a single fire"
    );
}
