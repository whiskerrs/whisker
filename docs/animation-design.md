# Animation Design

Whisker has **two animation systems**, and this document is about the
second one:

1. **Lynx animations** (existing) — the engine's CSS-keyframe animator,
   reached through `animate_start` / `animate_cancel` / `AnimateOptions`
   (the `lynx_element_animate` bridge). These are **discrete,
   time-driven, declarative** keyframe animations: you name a set of
   keyframes and an operation (`START` / `PLAY` / `PAUSE` / `CANCEL` /
   `FINISH`) and Lynx runs them on its own clock. There is **no API to
   scrub progress** — you cannot set "currentTime = 0.42" from Rust. They
   are great for fire-and-forget CSS transitions; they are a poor fit for
   gesture-driven, interruptible, physics-based motion.

2. **Whisker animations** (this doc, design) — a **continuous,
   signal-based** animation engine that lives entirely in Rust. It does
   not use Lynx's animator at all. It asks the host only for two things
   it already provides: a **vsync wake-up** (`request_frame`) and
   **per-property style application** (`set_inline_styles`). Everything
   else — interpolation, easing, springs, scheduling, explicit
   forward/reverse control — is plain Rust. No Lynx-fork or C++ bridge
   change is required.

This doc is the *design* — the model and the "why". The engine lives in
the `whisker-animation` crate (`AnimationController` + `Tween` + the
frame-driving hook); the router (`whisker-router`) is its intended first
consumer (see `docs/router-design.md`), not yet wired.

> Status: **engine implemented** (`crates/whisker-animation`,
> unit-tested); **not yet wired into the router or any device path**.

## Why a second system

Whisker's reactive runtime is fine-grained: when a signal changes, only
the inline-style / attribute that reads it is patched — there is no
virtual DOM and no diff. That makes "update a value every frame" cheap,
which is exactly what a continuous animation is. So a continuous engine
is a natural fit: **an animation is just a signal whose value the runtime
advances each frame**, and the existing reactive plumbing carries it to
the screen.

Lynx's animator can't give us what interactive UI needs:

- **No progress scrubbing.** `lynx_element_animate` exposes only
  START/PLAY/PAUSE/CANCEL/FINISH — discrete operations on Lynx's own
  clock. An iOS swipe-back / Android predictive-back / modal-drag needs
  to set the animation to an arbitrary 0..1 driven by a finger, which the
  CSS-keyframe model cannot do.
- **No springs / physics.** CSS keyframes are time-curves; spring-to-rest
  motion isn't expressible.
- **Not unified with reactivity.** A Lynx animation is opaque to the
  signal graph; you can't `computed()` over its current value.

The existing `whisker-router` gesture code already worked around this by
**hand-writing transforms via `set_inline_styles` every frame** (see
`packages/whisker-router/src/gestures/ios_swipe_back.rs`). This engine
generalises that ad-hoc approach into a first-class primitive.

## The model: Controller + Tween (Flutter's split)

The design follows Flutter's `AnimationController` + `Tween` split, which
cleanly separates *driving* from *interpolating*:

| Concept | Role | State |
| --- | --- | --- |
| `AnimationController` | Drives a `0..1` **progress** as an explicit state machine: `forward` / `reverse` / `stop` / `set_value`. **Idle when not driving** (requests no frames). | stateful (current progress) |
| `Tween<T>` | A **pure** mapping from `0..1` to a value of type `T`. Stateless, reusable. | stateless |

`tween.animate(&controller)` ties them together and returns a
`ReadSignal<T>` — the animated value, consumable anywhere in the reactive
graph.

