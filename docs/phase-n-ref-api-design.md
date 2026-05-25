# Phase N design: `ElementRef` + `Handle` API standardization

**Status**: design (no implementation yet). Tracking issue: #60. Parent epic: #55.

This doc proposes the imperative-control layer that all of Whisker's
"call a method on a mounted thing" use cases build on (`videoHandle.play()`,
`textInputHandle.focus()`, `modalHandle.open()`, application-level composite
handles).

The design grew out of the Phase L discussion (#58). After a few iterations
the conclusion was that **a single "Ref" abstraction conflates two very
different things**, and splitting them makes the whole picture much simpler:

| Layer | Name | Role | Who writes it |
|---|---|---|---|
| **FFI primitive** | `ElementRef` | Receives a mounted element's id; exposes `invoke()` to call platform methods | Framework |
| **User-facing imperative API** | `XxxHandle` (`VideoHandle`, `ModalHandle`, …) | Plain `Clone` struct, internally `RwSignal<...>` for commands and reactive `ReadSignal<T>` for queries. End-users call methods on it. | Module / app authors |

Handles are pure-Rust signal-backed structs. There is **no `WhiskerRef`
trait, no derive, no auto-bind**. The connection from a `Handle` down to
an `ElementRef` (and from there to native) happens inside the
`#[whisker::component]` that wraps the platform component, written by
the module author as ordinary `effect(...)` blocks.

This design fully embraces Whisker's signal-first philosophy: the
imperative-feeling user surface (`handle.play()`) is just sugar over
"write to a signal", and the bridge to native is one more `effect(...)`
subscribing to that signal.

## 1. Primitive: `ElementRef`

A small framework-provided type. Module authors use it when wiring up
their platform-component wrappers; end-users normally never see it.

```rust
// crates/whisker-modules-api
use whisker::reactive::{RwSignal, Signal, signal};

#[derive(Clone)]
pub struct ElementRef {
    // Single source of truth: holds the bound Element (None while
    // unmounted) and is reactive so `bound()` can be observed.
    inner: RwSignal<Option<Element>>,
}

impl ElementRef {
    pub fn new() -> Self {
        Self { inner: signal(None) }
    }

    /// Run a platform method through the C bridge. Returns `Err` when
    /// the ref isn't currently bound to a mounted element, or the
    /// platform side surfaces a `WhiskerValue::Err`.
    pub fn invoke(
        &self,
        method: &'static str,
        args: Vec<WhiskerValue>,
    ) -> Result<WhiskerValue, RefError> {
        // get_untracked: `invoke` is imperative and must not subscribe
        // its caller to the binding signal.
        let elem = self.inner.get_untracked().ok_or(RefError::NotBound)?;
        match whisker_bridge_invoke_element_method(elem, method, args) {
            WhiskerValue::Err(msg) => Err(RefError::DispatchFailed {
                method: method.into(),
                message: msg,
            }),
            v => Ok(v),
        }
    }

    /// Ergonomic typed wrapper around `invoke`.
    pub fn invoke_typed<T>(
        &self,
        method: &'static str,
        args: Vec<WhiskerValue>,
    ) -> Result<T, RefError>
    where
        T: TryFrom<WhiskerValue, Error = String>,
    {
        let v = self.invoke(method, args)?;
        T::try_from(v).map_err(|message| RefError::DispatchFailed {
            method: method.into(),
            message,
        })
    }

    /// `true` iff bound to a live element right now. Non-reactive read.
    pub fn is_bound(&self) -> bool {
        self.inner.get_untracked().is_some()
    }

    /// Reactive: subscribe inside `effect(...)` / `computed(...)` to
    /// react to mount / unmount events.
    pub fn bound(&self) -> Signal<bool> {
        let inner = self.inner;
        Signal::derive(move || inner.get().is_some())
    }

    /// Framework-internal: `#[platform_component]` macro calls this on mount.
    #[doc(hidden)]
    pub fn __bind(&self, element: Element) {
        self.inner.set(Some(element));
    }

    /// Framework-internal: unmount sentinel.
    #[doc(hidden)]
    pub fn __unbind(&self) {
        self.inner.set(None);
    }
}
```

Key properties:

- **Single source of truth.** Both the binding state and reactive
  observation flow from one `RwSignal<Option<Element>>`. There is no
  separate `Cell` to keep in sync.
- **`Clone` is cheap.** `RwSignal<T>` is internally `Copy`-handle to
  an arena slot, so passing `ElementRef` to multiple closures is free.
- **`bound()` is reactive, `is_bound()` is not.** Hot-path `invoke()`
  uses `get_untracked()` to avoid accidentally subscribing the caller
  to the binding signal. Authors wanting to react to mount/unmount
  read `bound()` from inside an `effect`.
- **Where `ElementRef` is bound.** Only `#[platform_component]` macros
  bind `ElementRef` (to the underlying native element they create).
  `#[component]` (custom components) never auto-binds anything —
  there is no element to bind to in a pure Rust component.

