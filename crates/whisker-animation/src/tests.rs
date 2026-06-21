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
    ctrl.on_finish(move |finished| {
        assert!(finished, "natural completion must report finished=true");
        *c.borrow_mut() += 1;
    });
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

// ----- 11. springs ---------------------------------------------------------

/// Drive a spring controller through frames `frame_ms` apart, starting at
/// `start_ms`, for up to `max_frames`. Returns the number of frames run
/// (i.e. it stops early once the controller deregisters / settles).
fn run_spring_frames(start_ms: f64, frame_ms: f64, max_frames: usize) -> usize {
    let mut now = start_ms;
    // Anchor the run (first frame is the start, integrates nothing).
    __step_for_tests(now);
    for i in 0..max_frames {
        now += frame_ms;
        __step_for_tests(now);
        if __active_count() == 0 {
            return i + 1;
        }
    }
    max_frames
}

#[test]
fn spring_forward_settles_to_one() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::spring());
    let v = ctrl.value();
    ctrl.forward();
    assert_eq!(__active_count(), 1);

    // ~16ms frames; a default spring settles well within a couple seconds.
    let frames = run_spring_frames(0.0, 16.0, 240);
    assert!(frames < 240, "spring never settled (ran all frames)");
    assert!(
        approx(v.get_untracked(), 1.0),
        "spring forward end {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0, "settled spring must deregister");
    assert!(!ctrl.is_animating());
}

#[test]
fn spring_reverse_settles_to_zero() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::spring());
    let v = ctrl.value();
    // Park it at 1.0 first (one-shot set, no self-drive).
    ctrl.set_value(1.0);
    assert!(approx(v.get_untracked(), 1.0));

    ctrl.reverse();
    let frames = run_spring_frames(0.0, 16.0, 240);
    assert!(frames < 240, "spring reverse never settled");
    assert!(
        approx(v.get_untracked(), 0.0),
        "spring reverse end {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0);
}

#[test]
fn spring_is_monotonic_or_overshoots_per_config() {
    // A stiff (near-critically-damped) spring should not overshoot
    // meaningfully above the target.
    fresh();
    let ctrl = AnimationController::new(AnimConfig::stiff());
    let v = ctrl.value();
    ctrl.forward();
    let mut peak_stiff = 0.0_f32;
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..240 {
        now += 16.0;
        __step_for_tests(now);
        peak_stiff = peak_stiff.max(v.get_untracked());
        if __active_count() == 0 {
            break;
        }
    }
    assert!(
        peak_stiff <= 1.01,
        "stiff() spring overshot too far: peak {peak_stiff}"
    );
    assert!(
        approx(v.get_untracked(), 1.0),
        "stiff end {}",
        v.get_untracked()
    );

    // A bouncy (underdamped) spring SHOULD overshoot above 1.0 before
    // settling.
    fresh();
    let ctrl = AnimationController::new(AnimConfig::bouncy());
    let v = ctrl.value();
    ctrl.forward();
    let mut peak_bouncy = 0.0_f32;
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..360 {
        now += 16.0;
        __step_for_tests(now);
        peak_bouncy = peak_bouncy.max(v.get_untracked());
        if __active_count() == 0 {
            break;
        }
    }
    assert!(
        peak_bouncy > 1.05,
        "bouncy() spring should visibly overshoot, peak {peak_bouncy}"
    );
    // …and still settle back to exactly 1.0.
    assert!(
        approx(v.get_untracked(), 1.0),
        "bouncy end {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0);
}

#[test]
fn spring_idle_gap_does_not_explode() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::bouncy());
    let v = ctrl.value();

    // Long idle gap: the engine clock advances far while the spring is
    // idle, so the previous-frame timestamp would be stale.
    __step_for_tests(10_000.0);
    assert_eq!(__active_count(), 0);

    ctrl.forward();
    // First real frame at t=10000: must be ~0 (the run's start), NOT a
    // huge integration step from the stale clock.
    __step_for_tests(10_000.0);
    assert!(
        approx(v.get_untracked(), 0.0),
        "first spring frame after idle must be ~0, got {}",
        v.get_untracked()
    );

    // The next frames must stay bounded and finite — the dt clamp keeps a
    // residual gap from blowing up the integrator.
    let mut now = 10_000.0;
    for _ in 0..30 {
        now += 16.0;
        __step_for_tests(now);
        let x = v.get_untracked();
        assert!(x.is_finite(), "spring value went non-finite: {x}");
        assert!(
            (-0.2..=1.4).contains(&x),
            "spring value left sane range after idle gap: {x}"
        );
    }
}

