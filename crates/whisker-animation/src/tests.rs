//! Unit tests for the continuous animation engine.
//!
//! Every test resets both the reactive runtime and the animation
//! scheduler first (cargo reuses worker threads, so thread-locals must
//! be cleared between runs), and drives frames through the injectable
//! virtual clock [`__step_for_tests`] so results are exact and no real
//! time passes.

use super::*;
use whisker_css::data_type::Color;
use whisker_runtime::reactive::{Owner, ReadSignal, flush};

/// Reset thread-local runtime + animation scheduler.
fn fresh() {
    whisker_runtime::reactive::__reset_for_tests();
    __reset_for_tests();
}

/// Approximate float equality for progress/value assertions.
fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

// ----- 1. Animatable::lerp -------------------------------------------------

#[test]
fn lerp_f32_endpoints_and_midpoint() {
    fresh();
    assert!(approx(f32::lerp(&0.0, &100.0, 0.0), 0.0));
    assert!(approx(f32::lerp(&0.0, &100.0, 1.0), 100.0));
    assert!(approx(f32::lerp(&0.0, &100.0, 0.5), 50.0));
    assert!(approx(f32::lerp(&20.0, &-20.0, 0.5), 0.0));
}

#[test]
fn lerp_color_rgba_endpoints_and_midpoint() {
    fresh();
    let from = Color::rgb(0, 0, 0);
    let to = Color::rgb(255, 100, 50);
    assert_eq!(Color::lerp(&from, &to, 0.0), from);
    assert_eq!(Color::lerp(&from, &to, 1.0), to);
    // Midpoint: each channel halves (with rounding).
    match Color::lerp(&from, &to, 0.5) {
        Color::Rgba(r, g, b, a) => {
            assert_eq!((r, g, b), (128, 50, 25));
            assert!(approx(a, 1.0));
        }
        other => panic!("expected Rgba, got {other:?}"),
    }
}

#[test]
fn lerp_color_alpha_interpolates() {
    fresh();
    let from = Color::rgba(255, 0, 0, 0.0);
    let to = Color::rgba(255, 0, 0, 1.0);
    match Color::lerp(&from, &to, 0.25) {
        Color::Rgba(_, _, _, a) => assert!(approx(a, 0.25)),
        other => panic!("expected Rgba, got {other:?}"),
    }
}

// ----- 2. Curves -----------------------------------------------------------

#[test]
fn curves_pin_endpoints() {
    for c in [
        Curve::Linear,
        Curve::EaseIn,
        Curve::EaseOut,
        Curve::EaseInOut,
        Curve::CubicBezier(0.42, 0.0, 0.58, 1.0),
    ] {
        assert!(approx(c.ease(0.0), 0.0), "{c:?} f(0) != 0");
        assert!(approx(c.ease(1.0), 1.0), "{c:?} f(1) != 1");
    }
}

#[test]
fn curves_are_monotonic() {
    for c in [
        Curve::Linear,
        Curve::EaseIn,
        Curve::EaseOut,
        Curve::EaseInOut,
        Curve::CubicBezier(0.25, 0.1, 0.25, 1.0),
    ] {
        let mut prev = c.ease(0.0);
        let mut t = 0.0;
        while t <= 1.0 {
            let v = c.ease(t);
            assert!(
                v + 1e-4 >= prev,
                "{c:?} not monotonic at t={t}: {v} < {prev}"
            );
            prev = v;
            t += 0.02;
        }
    }
}

#[test]
fn ease_out_vs_ease_in_asymmetry() {
    // ease_out is ahead of linear at the midpoint; ease_in is behind.
    let mid_in = Curve::EaseIn.ease(0.5);
    let mid_out = Curve::EaseOut.ease(0.5);
    assert!(mid_in < 0.5, "ease_in mid {mid_in} should be < 0.5");
    assert!(mid_out > 0.5, "ease_out mid {mid_out} should be > 0.5");
    // Symmetric about 0.5 for cubic in/out.
    assert!(approx(mid_in, 1.0 - mid_out));
}

#[test]
fn curve_clamps_out_of_range() {
    assert!(approx(Curve::Linear.ease(-1.0), 0.0));
    assert!(approx(Curve::Linear.ease(2.0), 1.0));
}

// ----- 3. forward / reverse ------------------------------------------------

#[test]
fn forward_reaches_one_then_reverse_returns_to_zero() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    assert!(approx(v.get_untracked(), 0.0));

    ctrl.forward();
    // Step in 25ms increments; t=0 at first registered start (last_ms=0).
    __step_for_tests(0.0);
    __step_for_tests(50.0);
    assert!(
        v.get_untracked() > 0.4 && v.get_untracked() < 0.6,
        "mid {}",
        v.get_untracked()
    );
    __step_for_tests(100.0);
    assert!(approx(v.get_untracked(), 1.0), "end {}", v.get_untracked());
    // Finished → deregistered.
    assert_eq!(__active_count(), 0);

    ctrl.reverse();
    __step_for_tests(100.0); // run start re-anchored to last seen 100
    __step_for_tests(150.0);
    assert!(
        v.get_untracked() < 0.6,
        "reversing mid {}",
        v.get_untracked()
    );
    __step_for_tests(200.0);
    assert!(
        approx(v.get_untracked(), 0.0),
        "reverse end {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0);
}