### `ElementRef` is module-author surface, not app surface

App authors don't pass `ElementRef` around. They construct a `Handle`
(see §2 / §3), pass it to a component, and call methods on it. The
`ElementRef` lives internally in the platform-component wrapper that
the module author wrote.

## 2. User-facing `Handle` pattern

A **Handle** is whatever struct the module / app author defines to give
end-users an imperative-feeling API for a component. There is **no
trait** to implement, no `derive`, no Whisker-specific macro: a Handle
is just a `Clone`-able plain Rust struct whose fields are reactive
signals.

The conventions:

- Field name suffix: `XxxHandle` (`VideoHandle`, `ModalHandle`, …).
- All command / state fields are `RwSignal<T>`.
- Methods are sugar: a method either writes a signal, reads a signal,
  or composes child-handle methods.
- The wrapping `#[whisker::component]` bridges the handle's command
  signals to native via `ElementRef` (only needed when the handle
  controls a native element).

### Canonical example: `VideoHandle`

```rust
use std::cell::Cell;
use whisker::reactive::{RwSignal, Signal, signal};

#[derive(Clone)]
pub struct VideoHandle {
    // --- command signals (write side) ---
    // Counter pattern: increment to dispatch one play.
    play_tick:  RwSignal<u64>,
    pause_tick: RwSignal<u64>,
    // Argument-bearing one-shot: seek-to-position.
    seek_to:    RwSignal<Option<f64>>,

    // --- query signals (read side; the bridge writes, users read) ---
    current_time: RwSignal<f64>,
    duration:     RwSignal<f64>,
}

impl VideoHandle {
    pub fn new() -> Self {
        Self {
            play_tick: signal(0),
            pause_tick: signal(0),
            seek_to: signal(None),
            current_time: signal(0.0),
            duration: signal(0.0),
        }
    }

    // ---- commands: write a signal ----
    pub fn play(&self)         { self.play_tick.update(|n| *n += 1); }
    pub fn pause(&self)        { self.pause_tick.update(|n| *n += 1); }
    pub fn seek(&self, t: f64) { self.seek_to.set(Some(t)); }

    // ---- queries: hand out a ReadSignal ----
    pub fn current_time(&self) -> Signal<f64> { self.current_time.read_only().into() }
    pub fn duration(&self)     -> Signal<f64> { self.duration.read_only().into() }
}
```

End-user code:

```rust
let v = VideoHandle::new();
let progress = v.current_time();           // Signal<f64> — reactive everywhere

let v_play = v.clone();
let v_restart = v.clone();

render! {
    Video(handle: v.clone(), src: signal("video.mp4".into()))
    text(value: move || format!("{:.1}s", progress.get()))
    text(on_tap: move || v_play.play(),               value: "▶")
    text(on_tap: move || v_restart.seek(0.0),         value: "⏮")
}
```

### Canonical example: `ModalHandle` (no native bridge needed)

For custom components that don't talk to native, the handle is
*entirely* signal-based and there is no `ElementRef` anywhere.

```rust
#[derive(Clone)]
pub struct ModalHandle {
    open: RwSignal<bool>,
}

impl ModalHandle {
    pub fn new() -> Self { Self { open: signal(false) } }
    pub fn open(&self)   { self.open.set(true); }
    pub fn close(&self)  { self.open.set(false); }
    pub fn toggle(&self) { self.open.update(|o| *o = !*o); }
    pub fn is_open(&self) -> Signal<bool> { self.open.read_only().into() }
}
```

```rust
#[whisker::component]
pub fn modal(handle: ModalHandle, children: Children) -> Element {
    render! {
        show!(handle.is_open(), {
            view(class: "modal-backdrop", on_tap: {
                let h = handle.clone();
                move || h.close()
            }) {
                view(class: "modal-content") { children() }
            }
        })
    }
}
```

