# Module API Design Guide

How to choose the user-facing shape for a new Whisker module crate
(`whisker-audio`, `whisker-image`, `whisker-notifications`, …) so it
slots into the rest of the framework instead of becoming a new
five-th paradigm.

This doc covers the **shape**: which Rust surface to expose to the
app author, and why. For the **mechanics** (how the crate is laid
out, and how Kotlin / Swift wire to Rust through the bridge), scaffold
a crate with `whisker new-module`, read the reference module
[`whisker-local-store`](../packages/whisker-local-store/), and see the
crate-graph / bridge overview in [`architecture.md`](./architecture.md).

> **Audience.** Authors of new module crates, and reviewers of new
> module PRs.

> **Distribution is crates.io-only.** A module is one ordinary crate
> published to crates.io. There is **no** SwiftPM Registry or Maven
> Central entry per module — the native Swift / Kotlin sources ship
> *inside the crate tarball* via the `include` list in `Cargo.toml`
> (`ios/**/*.swift`, `android/**/*.kt`, `Package.swift`,
> `build.gradle.kts`). The iOS / Android builds extract them from the
> downloaded crate. So your module's whole public contract — Rust
> surface *and* native code — lives behind a single `whisker = "0.1"`
> + `whisker-yourmodule = "0.1"` in the app's `Cargo.toml`.

---

## TL;DR

Whisker ships five surface shapes for platform-bridging APIs. Match
your module to the shape that fits its semantics and stop:

| Shape | Looks like | Example | When |
|---|---|---|---|
| 1. **Component** | `<Image src=… />` | `whisker-image`, `whisker-svg`, `whisker-icons` | Pure native UI widget with no imperative methods |
| 2. **Component + ref-bound handle** | `<Video ref:=h.r() />` + `h.play()` | `whisker-video` | UI widget that *also* needs imperative methods |
| 3. **Clone value-type handle** | `Player::new(url)` + `.play()` + `.status()` | `whisker-audio` | Native resource without a view, with lifetime + observable state |
| 4. **Free fn returning a signal** | `safe_area_insets() -> ReadSignal<…>` | `whisker-safe-area` | Singleton environment observable (viewport, network, battery) |
| 5. **Static methods / free fns** | `WhiskerLocalStore::save(k, v)` | `whisker-local-store` | Stateless one-shot operation |

The rest of this doc walks the decision and explains why each shape
exists.

---

## Decision flow

```
                 ┌───────────────────────────┐
                 │ Does it render visible    │
                 │ pixels of its own?        │
                 └───────────┬───────────────┘
                  yes        │           no
              ┌──────────────┘           └───────────────┐
              ▼                                          ▼
  ┌─────────────────────────┐               ┌──────────────────────┐
  │ Does it also need       │               │ Does it have a       │
  │ imperative methods      │               │ lifetime detached    │
  │ (play, focus, scrollTo)?│               │ from any UI element? │
  └────┬──────────────┬─────┘               └────┬─────────────┬──┘
   yes │              │ no                   yes │             │ no
       ▼              ▼                          ▼             ▼
  ┌─────────┐    ┌─────────┐         ┌──────────────────┐  ┌────────────────┐
  │ Shape 2 │    │ Shape 1 │         │ Does it expose   │  │ Is it a single │
  │  Comp + │    │  pure   │         │ observable state │  │ app-singleton  │
  │  ref:   │    │ Component│        │ worth reacting   │  │ observable?    │
  │ handle  │    └─────────┘         │ to?              │  └──┬──────────┬──┘
  └─────────┘                        └──┬───────────┬───┘ yes │       no │
                                     yes │         no │         ▼          ▼
                                         ▼            ▼     ┌─────────┐  ┌──────────┐
                                    ┌─────────┐   ┌──────┐  │ Shape 4 │  │ Shape 5  │
                                    │ Shape 3 │   │Shape5│  │ free fn │  │ static / │
                                    │  Clone  │   │static│  │ → signal│  │ free fn  │
                                    │ handle  │   │      │  └─────────┘  └──────────┘
                                    └─────────┘   └──────┘
```

In words:

1. **Render pixels?** → it's a component.
   1a. With imperative methods? Shape 2 (handle attached via `ref:`).
   1b. Pure declarative? Shape 1.
