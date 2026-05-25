# Phase N design: `Ref` API standardization

**Status**: design (no implementation yet). Tracking issue: #60. Parent epic: #55.

This doc proposes the standardized Ref API that all Whisker imperative
handles (`TextInputRef`, `ScrollViewRef`, `VideoRef`, custom-component
refs that compose them) build on top of.

The design grew out of the Phase L discussion (#58) where the question
"how does a Rust author declare ref methods on a platform component?"
expanded into a broader question about whether Whisker should have any
ref API at all (vs leaning entirely on signals). The answer settled on:

- **State** lives in `Signal<T>` / `RwSignal<T>` — Leptos / SolidJS style
  controller pattern for everything stateful.
- **Commands** (FFI dispatches, focus / scroll / play / pause, etc.)
  go through a thin, standardized Ref API. Refs are needed where
  signals don't model the operation cleanly.

Phase N standardizes the bottom layer (one primitive `RefHandle` +
`WhiskerRef` trait + `RefError` enum). Above that, every typed ref
(`TextInputRef`, `VideoRef`, application-level `CustomInputRef`, …)
is **plain Rust struct + impl** with no Whisker-specific macro support
needed.

## 1. Primitive: `RefHandle`

The single FFI-dispatch primitive every typed ref builds on.

```rust
// crates/whisker-modules-api (or whisker)
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone)]
pub struct RefHandle {
    inner: Rc<Cell<Option<Element>>>,
}

impl RefHandle {
    pub fn new() -> Self {
        Self { inner: Rc::new(Cell::new(None)) }
    }

    /// Run a platform method through the C bridge. Returns `Err` when
    /// the ref isn't currently bound to a mounted element, or the
    /// platform side surfaces a `WhiskerValue::Err`.
    pub fn invoke(
        &self,
        method: &'static str,
        args: Vec<WhiskerValue>,
    ) -> Result<WhiskerValue, RefError> {
        let elem = self.inner.get().ok_or(RefError::NotBound)?;
        match whisker_bridge_invoke_element_method(elem, method, args) {
            WhiskerValue::Err(msg) => Err(RefError::DispatchFailed {
                method: method.into(),
                message: msg,
            }),
            v => Ok(v),
        }
    }

    pub fn is_bound(&self) -> bool {
        self.inner.get().is_some()
    }

    /// Framework-internal: render! / #[component] / #[platform_component]
    /// macros call this on mount. Authors should not call it.
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

The `Rc<Cell<Option<Element>>>` shape gives `RefHandle` three
properties at once:

- **`Clone` is cheap** — multiple closures can hold their own copy.
  All clones point to the same `Rc`, so mount/unmount is visible
  through every clone simultaneously.
- **No mutable borrow needed for invoke** — `Cell::get` on a `Copy`
  payload (`Option<Element>` is `Copy` because `Element` is a small
  newtype) lets `invoke(&self, …)` work without `&mut self`.
- **Runtime-checked binding** — the `Option` distinguishes "haven't
  mounted yet" / "unmounted" from a real `Element`.

## 2. Trait: `WhiskerRef`

The opt-in marker for "this type is a Ref the framework can bind to a
mounted element."

```rust
pub trait WhiskerRef {
    /// The underlying handle the framework binds on mount.
    fn handle(&self) -> &RefHandle;
}
```

Just one method. Authors implement it on their `XxxRef` structs so
the `#[component]` / `#[platform_component]` macros can find the
`RefHandle` they own and wire up mount-time binding.

Composite custom refs (`CustomInputRef`, `FormRef` — see §5) do **not**
implement `WhiskerRef` because they don't bind to a single Lynx
element. They compose `WhiskerRef`-implementing inner refs and the
binding happens through those.

## 3. Errors: `RefError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum RefError {
    /// Ref isn't bound to a mounted element. Either the component
    /// hasn't been rendered yet, or it has unmounted.
    #[error("ref is not bound to a mounted element")]
    NotBound,

    /// Platform side surfaced a dispatch error
    /// (method not found, exception thrown, type mismatch, …).
    #[error("platform method `{method}` failed: {message}")]
    DispatchFailed { method: String, message: String },
}
```

The two-variant split lets callers distinguish "haven't been mounted
yet" (often a UI race the caller should silently ignore) from "the
platform side broke" (often a bug worth surfacing).

For the common fire-and-forget UI handler case, each typed ref
provides a paired strict / lenient method (see §4).

## 4. Typed refs: plain struct + impl

Every typed ref Whisker / a module crate / an app crate exposes is a
**regular Rust struct that owns a `RefHandle` and implements
`WhiskerRef`**. No macro, no special syntax, no DSL.

Example — `VideoRef` shipped by `whisker-video`:

```rust
use whisker::{RefError, RefHandle, WhiskerRef, WhiskerValue};