`modal` is just a custom `#[component]` — no `ElementRef`, no
framework-level binding logic, no `WhiskerRef` trait. The component
body subscribes to `handle.is_open()` via `show!`, and the user calls
`handle.open()` to flip the signal.

## 3. Bridging a `Handle` to an `ElementRef`

When a Handle controls a native element (like `VideoHandle` driving the
`<video>` element), the **wrapping `#[whisker::component]` builds the
bridge** by hand using `effect(...)`. Whisker provides no framework-level
auto-bridging — the boundary is explicit so authors can see and tune it.

```rust
use std::cell::Cell;
use whisker::reactive::effect;

#[whisker::component]
pub fn video(handle: VideoHandle, src: Signal<String>) -> Element {
    // Internal: ElementRef bound by `#[platform_component]` on mount.
    let sys = ElementRef::new();

    // --- command bridges (write side: signal -> invoke) ---

    // Counter pattern needs first-fire suppression (effects run once on
    // creation; the signal starts at 0, so without this guard the
    // component would call `play()` on every mount).
    effect({
        let sys = sys.clone();
        let h = handle.clone();
        let first = Cell::new(true);
        move || {
            h.play_tick.get();                  // subscribe
            if first.replace(false) { return; }
            let _ = sys.invoke("play", vec![]);
        }
    });
    effect({
        let sys = sys.clone();
        let h = handle.clone();
        let first = Cell::new(true);
        move || {
            h.pause_tick.get();
            if first.replace(false) { return; }
            let _ = sys.invoke("pause", vec![]);
        }
    });

    // Option pattern: dispatch and reset to None so re-setting the
    // same value re-dispatches.
    effect({
        let sys = sys.clone();
        let h = handle.clone();
        move || {
            if let Some(t) = h.seek_to.get() {
                let _ = sys.invoke("seek", vec![WhiskerValue::Float(t)]);
                h.seek_to.set(None);
            }
        }
    });

    // --- query bridges (read side: native event -> signal) ---

    let on_time_update = {
        let h = handle.clone();
        move |e: WhiskerEvent| {
            if let Some(t) = e.get_float("currentTime") { h.current_time.set(t); }
        }
    };
    let on_loaded_metadata = {
        let h = handle.clone();
        move |e: WhiskerEvent| {
            if let Some(d) = e.get_float("duration") { h.duration.set(d); }
        }
    };

    render! {
        VideoSys(
            ref: sys.clone(),
            src: src,
            on_time_update: on_time_update,
            on_loaded_metadata: on_loaded_metadata,
        )
    }
}
```

Conceptually: a Handle is a **signal-shaped specification of an
imperative API**, and the wrapper component is the **interpreter** that
turns those signal changes into native calls + native events back into
signal writes.

## 4. Command signal patterns

Four idiomatic shapes covering ~all imperative use cases:

| Use case | Signal shape | Method body |
|---|---|---|
| Idempotent toggle (`open`, `close`, `focus`) | `RwSignal<bool>` | `open.set(true)` |
| Re-firable side effect (`play`, `toast.show`) | `RwSignal<u64>` counter | `n.update(|x| *x += 1)` |
| Argument-bearing one-shot (`seek(t)`, `scroll_to(y)`) | `RwSignal<Option<T>>` | `s.set(Some(t))` — bridge resets to `None` after dispatch |
| Ordered queue (`enqueue(op)` series) | `RwSignal<Vec<Cmd>>` | `q.update(|v| v.push(c))` — bridge drains in `effect` |

The bridge `effect`s match the signal shape:

- `bool` → subscribe, dispatch when value goes high (use first-fire
  guard or pattern-match on the new value).
- counter → subscribe, dispatch on every change except the initial
  effect creation (first-fire guard via `Cell<bool>`).
- `Option<T>` → match `Some(t)`, dispatch, reset to `None`.
- `Vec<Cmd>` → match non-empty, drain, dispatch each.

These four patterns are documented as the standard idioms. Authors can
combine them in a single Handle (e.g. `VideoHandle` mixes counters and
`Option`).

## 5. Composite Handles

App-level handles compose framework / module handles as plain struct
fields. No special Whisker support is needed because Handles are just
Rust structs.

### Wrapping a built-in (forward methods)

```rust
#[derive(Clone)]
pub struct CustomInputHandle {
    text_input: TextInputHandle,    // built-in handle from whisker
    // could add internal signals too:
    //   error: RwSignal<Option<String>>,
}