2. **Lifetime detached from UI?** → Shape 3 if observable, Shape 5
   if stateless.
3. **Process-wide singleton observable?** → Shape 4.

---

## The two macros, and the native side

Every shape below uses one of two Rust-side macros:

- `#[whisker::module_component("Name")]` — declares a **native view**
  element for `render!`. The Lynx tag is `<crate-name>:Name` (the crate
  name is auto-prepended). Shapes 1 and 2 use it.
- `whisker::module!("Name")` — resolves a **function module** handle
  (`PlatformModule`) you `.invoke(method, args)` / `.on_event(...)` on.
  Shapes 3, 4 and 5 build their typed wrappers over it. The name is
  likewise crate-namespaced to `<crate>:Name`.

There is **no** `#[whisker::platform_module]` attribute — function
modules are plain Rust wrappers over `module!`, not macro-generated.

The matching **native** half (Swift / Kotlin) is written with the
`definition()` ModuleDefinition DSL — *not* annotations. A module
subclass overrides `definition()` and returns a `ModuleDefinition {
Name(...); View(...); Prop(...); Function(...) }` block; the
per-platform codegen plugin discovers the subclass and registers a
dispatch shim under the crate-namespaced key that `module!` /
`module_component` resolve to. (The older `@WhiskerModule` /
`@WhiskerComponent` / `@WhiskerProp` annotation framing is gone.)

```swift
// iOS — packages/whisker-local-store/ios/.../LocalStoreModule.swift
public final class LocalStoreModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerLocalStore")
            Function("save") { (args: [WhiskerValue]) -> WhiskerValue in /* … */ }
            // view modules add `View(WhiskerImageView.self)` + `Prop("src")` here
        }
    }
}
```

---

## Shape 1 — Pure Component

```rust
#[whisker::module_component("Image")]
pub fn image(src: Signal<String>, mode: Signal<ImageMode>, style: Signal<String>) {}
```

```rust
// Usage
render! {
    Image(src: "https://…", mode: ImageMode::AspectFill, style: "width: 200px")
}
```

**When:** the module renders a native UI widget whose state is fully
captured by its props. The app never needs to call methods on it
imperatively — re-render with different props is enough.

**Why a component:** the platform widget needs to live inside the
element tree (so it inherits layout, gets sized by flex, paints in
the right z-order). Props are `Signal<T>`, so a reactive `src` swap
re-fetches the image without a remount.

**Lifetime:** tied to the parent owner. Unmount happens when the
parent component disposes, cascading through Whisker's reactive
scope tree.

**Reactive:** props are individually reactive. No `status()` signal —
there's nothing to observe.

**Examples:** `whisker-image::Image`, `whisker-svg::Svg`,
`whisker-icons::Icon`.

---

## Shape 2 — Component + ref-bound handle

```rust
#[whisker::module_component("Video")]
pub fn video(src: Signal<String>, style: Signal<String>) {}

#[derive(Copy, Clone)]
pub struct VideoHandle { r: ElementRef }

impl VideoHandle {
    pub fn new() -> Self;
    pub fn r(&self) -> ElementRef;     // attach via `Video(ref: handle.r())`
    pub fn play(&self);
    pub fn pause(&self);
    pub fn seek(&self, secs: f64);
}
```

```rust
// Usage
let video = VideoHandle::new();
render! {
    view {
        Video(ref: video.r(), src: "clip.mp4", style: "height: 200px")
        view(on_tap: move |_| video.play()) { text(value: "play") }
    }
}
```

**When:** the module renders a native UI widget *and* needs imperative
methods (`play`, `pause`, `seek`, `focus`, `scrollTo`, …). These are
actions the user triggers in response to a tap or other event — not
declarative state that a re-render can capture.

**Why a separate handle:** the methods need to dispatch through the
mounted element. Whisker exposes `ElementRef` for exactly this
shape — the handle is `Copy` (just a slotmap key) so it copies freely
into `on_tap` closures.

**Lifetime:** the *handle* is just a key; constructing one before the
element mounts is fine (methods no-op until the `ref:` binds). The
*native player* tracks the mounted element and is released when the
element is unmounted by its owner.