#[derive(Clone)]
pub struct VideoRef {
    handle: RefHandle,
}

impl VideoRef {
    pub fn new() -> Self {
        Self { handle: RefHandle::new() }
    }

    // Strict variant — caller handles RefError.
    pub fn play(&self) -> Result<(), RefError> {
        self.handle.invoke("play", vec![]).map(|_| ())
    }
    pub fn pause(&self) -> Result<(), RefError> {
        self.handle.invoke("pause", vec![]).map(|_| ())
    }
    pub fn seek(&self, position_seconds: f64) -> Result<(), RefError> {
        self.handle.invoke("seek", vec![WhiskerValue::Float(position_seconds)]).map(|_| ())
    }

    // Lenient variant — silently no-ops when unbound. For
    // fire-and-forget UI handlers (on_tap closures etc.).
    pub fn try_play(&self) { let _ = self.play(); }
    pub fn try_pause(&self) { let _ = self.pause(); }
    pub fn try_seek(&self, seconds: f64) { let _ = self.seek(seconds); }
}

impl WhiskerRef for VideoRef {
    fn handle(&self) -> &RefHandle { &self.handle }
}
```

That's the entire pattern. ~25 lines per ref type, all plain Rust:

- `cargo doc` lists `VideoRef` and its methods on its own page.
- rust-analyzer's go-to-definition lands on the impl block, not
  inside a macro expansion.
- Adding custom validation (logging, retries, type conversions) is
  just editing the impl block.
- Tests can construct a `VideoRef` and observe its `handle().is_bound()`
  directly.

Use site:

```rust
let r = VideoRef::new();
let r_play = r.clone();
let r_pause = r.clone();

render! {
    Video(ref: r, src: "https://...", style: "width: 100%; height: 240px;")
    view {
        text(on_tap: move || r_play.try_play(),   value: "▶")
        text(on_tap: move || r_pause.try_pause(), value: "⏸")
    }
}
```

## 5. Composite custom refs

Application authors that build a custom component wrapping a built-in
(e.g. a `CustomInput` that puts a label next to a `TextInput`) can
compose existing refs:

```rust
#[derive(Clone)]
pub struct CustomInputRef {
    text_input: TextInputRef,
    // Free to add more internal refs / signals here as the component grows:
    //   wrapper_view: ViewRef,
    //   error_message: RwSignal<Option<String>>,
}

impl CustomInputRef {
    pub fn new() -> Self {
        Self { text_input: TextInputRef::new() }
    }

    pub fn focus(&self) -> Result<(), RefError> { self.text_input.focus() }
    pub fn blur(&self)  -> Result<(), RefError> { self.text_input.blur() }

    // Methods that compose multiple inner-ref calls are just plain Rust:
    pub fn focus_and_select_all(&self) -> Result<(), RefError> {
        self.text_input.focus()?;
        self.text_input.select(0, usize::MAX)?;
        Ok(())
    }
}

// CustomInputRef does NOT implement WhiskerRef — it doesn't bind
// to a single element. The binding flows through `text_input`.

#[whisker::component]
pub fn custom_input(r: CustomInputRef, label: Signal<String>) -> Element {
    render! {
        view {
            text(value: label)
            // r.text_input.clone() shares the same Rc — when this
            // TextInput mounts, the same RefHandle the parent holds
            // (via r.text_input) becomes bound.
            TextInput(ref: r.text_input.clone())
        }
    }
}
```

### Three-layer composition just works

```rust
#[derive(Clone)]
pub struct FormRef {
    email: CustomInputRef,
    password: CustomInputRef,
}