impl CustomInputHandle {
    pub fn new() -> Self {
        Self { text_input: TextInputHandle::new() }
    }

    // Forwarders — plain Rust:
    pub fn focus(&self) { self.text_input.focus(); }
    pub fn blur(&self)  { self.text_input.blur(); }

    // Compositions — plain Rust:
    pub fn focus_and_select_all(&self) {
        self.text_input.focus();
        self.text_input.select_all();
    }

    pub fn value(&self) -> Signal<String> { self.text_input.value() }
}
```

```rust
#[whisker::component]
pub fn custom_input(handle: CustomInputHandle, label: Signal<String>) -> Element {
    let t = handle.text_input.clone();   // share — same Rc-backed signals
    render! {
        view {
            text(value: label)
            TextInput(handle: t)
        }
    }
}
```

Cloning `handle.text_input` shares the same backing signals, so calling
`handle.focus()` on the outer `CustomInputHandle` propagates through the
inner `TextInputHandle` → into the `TextInput`'s wrapper component's
bridge `effect` → `sys.invoke("focus", _)`.

### Mixed handles (signal-only + native-bridge)

A `TooltipHandle` could combine its own `RwSignal<bool>` (visibility) with
a child `ViewHandle` (for positioning calls). All composition is plain
struct fields — no Whisker awareness needed.

### Three layers deep is the same

```rust
#[derive(Clone)]
pub struct FormHandle {
    email: CustomInputHandle,
    password: CustomInputHandle,
}

impl FormHandle {
    pub fn new() -> Self {
        Self {
            email: CustomInputHandle::new(),
            password: CustomInputHandle::new(),
        }
    }
    pub fn focus_email(&self) {
        self.email.focus();
        // → TextInputHandle.focus → focus_tick.update
        // → wrapping `text_input` component's bridge effect
        // → sys.invoke("focus", _)
        // → C bridge → Lynx → native EditText / UITextField
    }
}
```

## 6. Lifetime / unmount semantics

Handles **do not have a binding state**. They are pure-signal structs
that live as long as the `Rc` graph keeps them alive. Calling
`handle.play()` on an unmounted component just writes to a signal — the
bridge `effect` may or may not still be alive:

- **If the bridge is still alive** (component is mounted): the effect
  fires, calls `sys.invoke("play")`. If `sys` happens to be momentarily
  unbound, `invoke` returns `Err(RefError::NotBound)` and the effect
  swallows it (since the bridge writes `let _ = sys.invoke(...)`).
- **If the bridge has been disposed** (component unmounted, owner
  dropped): the signal write is observed by nothing — silently no-ops.

So the unmount handling is:

- **`ElementRef`** is the only thing that carries explicit "bound /
  unbound" state. `invoke()` returns `Result<_, RefError::NotBound>`.
- **`Handle`** never returns errors. Calling `handle.play()` after
  unmount is always silently a no-op, which matches what end-users want
  for fire-and-forget UI handlers.

This is a strict improvement over the earlier `RefHandle + WhiskerRef`
design: the lifetime-from-signal-system handles cleanup automatically,
and users never need to write `try_*` paired methods or unwrap
`Result<_, RefError>` in `on_tap` closures.

## 7. Where this leaves `Signal<T>`

Signals stay the foundation. Handles **are** sugar over signals. The
decision table:

| Use case | Mechanism |
|---|---|
| Stateful value (form input value, scroll position, selection, …) | `Signal<T>` / `RwSignal<T>` directly, or wrapped in a Handle |
| Derived state | `computed(...)` / `effect(...)` |
| Imperative command on a custom component (`modal.open()`, `drawer.toggle()`) | `XxxHandle` (signal-backed, no native bridge) |
| Imperative command that talks to native (`video.play()`, `textInput.focus()`) | `XxxHandle` (signal-backed) + wrapper component with `ElementRef` bridge |
| Reactive read of native state (`video.current_time()`, `textInput.value()`) | `Handle` exposes `Signal<T>`; wrapper pushes native events into the signal |
| Events fired from native back to Rust (`on_completed`, `on_change`, …) | Callback props on the platform component (unchanged) |

In contrast to typical React refs, **Whisker has no "current value"
sync read API** — every "reading platform state" goes through a signal.
This is the right call for a signal-first framework: any read can
power a `text(value: ...)` or an `effect(|| ...)`. If a one-shot
non-reactive read is genuinely needed (rare), `Signal::get_untracked()`
is the escape hatch.

## 8. Migration from today's `ElementRef<T>` / `#[element_methods]`

