# Cross-crate dependencies discovered while building whisker-router

This file tracks every place where the router design requires
extending another crate. Per the agreed working rule, none of these
changes are made unilaterally — each is surfaced here and discussed
with the project owner before landing.

Status legend: 🟡 needed soon · 🟠 blocking next phase · 🔴 blocking
something already attempted.

## 🟠 1. `Animatable<T>` primitive in `whisker-runtime`

**Why**: Stack-push / pop transitions and the interactive pop
gesture both need a per-frame interpolation driver. CSS
`transition` covers the declarative cases, but anything driven by
a signal (gesture-tracking transforms, programmatic
`animate_to(target, duration, easing)`) needs a Rust-side animator
tied to the runtime tick.

**Proposed API** (sketch):

```rust
pub struct Animatable<T> { ... }

impl Animatable<f32> {
    pub fn new(initial: f32) -> Self;
    pub fn read(&self) -> ReadSignal<f32>;
    pub fn set_immediately(&self, v: f32);
    pub fn animate_to(&self, target: f32, duration: Duration, easing: Easing);
}
```

**Affected layouts**: `StackLayout` (slide-in / slide-out, pop
animation), `ModalLayout` (slide-up), `TabsLayout` (cross-fade
between panes if we add it).

**Required from runtime**: hook into the frame tick callback the
host already drives, plus a `Easing` enum (likely just
`Linear` / `EaseIn` / `EaseOut` / `EaseInOut` / `Spring` for v1).

## 🟠 2. `Owner::pause()` / `Owner::resume()` for screen freeze

**Why**: When a screen transitions to `EntryState::Suspended`
(beneath top of stack), its effects should pause so it doesn't
churn signals while invisible. RN's `enableFreeze` is the same
idea, retrofitted via Suspense.

**Proposed API**:

```rust
impl OwnerId {
    pub fn pause(self);   // stop firing effects under this owner
    pub fn resume(self);
}
```

**Alternatives considered**: a per-entry "active" signal that
every effect manually gates on. Rejected — error-prone and breaks
the implicit-reactivity model.

## 🟡 3. `on_global_event` accessible from package crates

**Why**: Deep-link Level 2 wires native-side `whisker_url` global
events into Whisker subscribers. The runtime has the
`GlobalEventEmitter` plumbing for `<lynx-view>` events, but it
isn't exposed as a `pub fn on_global_event(name, handler)` in
`whisker::event`.

**Current state**: `whisker-router/src/linking.rs` returns
`None` / `()` as a placeholder; the API shape is finalized but the
implementation is gated on the runtime export.

**Required from runtime**: a `pub fn on_global_event(name: &str,
handler: impl Fn(CustomEvent) + 'static) -> impl Drop`-ish guard.

## 🟡 4. Touch events in `render!` kwargs

**Why**: Pure-Whisker interactive pop gesture needs
`view(on_touchstart: …, on_touchmove: …, on_touchend: …,
on_touchcancel: …)` at the call site.

**Audit needed**: `whisker-runtime/src/event.rs` defines
`TouchEvent` and `bind_typed`, but I haven't confirmed which kwarg
names the `render!` builder surface accepts. Likely a small
addition in `whisker-macros/src/render.rs` (the `is_known_event_method`
list) + matching `on_touchstart` etc. methods on the universal
`ElementBuilder` trait in `whisker/src/lib.rs`.

**Risk**: if the names already exist under different identifiers
(e.g. `on_tap_start`), call sites need to match.

## 🟡 5. `#[whisker::route]` proc macro

**Why**: Hand-writing `Route::parse` / `to_path` per enum is
boilerplate-heavy, especially with nested groups and path
parameters. The proc macro generates both, plus the URL grammar
from `#[at("/profile/:id")]` attributes.

**Current state**: v1 ships without it — users write the impl by
hand, following the canonical example in
`whisker-router/src/route.rs` test module. Derive lands as a
follow-up RFC.

**Required from `whisker-macros`**: a new proc-macro-attribute (or
derive) entry point that parses the variant attrs (`#[at(...)]`,
`#[layout(LayoutComponent)]`) and emits the parser + builder +
optional `RouteLayout` impl.

## 🟡 6. `expect_context::<T>("reason")` helper

**Why**: `outlet::router::<R>()` currently uses
`use_context::<T>().expect("...")` inline. A canonical
`expect_context` would surface a better message and keep the
panic site in one place.

**Trivial addition** to `whisker-runtime/src/reactive/context.rs`,
re-exported through the prelude — punt until something else needs
the same pattern.

## Status snapshot

The router skeleton, `NavStack`, `Route` trait, `Outlet`,
`Router`, `BackHandler` chain, and a stub `StackLayout` are all in
the crate and tested without touching any other crate. Items 1, 2,
4 are blockers for the next implementation phase (interactive
gestures + transitions); items 3 and 5 are quality-of-life that
can wait until the surface is exercised by a real example.