#[test]
fn spring_finishes_and_goes_idle() {
    fresh();
    let ctrl = AnimationController::new(AnimConfig::spring());
    ctrl.forward();
    let frames = run_spring_frames(0.0, 16.0, 240);
    assert!(frames < 240);
    // Fully idle: deregistered, not animating, runtime has no pending work.
    assert_eq!(__active_count(), 0);
    assert!(!ctrl.is_animating());
    assert!(!whisker_runtime::anim_hook::is_animating());
    assert!(!whisker_runtime::reactive::has_pending_work());
}

// ----- 12. spring velocity: configured initial, hand-off, injection -------

#[test]
fn spring_initial_velocity_speeds_first_frames() {
    // A spring seeded with a positive initial velocity covers more ground
    // in the first few frames than the same spring from rest.
    fresh();
    let a = AnimationController::new(AnimConfig::spring());
    let va = a.value();
    a.forward();
    let mut now = 0.0;
    __step_for_tests(now); // anchor
    for _ in 0..3 {
        now += 16.0;
        __step_for_tests(now);
    }
    let no_vel = va.get_untracked();

    fresh();
    let b = AnimationController::new(AnimConfig::spring().with_velocity(4.0));
    let vb = b.value();
    b.forward();
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..3 {
        now += 16.0;
        __step_for_tests(now);
    }
    let with_vel = vb.get_untracked();

    assert!(
        with_vel > no_vel + 0.02,
        "configured initial velocity should advance further early: \
         with_vel {with_vel} vs no_vel {no_vel}"
    );
}

#[test]
fn spring_velocity_handoff_on_interrupt() {
    // Start a spring forward and let it build momentum, then interrupt it
    // with a new target. The hand-off policy KEEPS the in-flight velocity
    // rather than zeroing it (so a swipe hand-off feels natural).
    fresh();
    let ctrl = AnimationController::new(AnimConfig::spring());
    ctrl.forward();
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..5 {
        now += 16.0;
        __step_for_tests(now);
    }
    let vel_before = ctrl.__velocity();
    assert!(
        vel_before > 0.1,
        "spring should have built positive momentum, got {vel_before}"
    );

    // Interrupt with a new target. Velocity must NOT be reset to 0.
    ctrl.animate_to(0.8);
    let vel_after = ctrl.__velocity();
    assert!(
        (vel_after - vel_before).abs() < 1e-6,
        "interrupt must carry momentum: before {vel_before}, after {vel_after}"
    );
}

#[test]
fn spring_handoff_resets_from_rest() {
    // Starting from rest (not already animating) uses the configured
    // initial velocity, not whatever stale velocity lingered.
    fresh();
    let ctrl = AnimationController::new(AnimConfig::spring().with_velocity(2.0));
    ctrl.forward();
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..200 {
        now += 16.0;
        __step_for_tests(now);
        if __active_count() == 0 {
            break;
        }
    }
    assert_eq!(__active_count(), 0, "should have settled");
    // Now a fresh run from rest: velocity seeds the configured 2.0.
    ctrl.reverse();
    assert!(
        approx(ctrl.__velocity(), 2.0),
        "fresh run from rest should seed configured velocity, got {}",
        ctrl.__velocity()
    );
}

#[test]
fn forward_with_velocity_injects_release_velocity() {
    // forward_with_velocity injects an explicit release velocity; the
    // early frames move faster than a plain forward() from rest.
    fresh();
    let a = AnimationController::new(AnimConfig::spring());
    let va = a.value();
    a.forward();
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..3 {
        now += 16.0;
        __step_for_tests(now);
    }
    let plain = va.get_untracked();

    fresh();
    let b = AnimationController::new(AnimConfig::spring());
    let vb = b.value();
    b.forward_with_velocity(5.0);
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..3 {
        now += 16.0;
        __step_for_tests(now);
    }
    let injected = vb.get_untracked();

    assert!(
        injected > plain + 0.02,
        "injected release velocity should move faster early: \
         injected {injected} vs plain {plain}"
    );
}