The current implementation has `ElementRef<T>` doing the work of both
Phase N's `ElementRef` and `Handle`, with `#[whisker::element_methods]`
generating typed-wrapper traits. Phase N splits this:

| Today | Phase N |
|---|---|
| `ElementRef<VideoProps>` | `VideoHandle` (plain struct + impl) |
| `element_ref::<VideoProps>()` | `VideoHandle::new()` |
| `trait VideoSys` + `trait VideoControls` + `impl VideoControls for ElementRef<VideoProps>` | `impl VideoHandle { fn play(&self) { ... } }` |
| `#[whisker::element_methods]` proc macro | Deleted — authors write the impl block by hand |
| Implicit "this ref binds to a native element" | Explicit: module author wires a wrapper `#[whisker::component]` that owns an internal `ElementRef` and bridges signals to it |

Transitional `ElementRef<T> = ElementRef` alias (with the generic param
ignored) can stay during the deprecation window so existing module
crates keep compiling. The proc macro is deleted; existing
`#[element_methods]` annotations become a compile-time `unknown
attribute` error, prompting authors to migrate.

## 9. Open / deferred questions

- **Bridge boilerplate helper.** Per-command `effect(..)` blocks with
  first-fire guards are tedious. A future ergonomics layer (e.g. a
  `whisker::bridge!` declarative DSL or a `HandleBridge` trait) could
  generate them, but Phase N intentionally ships only the primitives.
- **Initial-fire suppression.** The counter pattern needs an
  `if first.replace(false) { return; }` line. We could add a
  `whisker::reactive::effect_skip_initial` helper but it's not strictly
  necessary; leaving as a documented idiom for now.
- **Type conversions in `invoke_typed`.** `TryFrom<WhiskerValue,
  Error = String>` is the conventional shape; the actual bound may need
  tweaking once we see ergonomics in practice.
- **`new()` taking arguments.** A handle with required init parameters
  (e.g. `VideoHandle::with_initial_volume(0.5)`) is just a regular Rust
  constructor on top of `new()`. No framework support needed.
- **Handle through `provide_context` / `use_context`.** Handles are
  `Clone`, so passing through context is `clone()`. No special
  handling.
- **SSR / no-element environments.** Bridges simply never fire
  (`sys.invoke` returns `NotBound`, swallowed by `let _ =`). Out of
  scope for Phase N.

## 10. Phasing

| Step | Scope |
|---|---|
| **N-1** | Add `ElementRef`, `RefError`, `WhiskerValue::try_into_*` helpers to `whisker-modules-api`. Existing `RefHandle` aliased. |
| **N-2** | `#[platform_component]` macro recognizes `ref:` props of type `ElementRef` and emits mount/unmount `__bind` / `__unbind` calls. `#[component]` no longer treats `ref:` specially — Handle props are ordinary `Clone` props. |
| **N-3** | Migrate `whisker-video`, `whisker-hello-element`, `whisker-local-store` to the `VideoHandle` + wrapper-component + bridge-effects pattern. |
| **N-4** | Built-in handles: `TextInputHandle`, `ScrollViewHandle`, `ViewHandle`. Update built-in platform components to expose them. |
| **N-5** | Deprecate `ElementRef<T>` alias + delete `#[whisker::element_methods]`. Land removal in a separate release window (Phase M follow-up). |

## 11. Estimated work

| Sub-task | Estimate |
|---|---|
| N-1 + N-2 (primitives + macro plumbing) | 2 days |
| N-3 (migrate three reference modules) | 1 day (more than the old design because of bridge code, less because no `WhiskerRef` impl) |
| N-4 (built-ins) | 1 day |
| N-5 (deprecation cycle) | 0.5 day after deprecation window |
| Total | ~3.5–4.5 days, plus the Phase M-style deprecation runway |

The framework surface is intentionally small (`ElementRef` + one error
enum). The Handle layer is fully in user-space — module authors write
plain Rust structs, and the bridge between Handle ↔ native lives inside
the wrapper component using ordinary `effect(...)` calls. No new traits
to implement, no derives, no auto-binding.