```rust
// Controller: drives progress; explicit control; idle when stopped.
let ctrl = AnimationController::new(AnimConfig::ease_out(300));

// Tween: pure interpolation definition (no state; reusable).
let x:     ReadSignal<f32>   = Tween::new(0.0,  100.0).animate(&ctrl);
let color: ReadSignal<Color> = Tween::new(BLUE, RED  ).animate(&ctrl); // same ctrl, second value

// Read it like any signal — fine-grained: only this transform re-patches.
view(style: css!(...).raw("transform", computed(move ||
    format!("translateX({}px)", x.get())))) {}
```

`Tween<T>` works for any `T: Animatable` (the lerp trait): `f32`,
`Color`, transform components, etc. One controller can drive **several**
tweens (one progress, many values follow).

## Explicit control — no auto-play

A key decision: **animations do not start on their own.** Real UI motion
is event-driven (a push, a tap, a finger), so a "starts running when
mounted" animation has essentially no use beyond demos. The controller is
a state machine you drive explicitly:

```rust
ctrl.forward();        // current → 1.0
ctrl.reverse();        // current → 0.0
ctrl.stop();           // halt at current value
ctrl.animate_to(0.5);  // current → arbitrary target
ctrl.value();          // ReadSignal<f32> of the current progress
// optional: ctrl.repeat(), ctrl.repeat_reverse() (ping-pong), ctrl.on_finish(..)
```

Because there is no auto-play, the convenience constructor returns the
**value signal and its controller together** (you always need both):

```rust
// Sugar for the common single-value case: build value + controller at once.
let (x, ctrl) = animated(0.0, 100.0, AnimConfig::ease_out(300));
ctrl.forward();        // you decide when it runs

// Full form for sharing one controller across multiple tweens:
let ctrl  = AnimationController::new(cfg);
let x     = Tween::new(0.0, 100.0).animate(&ctrl);
let color = Tween::new(BLUE, RED ).animate(&ctrl);
```

There is **no auto-playing `animated()`** variant — it was dropped as
useless. `animated(from, to, cfg)` only *constructs* the `(signal,
controller)` pair; nothing moves until you call `forward`/`reverse`/etc.

## "Not always updating" — idle is free

The controller drives the engine **only while it is actively animating**:

- `forward()` / `reverse()` / `animate_to()` / `repeat()` → registers
  with the `AnimationScheduler`, which requests a frame every vsync and
  advances progress.
- Reaching the target (or `stop()`) → **deregisters**; when the scheduler
  is empty it **releases vsync** (no `request_frame`). Idle animations
  cost nothing — the value signal simply holds its last value.
- `set_value(x)` → updates the progress once (for finger-driven frames);
  it does not start self-driving.

This matches the runtime's existing level-triggered idle: vsync is only
spun while there is motion to show.

## Interactive gestures use the same controller

Gesture-driven, interruptible motion (iOS swipe-back, Android predictive
back, modal swipe-down) is expressed by **driving the controller's value
by hand, then handing off to `forward`/`reverse` on release**:

```rust
let ctrl = AnimationController::new(AnimConfig::spring());

// touchmove: set progress directly from the finger (one update per frame)
ctrl.set_value(drag_progress);     // 0..1 follows the finger

// touchend: let it settle — commit drives to 1, cancel drives back to 0
if commit { ctrl.forward() } else { ctrl.reverse() }   // spring to rest
```

The intermediate state is "just a signal value being set", so the
WAAPI-style "scrub a keyframe definition" problem that the Lynx animator
can't solve simply does not arise here. This is the generalised form of
what `ios_swipe_back.rs` does today.

## Internals

```
AnimationController::forward():
  compute target = 1.0 (reverse → 0.0); record start_value, start_time
  register self with AnimationScheduler

AnimationScheduler (one per runtime):
  holds the set of active controllers
  while non-empty → request_frame every vsync
  each frame, for each controller:
    elapsed   = now - start_time
    progress  = curve(elapsed / duration)        // or a spring step
    controller.value_signal.set(progress)        // a signal write
    if finished → deregister (+ fire on_finish)
  when the set empties → stop requesting frames (release vsync)

Tween<T>::animate(&ctrl) -> ReadSignal<T>:
  computed(move || T::lerp(from, to, ctrl.value().get()))
```