// ----- 13. overshoot clamping ---------------------------------------------

#[test]
fn overshoot_clamping_stops_at_target() {
    // bouncy() WOULD overshoot above 1.0 (asserted elsewhere); with
    // overshoot_clamping it must never exceed the target and settles at it.
    fresh();
    let ctrl = AnimationController::new(AnimConfig::bouncy().with_overshoot_clamping(true));
    let v = ctrl.value();
    ctrl.forward();
    let mut peak = 0.0_f32;
    let mut now = 0.0;
    __step_for_tests(now);
    for _ in 0..360 {
        now += 16.0;
        __step_for_tests(now);
        peak = peak.max(v.get_untracked());
        if __active_count() == 0 {
            break;
        }
    }
    assert!(
        peak <= 1.0 + 1e-3,
        "overshoot_clamping must not pass the target, peak {peak}"
    );
    assert!(
        approx(v.get_untracked(), 1.0),
        "clamped spring should settle at target, got {}",
        v.get_untracked()
    );
    assert_eq!(__active_count(), 0, "clamped spring must settle/deregister");
}

// ----- 14. on_finish carries finished/cancelled bool ----------------------

#[test]
fn on_finish_true_on_natural_completion() {
    fresh();
    use std::cell::RefCell;
    use std::rc::Rc;
    let got: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let g = got.clone();
    ctrl.on_finish(move |finished| g.borrow_mut().push(finished));
    ctrl.forward();
    __step_for_tests(0.0);
    __step_for_tests(100.0);
    assert_eq!(*got.borrow(), vec![true]);
}

#[test]
fn on_finish_false_on_stop() {
    fresh();
    use std::cell::RefCell;
    use std::rc::Rc;
    let got: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let g = got.clone();
    ctrl.on_finish(move |finished| g.borrow_mut().push(finished));
    ctrl.forward();
    __step_for_tests(0.0);
    __step_for_tests(40.0);
    ctrl.stop();
    assert_eq!(
        *got.borrow(),
        vec![false],
        "stop mid-flight must report false"
    );
    // A stop while idle fires nothing more.
    ctrl.stop();
    assert_eq!(*got.borrow(), vec![false]);
}

#[test]
fn on_finish_false_on_interrupt() {
    fresh();
    use std::cell::RefCell;
    use std::rc::Rc;
    let got: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let ctrl = AnimationController::new(AnimConfig::linear(100));
    let g = got.clone();
    ctrl.on_finish(move |finished| g.borrow_mut().push(finished));

    ctrl.forward();
    __step_for_tests(0.0);
    __step_for_tests(40.0);
    // Interrupt the in-flight run: the callback fires false for it.
    ctrl.reverse();
    assert_eq!(
        *got.borrow(),
        vec![false],
        "interrupt must cancel the old run with false"
    );
    // The callback stays registered (FnMut): the new run completes true.
    __step_for_tests(40.0);
    // Reverse span from ~0.4 → 0.0 is ~40ms; run far enough to finish.
    __step_for_tests(120.0);
    assert_eq!(
        *got.borrow(),
        vec![false, true],
        "new run completion must report true, callback persists"
    );
}

#[test]
fn owner_dispose_does_not_fire_on_finish() {
    // Owner-dispose is teardown, not a logical cancel: callbacks must NOT
    // fire (matches the documented semantics).
    fresh();
    use std::cell::RefCell;
    use std::rc::Rc;
    let got: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let g = got.clone();
    let owner = Owner::new(None);
    let ctrl = owner.with(|| {
        let c = AnimationController::new(AnimConfig::linear(100));
        c.on_finish(move |finished| g.borrow_mut().push(finished));
        c.forward();
        c
    });
    __step_for_tests(0.0);
    __step_for_tests(20.0);
    owner.dispose();
    assert!(
        got.borrow().is_empty(),
        "owner dispose must not fire on_finish, got {:?}",
        got.borrow()
    );
    drop(ctrl);
}