// ----- 4. idle when stopped ------------------------------------------------

#[test]
fn idle_when_no_controllers_active() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    // Constructed but not driven: nothing active, not animating.
    assert_eq!(__active_count(), 0);
    assert!(!whisker_runtime::anim_hook::is_animating());

    ctrl.forward();
    assert_eq!(__active_count(), 1);
    assert!(whisker_runtime::anim_hook::is_animating());

    // Run to completion; it must deregister and report idle.
    __step_for_tests(0.0);
    __step_for_tests(100.0);
    assert_eq!(__active_count(), 0);
    assert!(!whisker_runtime::anim_hook::is_animating());
    // has_pending_work must no longer be kept busy by animation.
    assert!(!whisker_runtime::reactive::has_pending_work());
}

#[test]
fn stop_deregisters_at_current_value() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    ctrl.forward();
    __step_for_tests(0.0);
    __step_for_tests(40.0);
    let held = v.get_untracked();
    ctrl.stop();
    assert_eq!(__active_count(), 0);
    // Further frames don't move it.
    __step_for_tests(200.0);
    assert!(approx(v.get_untracked(), held), "stop should hold value");
}

// ----- 5. set_value is one-shot --------------------------------------------

#[test]
fn set_value_does_not_self_drive() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    ctrl.set_value(0.42);
    assert!(approx(v.get_untracked(), 0.42));
    assert_eq!(__active_count(), 0);
    assert!(!whisker_runtime::anim_hook::is_animating());
    // Next frame must not change it.
    __step_for_tests(16.0);
    __step_for_tests(32.0);
    assert!(
        approx(v.get_untracked(), 0.42),
        "set_value drifted to {}",
        v.get_untracked()
    );
}

// ----- 6. one controller drives multiple tweens ----------------------------

#[test]
fn one_controller_drives_f32_and_color_tweens() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let x: ReadSignal<f32> = Tween::new(0.0_f32, 200.0).animate(&ctrl);
    let col: ReadSignal<Color> =
        Tween::new(Color::rgb(0, 0, 0), Color::rgb(100, 100, 100)).animate(&ctrl);

    ctrl.forward();
    __step_for_tests(0.0);
    __step_for_tests(50.0); // half-way (linear)
    flush();
    assert!(
        approx(x.get_untracked(), 100.0),
        "x mid {}",
        x.get_untracked()
    );
    match col.get_untracked() {
        Color::Rgba(r, g, b, _) => assert_eq!((r, g, b), (50, 50, 50)),
        other => panic!("expected Rgba, got {other:?}"),
    }

    __step_for_tests(100.0);
    flush();
    assert!(approx(x.get_untracked(), 200.0));
    match col.get_untracked() {
        Color::Rgba(r, g, b, _) => assert_eq!((r, g, b), (100, 100, 100)),
        other => panic!("expected Rgba, got {other:?}"),
    }
}

// ----- 7. interactive: scrub then settle -----------------------------------

#[test]
fn interactive_scrub_then_commit_settles_to_one() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    // Finger drags across several frames.
    for p in [0.1_f32, 0.25, 0.4, 0.55] {
        ctrl.set_value(p);
        __step_for_tests(0.0); // a frame in which nothing self-drives
        assert!(approx(v.get_untracked(), p));
        assert_eq!(__active_count(), 0);
    }
    // Release → commit: drive to 1.0.
    ctrl.forward();
    __step_for_tests(0.0);
    // Remaining span is 0.45 → 0.45*100ms = 45ms to finish.
    __step_for_tests(45.0);
    assert!(
        approx(v.get_untracked(), 1.0),
        "commit end {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0);
}

#[test]
fn interactive_scrub_then_cancel_settles_to_zero() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    ctrl.set_value(0.6);
    __step_for_tests(0.0);
    ctrl.reverse();
    __step_for_tests(0.0);
    __step_for_tests(60.0); // 0.6 span → 60ms
    assert!(
        approx(v.get_untracked(), 0.0),
        "cancel end {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0);
}

// ----- 8. cleanup deregisters on owner dispose -----------------------------

#[test]
fn controller_deregisters_on_owner_dispose() {
    fresh();
    let owner = Owner::new(None);
    let ctrl = owner.with(|| {
        let c = AnimationController::new(AnimConfig::linear(100));
        c.forward();
        c
    });
    // Mid-flight: active.
    __step_for_tests(0.0);
    __step_for_tests(20.0);
    assert_eq!(__active_count(), 1);

    // Dispose the owner: the controller's on_cleanup must deregister.
    owner.dispose();
    assert_eq!(__active_count(), 0, "controller leaked after owner dispose");
    assert!(!whisker_runtime::anim_hook::is_animating());
    drop(ctrl);
}