- **`Animatable` trait**: `lerp(from, to, t) -> Self` for `f32`, `Color`,
  transform parts, etc. Pure Rust, unit-testable.
- **Curves / springs**: `AnimConfig` carries duration + easing (`linear`,
  `ease_in/out`, cubic-bezier) or a spring (stiffness/damping). Springs
  step by physics rather than `elapsed/duration`.
- **vsync / frame driving** (as built): the runtime is level-triggered —
  the host ticks while `scheduler::has_pending_work()` is true — so the
  engine does **not** poke `request_frame` directly. Instead it uses an
  inversion-of-control surface, `whisker_runtime::anim_hook`: the engine
  registers a per-frame `step` callback and a latched `is_animating`
  flag, `has_pending_work()` ORs in `anim_hook::is_animating()`, and the
  driver's `tick_frame` calls `anim_hook::step(monotonic_ms)` once per
  frame (before the reactive flush, so the progress write paints the same
  frame). `forward()`/etc. set the flag and `wake_runtime()` for an
  immediate wake; finishing clears it and the runtime goes idle (releases
  vsync). This keeps `whisker-runtime`/`whisker-driver` free of a
  dependency on the engine crate. Time is a monotonic millisecond
  timestamp passed into `step`; tests inject it for determinism.
- **Application**: the value is a signal; reading it in `css!` /
  `computed` re-runs that one effect, which calls the existing
  `set_inline_styles`. (Optimisation: where a numeric attribute path
  exists, prefer it over formatting a `transform` string each frame.)
- **Lifecycle**: a controller is owned by the component that creates it
  (Whisker's owner system); on unmount it is disposed and deregistered —
  no leaked frame requests.

## Relationship to the router

`whisker-router`'s `Route` transition parameters (`enter` / `exit` /
`pop_enter` / `pop_exit`, see `docs/router-design.md`) are implemented on
top of this engine, **not** on Lynx's animator:

- A non-interactive push/pop runs a controller `forward()`/`reverse()`
  over the route wrapper's transform/opacity tweens.
- An interactive back drives the same controller via `set_value` from the
  gesture, then `forward`/`reverse` on release.

This is what lets the router's "one definition, intermediate state
derived from progress" model actually hold: progress is a real,
continuously-driven signal, not a discrete keyframe set being scrubbed.
The transition presets (`Slide`, `Fade`, `Spring`, …) are
`Tween`-builders over a controller, with the gesture and the auto-run
sharing the same progress.

## When to use which system

| Want | Use |
| --- | --- |
| Fire-and-forget CSS keyframe effect, no interruption, no progress control | **Lynx** (`animate_start` / `AnimateOptions`) |
| Gesture-driven, interruptible, spring/physics, value composable in the signal graph, router transitions | **Whisker** (`AnimationController` + `Tween`, this doc) |

The two coexist. Lynx animations remain available and unchanged for the
declarative cases they're good at; the Whisker engine covers everything
that needs continuous, controllable, reactive motion.

## Open items

- `Animatable` coverage: shipped for `f32` and `Color`; transform
  shorthand and how transform composition is expressed are still open.
  **Caveat (as built):** `Color::Named` can't be lerped — the 147-entry
  named-colour table lives in Lynx, not this crate — so named endpoints
  **snap at the midpoint**; smooth colour tweens need explicit
  `Color::rgb(..)` / `Color::hsl(..)` endpoints.
- Spring parameterisation (stiffness/damping vs duration/bounce presets).
- Numeric attribute fast-path vs `transform` string formatting per frame
  (perf for many simultaneous animations).
- `repeat` / `ping-pong` / `on_finish` final surface.
- Multi-frame budget: keep per-frame interpolation off the critical path
  on the TASM thread; verify 60/120 fps on low-end Android.
