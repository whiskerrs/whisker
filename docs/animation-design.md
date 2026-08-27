# Animation Design

Whisker owns animation time in Rust. Hosts provide a monotonic frame timestamp,
apply the ordinary `FramePacket` produced for that sample, and schedule another
VSync while `RuntimeDrive::needs_frame` is true. There is no Host animation
object and no animation-specific wire protocol.

Two authoring surfaces share that frame driver:

1. CSS transitions and CSS keyframe animations, for declarative style motion.
2. `AnimationController` and `Tween<T>`, for imperative, gesture-driven, or
   physics-based motion.

They intentionally solve different problems. A transition reacts to a computed
style target changing. A keyframe animation starts with its element and follows
a declared timeline. An imperative controller exposes progress so application
code can scrub, reverse, interrupt, or derive several values from one gesture.

## CSS transitions

Whisker does not need selectors to implement transitions. A transition compares
the previously resolved target with the new target whenever reactive rendering
updates an element's `Css` value:

```rust
let style = if expanded.get() {
    Css::new().height(px(240)).opacity(1.0)
} else {
    Css::new().height(px(0)).opacity(0.0)
}
.transition(
    Transition::new(TransitionPropertyKind::All)
        .duration(240.ms())
        .timing(EasingFunction::EaseOut),
);
```

The current presentation value becomes the next transition's starting value,
so an interrupted transition remains continuous. Delays may be negative. The
runtime requests frames only while a transition is active.

## CSS keyframe animations

Keyframes are typed immutable values built in ordinary Rust. There is no
`keyframes!` macro and no global string registry:

```rust
let pulse = Keyframes::builder()
    .named("pulse")
    .from(Css::new().opacity(0.5))
    .at(50.percent(), Css::new().opacity(1.0))
    .to(Css::new().opacity(0.5))
    .build()?;

let style = Css::new().animation(
    Animation::new(pulse)
        .duration(800.ms())
        .timing(EasingFunction::EaseInOut)
        .iteration_count(AnimationIterationCount::Infinite)
        .direction(AnimationDirection::Alternate),
);
```

`KeyframesBuilder` validates offsets, sorts frames, and merges duplicate
offsets with later declarations winning. Motion declarations cannot be nested
inside a keyframe. Missing `0%` or `100%` property values are synthesized from
the element's underlying computed style.

The runtime implements delay, negative delay, duration, timing functions,
per-keyframe timing functions, finite and infinite iteration counts,
`normal`/`reverse`/`alternate` directions, fill modes, and pause/resume without
counting paused wall-clock time. Later animations win for a property; an active
transition wins over a keyframe animation, matching CSS cascade priority.

String names are retained only for source compatibility with the former Lynx
path. A string has no runtime registry to resolve; new code passes `Keyframes`
directly to `Animation::new` or `animation_name`.

## Interpolation and layout

The common sampler currently covers:

- opacity and numeric flex growth;
- background, border, and inherited text colors in premultiplied sRGB;
- compatible translate, rotate, scale, and skew function lists, including
  identity padding;
- matrix functions and incompatible transform-list suffixes through common
  4x4 matrix decomposition, quaternion interpolation, and recomposition;
- transform origin;
- the Lynx transitionable layout slice: physical insets, width/height and
  min/max constraints, margin, padding, border widths, and flex basis.

Layout samples are written back into `ComputedLayoutStyle` before Taffy runs.
The resulting geometry is sent through the ordinary `SetLayout` operation, so
all four Hosts observe the same sampled layout and need no property-specific
animation implementation.

Length-percentage pairs interpolate as two affine components. `auto`, intrinsic
sizing keywords, and incompatible value kinds sample discretely in keyframes
and do not start transitions. Singular transform matrices that cannot be
decomposed use discrete midpoint sampling; all other transform interpolation is
performed in Rust after resolving percentage translations against the final
Taffy border box.

## Lifecycle events

The Rust timeline emits the Lynx-compatible lifecycle surface through the same
capture/bubble listener path as Host input events:

- `animationstart`, `animationiteration`, `animationend`, and
  `animationcancel` for keyframe animations;
- `transitionstart`, `transitionend`, and `transitioncancel` for transitions.

The payload deserializes into `AnimationEvent`. It identifies the animation as
`keyframe-animation` or `transition-animation`; `animation_name` contains the
keyframe name or transitioned CSS property. Replacing an active timeline emits
its cancel event before the replacement starts. Because the runtime owns both
time and dispatch, Hosts need neither animation objects nor lifecycle-specific
protocol commands.

## Frame and idle behavior

```text
Host VSync(timestamp)
  -> RuntimeInstance::drive_frame
  -> sample imperative controllers
  -> flush reactive style changes
  -> sample CSS timelines
  -> run Taffy when a layout value changed
  -> produce an ordinary FramePacket
  -> needs_frame = any timeline/controller still active
```

No animation thread remains resident. When every timeline is paused or
finished and no other runtime work is pending, the Host stops scheduling
frames. Animation state stays retained with the element.

## Imperative motion

`AnimationController` drives normalized progress and `Tween<T>` maps that
progress into application values. It remains the preferred API for dragging,
predictive back, interruptible navigation, springs, and values composed in the
reactive graph:

```rust
let controller = AnimationController::new(AnimConfig::ease_out(300));
let x = Tween::new(0.0, 100.0).animate(&controller);

controller.set_value(drag_progress);
if commit {
    controller.forward();
} else {
    controller.reverse();
}
```

CSS property interpolation is conceptually Tween-like, but it does not allocate
a public `Tween` or reactive signal per property. Compiled property tracks are
sampled directly into retained presentation state to keep per-frame overhead
small.

## Compatibility boundary

The current public API follows Lynx's lifecycle event set. The browser-only
`transitionrun` event is not exposed; adding it later would be an API expansion,
not a Host rendering change.