// ----- 9. determinism: exact progress at exact frames ----------------------

#[test]
fn deterministic_linear_progress_values() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    ctrl.forward();
    // Anchor the run at t=0.
    __step_for_tests(0.0);
    // Linear, 100ms duration: progress == elapsed/100.
    __step_for_tests(10.0);
    assert!(
        approx(v.get_untracked(), 0.10),
        "t=10 -> {}",
        v.get_untracked()
    );
    __step_for_tests(33.0);
    assert!(
        approx(v.get_untracked(), 0.33),
        "t=33 -> {}",
        v.get_untracked()
    );
    __step_for_tests(75.0);
    assert!(
        approx(v.get_untracked(), 0.75),
        "t=75 -> {}",
        v.get_untracked()
    );
    __step_for_tests(100.0);
    assert!(approx(v.get_untracked(), 1.0));
}

#[test]
fn deterministic_ease_in_progress_values() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::ease_in(100));
    let v = ctrl.value();
    ctrl.forward();
    __step_for_tests(0.0);
    // ease_in = t^3. At elapsed 50ms, raw t=0.5 -> 0.125.
    __step_for_tests(50.0);
    assert!(
        approx(v.get_untracked(), 0.125),
        "ease_in t=50 -> {}",
        v.get_untracked()
    );
    __step_for_tests(100.0);
    assert!(approx(v.get_untracked(), 1.0));
}

// ----- on_finish + animated() sugar ----------------------------------------

#[test]
fn on_finish_fires_once_at_target() {
    fresh();
    use std::cell::RefCell;
    use std::rc::Rc;
    let count = Rc::new(RefCell::new(0));
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let c = count.clone();
    ctrl.on_finish(move || *c.borrow_mut() += 1);
    ctrl.forward();
    __step_for_tests(0.0);
    __step_for_tests(100.0);
    assert_eq!(*count.borrow(), 1);
    // No extra frames re-fire it.
    __step_for_tests(200.0);
    assert_eq!(*count.borrow(), 1);
}

#[test]
fn animated_sugar_no_autoplay() {
    fresh();
    let (x, ctrl) = animated(0.0_f32, 100.0, AnimConfig::linear(100));
    // No autoplay: stays at `from` with no frames requested.
    __step_for_tests(0.0);
    __step_for_tests(50.0);
    flush();
    assert!(
        approx(x.get_untracked(), 0.0),
        "autoplayed to {}",
        x.get_untracked()
    );
    assert_eq!(__active_count(), 0);

    // Explicit forward drives it. The run anchors at the scheduler's
    // last-seen time (50ms), so a full 100ms sweep finishes at 150ms.
    ctrl.forward();
    __step_for_tests(50.0);
    __step_for_tests(150.0);
    flush();
    assert!(
        approx(x.get_untracked(), 100.0),
        "forward end {}",
        x.get_untracked()
    );
}

#[test]
fn animate_to_already_at_target_finishes_without_registering() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();
    // Already at 0.0; reverse() targets 0.0 → immediate finish.
    ctrl.reverse();
    assert_eq!(__active_count(), 0);
    assert!(approx(v.get_untracked(), 0.0));
}

// ----- 10. regression: idle gap must not teleport on the first frame -------

#[test]
fn forward_after_idle_gap_starts_from_zero_not_teleport() {
    // The run's start time is anchored on its FIRST advance, not on the
    // scheduler's last-seen timestamp. Otherwise, after a long idle gap
    // (the scheduler's `last_ms` is stale), the first `forward()` frame
    // would compute a huge elapsed and jump straight to the target — the
    // "single tap teleports, mashing animates" bug. Here we let the clock
    // advance to 5000ms while idle, then forward() and assert the very
    // first frame is ~0, and the animation then progresses smoothly.
    fresh();
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let v = ctrl.value();

    // Simulate a long idle gap: the engine's clock has moved far ahead
    // (e.g. earlier animations / app uptime) with this controller idle.
    __step_for_tests(5000.0);
    assert_eq!(__active_count(), 0, "nothing should be animating yet");

    ctrl.forward();
    // First real frame at t=5000: must read ~0, NOT jump to 1.0.
    __step_for_tests(5000.0);
    assert!(
        approx(v.get_untracked(), 0.0),
        "first frame after idle must be ~0, got {} (teleport bug)",
        v.get_untracked()
    );
    // Then it advances normally over its 100ms duration.
    __step_for_tests(5050.0);
    assert!(
        v.get_untracked() > 0.4 && v.get_untracked() < 0.6,
        "mid {}",
        v.get_untracked()
    );
    __step_for_tests(5100.0);
    assert!(approx(v.get_untracked(), 1.0), "end {}", v.get_untracked());
    assert_eq!(__active_count(), 0);
}