**Reactive:** *optional* — a Shape-2 module can expose a `status()`
reactive signal if it has observable state worth binding (see
[#128](https://github.com/whiskerrs/whisker/issues/128) for
`whisker-video`'s pending decision on this).

**Examples:** `whisker-video::Video` / `VideoHandle`.

---

## Shape 3 — Clone value-type handle

```rust
#[derive(Clone)]
pub struct Player { /* Rc-backed */ }

impl Player {
    pub fn new(source: impl Into<String>) -> Self;
    pub fn play(&self);
    pub fn pause(&self);
    pub fn seek_to(&self, s: f64);
    pub fn status(&self) -> ReadSignal<PlaybackStatus>;
    // …
}
```

```rust
// Usage
let player = Player::new("clip.mp3");
let status = player.status();
render! {
    text(value: move || format!("{:.1}s", status.get().position))
    view(on_tap: { let p = player.clone(); move |_| p.play() }) {
        text(value: "play")
    }
}
```

**When:** the module owns a native resource (audio player, network
socket, watchdog timer) that:

1. has no visual representation of its own (no element in the tree),
2. has a lifetime independent of any one UI element (can outlive a
   tab change, can be shared between two unrelated views),
3. has observable state worth binding (`position`, `is_playing`,
   connection state, etc.).

**Why a Clone handle:** the native resource needs a Rust-side owner
to release it. `Clone` lets the handle live in multiple closures
without back-channel coordination. Internally it's `Rc<Inner>` so
the native side releases on the last drop.

**Lifetime:** ref-counted on the Rust side. The native resource
releases when the last `Player` clone drops — typically when the
component that owns the source clone unmounts and the scope
disposes.

**Reactive:** `status() -> ReadSignal<T>` is the canonical entry
point. The signal is process-global, dispatched per-player-id; all
clones of the same handle see the same signal.

**Examples:** `whisker-audio::Player`.

---

## Shape 4 — Free fn returning a signal

```rust
pub fn safe_area_insets() -> ReadSignal<SafeAreaInsets>;

#[derive(Clone, Copy, Debug)]
pub struct SafeAreaInsets { pub top: f64, pub leading: f64, /* … */ }
```

```rust
// Usage
let insets = safe_area_insets();
let padding_style = computed(move || {
    format!("padding-top: {}px", insets.get().top as i32)
});
render! { view(style: padding_style) { … } }
```

**When:** the value is a **singleton observable** for the whole
process — environment metadata that has one canonical answer at any
moment (safe-area insets, keyboard height, network reachability,
battery level, dark-mode state, …). The app doesn't construct it; it
just *observes* it.

**Why a free function:** there's nothing to construct, name, or
keep distinct copies of. A `Hook`-style call is the most direct
expression.

**Lifetime:** the underlying signal is process-global, lazily
initialised on the first call via `OnceLock`. It stays alive for the
process — never disposed.

**Reactive:** *the whole point* — the function returns a
`ReadSignal<T>` directly.

**Examples:** `whisker-safe-area::safe_area_insets`.

---

## Shape 5 — Static methods / free fns

```rust
pub struct WhiskerLocalStore;

impl WhiskerLocalStore {
    pub fn save(key: String, value: String) -> Result<bool, WhiskerModuleError>;
    pub fn load(key: String) -> Result<Option<String>, WhiskerModuleError>;
    pub fn remove(key: String) -> Result<(), WhiskerModuleError>;
}
```

```rust
// Usage
let _ = WhiskerLocalStore::save("user_id".into(), "abc".into())?;
let loaded = WhiskerLocalStore::load("user_id".into())?;
```

**When:** the operation is **stateless and one-shot** — fire it,
get a `Result`, done. No identity to carry across calls. No
observation worth binding.

**Why static methods (or free functions):** the unit struct
`WhiskerLocalStore` is a namespace, nothing more. The functions
could be free `whisker_local_store::save(…)` — it's a style
preference; the unit-struct form lets the rustdoc page render the
operations grouped under one heading.

**Lifetime:** none.

**Reactive:** none. If the caller wants to *react* to the underlying
state changing (e.g. someone else wrote to the same key), they
should poll via a `resource()` — but that's the caller's concern,
not the module's.

**Examples:** `whisker-local-store::WhiskerLocalStore::{save, load,
remove}`.

---

## Why these five and not "one true shape"

Whisker is **a reactive Rust runtime atop a native UI layer**. Each
shape exists because the underlying constraint shape is different:

| Constraint | Implication |
|---|---|
| Native UI widgets must live in the element tree | Shape 1 / 2 — they have to be components |
| Native resources have lifetime, observable state, and Rust-side ownership | Shape 3 — Rust handles + `Rc` are the right primitive |
| Some natives are singleton process-wide observables | Shape 4 — a free fn returning a signal removes ceremony |
| Some operations are pure side-effecting calls | Shape 5 — wrapping them in a handle would be noise |

### Why not "everything is a component" (React-style)

React fits when the platform composes from a single primitive — DOM
nodes. Whisker bridges to native widgets *plus* native resources
*plus* environment observables. Forcing audio playback into a
`<Player src=…>` mount means:

- The `Player` lifetime is now tied to where in the tree it's mounted
  — moving the mount point unmounts + remounts (re-fetch, lost state).
- The handle has to be passed back out via `ref:` (Shape 2), which is
  exactly Shape 3 plus an irrelevant element.
- Sharing one player between two tabs becomes a context-provider
  ceremony.

### Why not "everything is a hook" (React-with-only-hooks-style)

Hooks fit singleton observables (Shape 4). For multi-instance
resources (two players running in parallel, two open camera
sessions) you need value identity — distinct `Player` handles. Hooks
return values keyed on call-site, not value-keyed, so they don't
naturally compose for the multi-instance case.

### Why not "everything is a free fn" (Solid-without-Owner style)

Free functions work for Shape 4 / 5 but lose lifetime semantics for
Shape 3. `play()` and `pause()` need to know *which* player —
that's a handle, not a function.

The five shapes are the *minimum* set; anything fewer and one of
those constraint axes gets paved over.

---

## Anti-patterns

When reviewing a new module crate, flag these:

### Anti-pattern A: Component for what's actually Shape 3

> "Let's mount a `<Audio src='clip.mp3' is_playing=true />` and toggle
> `is_playing` via a signal."

Symptoms: the prop set keeps growing (`is_playing`, `position_seek`,
`volume`, …); each one needs back-channel coordination to know when
to fire. Lifetime is tied to the mount point, so unmounting breaks
playback.

Fix: make it Shape 3 (`Player::new + play() + pause()`).

### Anti-pattern B: Shape 4 for multi-instance state

> "`use_player(url)` returns a `ReadSignal<PlaybackStatus>` for that
> URL."

Symptoms: re-calling `use_player` with a new URL is ambiguous —
new player, or new subscriber to the same one? Two components want
two different players; can the hook give them distinct instances?

Fix: make it Shape 3 (value identity is in the handle).

### Anti-pattern C: Shape 3 for stateless ops

> "`let store = LocalStore::new(); store.save('k', 'v');`"

Symptoms: `new` doesn't do anything meaningful; the user has to
think about lifetime where there is none.

Fix: make it Shape 5 (`WhiskerLocalStore::save`).

### Anti-pattern D: Shape 5 for observable state

> "`get_safe_area_insets() -> SafeAreaInsets` — call it again to get
> a fresh value."

Symptoms: callers poll in a `resource` or `effect`, races with
native event delivery, miss updates.

Fix: make it Shape 4 (return a `ReadSignal<T>` so updates are
push-based).

### Anti-pattern E: Mixing shapes within one module

A module that exposes *both* `Player::new(url)` *and* a
`<Player src=…>` element is two APIs for the same thing — confusing
and prone to drift. Pick one.

---

## Forward references

- Concrete how-to for wiring Kotlin / Swift / Rust together →
  scaffold with `whisker new-module`, then crib from the reference
  module [`whisker-local-store`](../packages/whisker-local-store/)
  (function module) or [`whisker-image`](../packages/whisker-image/)
  (view module); crate-graph + bridge overview in
  [`architecture.md`](./architecture.md).
- `whisker-video` reactive surface decision →
  [issue #128](https://github.com/whiskerrs/whisker/issues/128)
- Why `Owner` (Shape 3 mechanics) and what backs lifetime →
  [`reactivity-design.md`](./reactivity-design.md)