impl FormRef {
    pub fn new() -> Self {
        Self {
            email: CustomInputRef::new(),
            password: CustomInputRef::new(),
        }
    }
    pub fn focus_email(&self) -> Result<(), RefError> {
        self.email.focus()
        // → CustomInputRef::focus
        // → TextInputRef::focus
        // → RefHandle::invoke("focus", …)
        // → C bridge → Lynx → native UITextField / EditText
    }
}
```

No macro is involved in any of this. The Ref hierarchy is just struct
fields all the way down to the leaf `RefHandle`.

## 6. Lifetime / unmount semantics

The unmount problem ("what happens when a method is called on a ref
whose component has been removed from the tree?") cannot be expressed
in Rust's lifetime system because:

- Refs are `Rc<…>`-shared across closures, parent components, child
  components, and reactive scopes. The compile-time lifetime is "as
  long as anyone holds a clone."
- Mount / unmount is a runtime event driven by Lynx's reactive
  reconciliation. The compile-time graph can't see when an element
  appears or disappears.
- A single ref instance often outlives multiple mount-unmount cycles
  (e.g. a component that toggles a child View on/off).

So binding state is **runtime-checked** via `RefHandle.inner.get()`'s
`Option`. The framework's mount machinery calls `RefHandle.__bind(elem)`
on mount and `RefHandle.__unbind()` on unmount. Method dispatch
returns `Err(RefError::NotBound)` in the unbound window.

The `try_*` lenient variants on each typed ref make
fire-and-forget call sites read cleanly:

```rust
text(on_tap: move || r.try_play(), value: "▶")
```

without an `unwrap` or a `let _ =`.

## 7. Where this leaves `Signal<T>`

Phase N's "Ref API standardization" doesn't displace signals — it
complements them. The decision table:

| Use case | Mechanism |
|---|---|
| State the parent and child both read / write | `RwSignal<T>` in a shared struct (controller pattern) |
| Derived state | `computed(...)` / `effect(...)` |
| One-shot commands on a mounted element (`focus()`, `play()`, `scrollTo()`) | `XxxRef` via `RefHandle` |
| Custom-component imperative API (rare) | Composite Ref struct + plain method forwarding |
| Events fired from the platform side back to Rust (`on_completed`, …) | Callback props (unchanged from today) |

In practice, custom components rarely need their own Ref. The
controller pattern + signals covers ~all user-component use cases.
Refs are mostly for **platform components and built-in views where
the platform side owns the authoritative state**.

## 8. Migration from today's `ElementRef<T>`

The current `ElementRef<T>` + `#[whisker::element_methods]` pair is
roughly today's working version of this layer. Phase N migration:

| Today | Phase N |
|---|---|
| `ElementRef<VideoProps>` | `VideoRef` (plain struct) |
| `element_ref::<VideoProps>()` | `VideoRef::new()` |
| `trait VideoSys` (sys proxy) + `trait VideoControls` (typed wrapper) + `impl VideoControls for ElementRef<VideoProps>` | `impl VideoRef { fn play(&self) -> Result<(), RefError>; ... }` |
| `r.invoke("play", vec![])` (low-level) | `r.handle().invoke("play", vec![])` |

Transitional `ElementRef<T> = RefHandle` alias can stay during the
deprecation window so existing module crates keep compiling.
`element_ref` proc macro emits both the new struct + the old trait
impls so the typed-wrapper trait existing callers depend on stays
reachable.

The `#[whisker::element_methods]` macro itself becomes unnecessary
in the new world — authors write `impl VideoRef { ... }` directly.

## 9. Open / deferred questions

- **`new()` taking arguments**: deferred (§5 of Phase L design).
  Authors that need richer initialization can add their own
  `with_initial_state(...)` constructors on top of `new()`.
- **`#[derive(WhiskerRef)]`** to skip the 4-line trait impl: out of
  scope. The trait is one method; deriving it would save 3 lines and
  cost a macro definition.
- **Ref binding across `provide_context` / `use_context`**: Refs are
  `Clone`, so passing a ref through context is just a plain `clone()`.
  No special handling needed.
- **SSR / hydrate without a real element**: `invoke()` returns
  `Err(NotBound)` because the inner `Option<Element>` is `None`. The
  `try_*` lenient methods absorb this transparently. Out of scope for
  Phase N — Whisker doesn't have SSR yet.

## 10. Phasing

| Step | Scope |
|---|---|
| **N-1** | Add `RefHandle`, `WhiskerRef`, `RefError` to `whisker-modules-api`. Existing `ElementRef<T>` aliased to `RefHandle`. No behavior change. |
| **N-2** | `#[component]` / `#[platform_component]` macros recognize `ref:` props whose type implements `WhiskerRef` and emit mount/unmount binding code that calls `__bind` / `__unbind`. |
| **N-3** | Migrate `whisker-video`, `whisker-hello-element`, `whisker-local-store` to hand-written `XxxRef` structs in place of `#[element_methods]`. |
| **N-4** | Migrate built-in `TextInput`, `ScrollView`, etc. to expose `TextInputRef`, `ScrollViewRef` via the same pattern. |
| **N-5** | Deprecate `ElementRef<T>` alias + `#[whisker::element_methods]` macro. Land removal in a separate release window (Phase M follow-up). |

## 11. Estimated work

| Sub-task | Estimate |
|---|---|
| N-1 + N-2 (primitives + macro binding) | 2 days |
| N-3 (migrate three reference modules) | 0.5 day |
| N-4 (migrate built-ins) | 1 day |
| N-5 (deprecation cycle) | 0.5 day after deprecation window |
| Total | ~3–4 days, plus the Phase M-style deprecation runway |

The standardization is intentionally small — most of the Ref API
lives in user-space (typed-ref structs that authors write), not in
the framework.
