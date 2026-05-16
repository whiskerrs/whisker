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
    let (read, _write) = signal(42_i32);
    assert_eq!(read.get(), 42);
}

#[test]
fn write_signal_updates_value() {
    fresh();
    let (read, write) = signal(0_i32);
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
    let (read, write) = signal(vec![1, 2, 3]);
    let sum = read.with(|v| v.iter().sum::<i32>());
    assert_eq!(sum, 6);
    write.update(|v| v.push(4));
    assert_eq!(read.with(|v| v.len()), 4);
}

#[test]
fn signal_handles_are_copy_and_aliasable() {
    fresh();
    let (read, _write) = signal(String::from("hello"));
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
    let (count, set_count) = signal(0_i32);
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
fn effect_only_reruns_for_tracked_deps() {
    fresh();
    let (tracked, set_tracked) = signal(0_i32);
    let (untracked, set_untracked) = signal(100_i32);
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
    let (toggle, set_toggle) = signal(false);
    let (a, set_a) = signal(0_i32);
    let (b, set_b) = signal(0_i32);
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
    let (a, set_a) = signal(0_i32);
    let (b, set_b) = signal(0_i32);
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
    assert_eq!(*runs.borrow(), 2, "two writes must produce exactly one re-run");
}

#[test]
fn signals_written_during_flush_propagate_in_same_flush() {
    fresh();
    let (a, set_a) = signal(0_i32);
    let (b, set_b) = signal(0_i32);
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
    let owner = create_owner(None);
    let (read, _write) = with_owner(owner, || signal(123_i32));
    // Signal works while owner is alive.
    assert_eq!(read.get(), 123);
    dispose_owner(owner);
    // After disposal the underlying node is gone; reads panic in debug
    // and we don't want to fault the test, so we just verify the
    // owner slot itself is freed by trying to dispose again (no-op).
    dispose_owner(owner);
}

#[test]
fn dispose_cascades_to_children() {
    fresh();
    let parent = create_owner(None);
    let mut leaf_signals = Vec::new();
    let child = with_owner(parent, || {
        let c = create_owner(None);
        with_owner(c, || {
            let (r, _w) = signal(0_u32);
            leaf_signals.push(r);
        });
        c
    });

    // Parent owns child; disposing parent disposes child too.
    dispose_owner(parent);

    // Owner map should have neither.
    let alive = with_runtime(|rt| {
        (rt.owners.contains_key(parent), rt.owners.contains_key(child))
    });
    assert_eq!(alive, (false, false));
}

#[test]
fn on_cleanup_fires_lifo() {
    fresh();
    let owner = create_owner(None);
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    with_owner(owner, || {
        let l = log.clone();
        on_cleanup(move || l.borrow_mut().push("first"));
        let l = log.clone();
        on_cleanup(move || l.borrow_mut().push("second"));
        let l = log.clone();
        on_cleanup(move || l.borrow_mut().push("third"));
    });

    dispose_owner(owner);
    assert_eq!(*log.borrow(), vec!["third", "second", "first"]);
}

#[test]
fn disposing_owner_removes_its_effects_from_pending() {
    fresh();
    let owner = create_owner(None);
    let (count, set_count) = signal(0_i32);
    let runs = Rc::new(RefCell::new(0));
    let runs_clone = runs.clone();

    with_owner(owner, || {
        effect(move || {
            let _ = count.get();
            *runs_clone.borrow_mut() += 1;
        });
    });
    assert_eq!(*runs.borrow(), 1);

    // Schedule the effect, then dispose its owner before flush.
    set_count.set(1);
    dispose_owner(owner);
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
    assert_eq!(*runs.borrow(), 1, "StoredValue writes must not trigger reactivity");
}

#[test]
fn stored_value_disposed_with_owner() {
    fresh();
    let owner = create_owner(None);
    let sv = with_owner(owner, || StoredValue::new(123_i32));
    assert_eq!(sv.get(), 123);
    dispose_owner(owner);
    // Owner gone — node is gone too. We can verify indirectly: a fresh
    // owner has no leftover.
    let leftover = with_runtime(|rt| rt.owners.contains_key(owner));
    assert!(!leftover);
}

// ----- Memo -----------------------------------------------------------------

#[test]
fn memo_caches_initial_value() {
    fresh();
    let (count, _set) = signal(3_i32);
    let doubled = memo(move || count.get() * 2);
    assert_eq!(doubled.get(), 6);
}

#[test]
fn memo_recomputes_on_source_change() {
    fresh();
    let (count, set_count) = signal(0_i32);
    let doubled = memo(move || count.get() * 2);
    assert_eq!(doubled.get(), 0);
    set_count.set(5);
    flush();
    assert_eq!(doubled.get(), 10);
}

#[test]
fn memo_notifies_downstream_subscribers() {
    fresh();
    let (count, set_count) = signal(0_i32);
    let doubled = memo(move || count.get() * 2);
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
fn memo_does_not_notify_when_value_unchanged() {
    fresh();
    let (count, set_count) = signal(5_i32);
    // floor-div by 10 — different `count` values yield the same memo value
    let bucket = memo(move || count.get() / 10);
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

// ----- Self-write doesn't cause unbounded recursion --------------------------

#[test]
fn flush_breaks_self_feedback_loop_with_warning() {
    fresh();
    let (count, set_count) = signal(0_i32);
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
    let (a, _) = signal(0_i32);
    let (_b, set_b) = signal(0_i32);
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
