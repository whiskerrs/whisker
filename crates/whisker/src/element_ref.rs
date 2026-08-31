//! `ElementRef` — Rust-side handle for invoking methods on a mounted
//! Whisker platform component, plus the typed `XxxHandle` family that
//! wraps it for end-user code.
//!
//! ## Design
//!
//! - **Non-generic** — `ElementRef` carries no marker type. End-users
//!   never see `ElementRef` in component signatures; they hold typed
//!   `XxxHandle` structs and let the wrapping `#[whisker::component]`
//!   own the internal `ElementRef` that bridges native invocations.
//! - **`RwSignal`-backed binding** — the inner `Option<Element>` lives
//!   in the reactive runtime so [`ElementRef::bound`] returns a
//!   `Signal<bool>` that `effect(...)` / `computed(...)` /
//!   `Text(value: ...)` can observe. The hot-path
//!   [`ElementRef::command`] reads via `get_untracked()` so imperative
//!   dispatch never accidentally subscribes its caller.
//! - **One command shape** — `command(name, parameters: WhiskerValue) ->
//!   Result<(), RefError>`. Element commands are ordered, one-way frame
//!   operations. Result-bearing element calls are not part of module v1;
//!   service modules provide `invoke` / `invoke_async` when a value is needed.
//!
//! ## Where `ElementRef` appears
//!
//! Only in the signatures of `#[whisker::module_element]`-declared
//! functions, as a hidden `__ref` prop the macro emits, and inside
//! module-author-written `#[whisker::component]` wrappers that bridge
//! a Handle struct to native via `effect(...)` blocks. End-user app
//! code sees [`ElementHandle`], [`ScrollViewHandle`], [`TextHandle`],
//! and similar typed handles — never `ElementRef` directly.

use whisker_runtime::reactive::{RwSignal, Signal, computed};
use whisker_runtime::view::Element;

use whisker_runtime::value::WhiskerValue;

/// Errors that can surface from imperative element-method dispatch.
///
/// Returned by [`ElementRef::command`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefError {
    /// Ref isn't bound to a mounted element. Either the component
    /// hasn't been rendered yet, or it has unmounted. Most UI
    /// fire-and-forget callers want to silently ignore this — that's
    /// what `let _ = sys.command(...);` inside a bridge `effect`
    /// provides.
    NotBound,
    /// Platform side surfaced a dispatch error (unknown method, type
    /// mismatch, platform-side exception, …). The `message` is the
    /// bridge's verbatim UTF-8 description.
    DispatchFailed { method: String, message: String },
}

impl std::fmt::Display for RefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefError::NotBound => f.write_str("ref is not bound to a mounted element"),
            RefError::DispatchFailed { method, message } => {
                write!(f, "platform method `{method}` failed: {message}")
            }
        }
    }
}

impl std::error::Error for RefError {}

/// Framework-internal handle to a mounted platform element. Lives in
/// `#[module_element]`-emitted prop tables and the wrapping
/// `#[component]`s that drive a Handle. Not part of an app-author's
/// surface — Handles wrap this.
///
/// `Clone` produces a shared handle (same backing `RwSignal` arena
/// slot), so any number of bridge `effect`s can hold their own
/// `ElementRef` clone and observe the same mount / unmount events.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct ElementRef {
    /// Single source of truth: holds the currently-bound `Element`
    /// (or `None` while unmounted), and is the `Signal` that
    /// [`bound()`] derives from.
    inner: RwSignal<Option<Element>>,
}

impl ElementRef {
    /// Allocate a fresh, unbound ref.
    ///
    /// Used by `#[module_element]` macro emission and by Handle
    /// bridge wrappers (`fn video(handle: VideoHandle, ...) -> Element`).
    /// Allocates in the current reactive owner — see
    /// `whisker_runtime::reactive::signal()`.
    pub fn new() -> Self {
        Self {
            inner: RwSignal::new(None),
        }
    }

    /// Currently-bound `Element` handle, or `None` if the ref hasn't
    /// seen a mount yet (or has been cleared by unmount). Non-reactive
    /// (uses `get_untracked()`), so calling from inside an
    /// `effect(...)` doesn't subscribe the effect to the binding.
    pub fn element(&self) -> Option<Element> {
        self.inner.get_untracked()
    }

    /// `true` iff bound to a live element right now. Non-reactive.
    /// For reactive observation, use [`bound`](Self::bound).
    pub fn is_bound(&self) -> bool {
        self.inner.get_untracked().is_some()
    }

    /// Reactive read of "is the underlying element mounted right now?"
    ///
    /// Subscribe inside `effect(...)` / `computed(...)` / a tag's
    /// `value: move || ...` to react to mount / unmount events.
    ///
    /// ```ignore
    /// let sys = ElementRef::new();
    /// effect({
    ///     let sys = sys.clone();
    ///     move || if sys.bound().get() {
    ///         // Component just mounted — kick off initial state.
    ///     }
    /// });
    /// ```
    pub fn bound(&self) -> Signal<bool> {
        let inner = self.inner;
        Signal::Dynamic(computed(move || inner.with(|opt| opt.is_some())))
    }

    /// Queues a one-way command on the bound element.
    ///
    /// The command is schema-validated and ordered with the next frame. A
    /// successful return means it was enqueued; Host execution happens later
    /// and cannot synchronously return a value.
    pub fn command(&self, command: &str, parameters: WhiskerValue) -> Result<(), RefError> {
        let Some(element) = self.inner.get_untracked() else {
            return Err(RefError::NotBound);
        };
        if !parameters.is_data() {
            return Err(RefError::DispatchFailed {
                method: command.into(),
                message: "element command parameters cannot contain Error values".into(),
            });
        }
        invoke_element_command(element, command, parameters)
    }

    /// Bind the ref to `handle`. Invoked by `#[whisker::platform_
    /// component]`-generated code after `create_element_by_name`.
    ///
    /// Doesn't enforce uniqueness — if author code passes the
    /// same ref to two different element call sites, the last
    /// mount wins. This matches React's `useRef` semantics for
    /// the same reason (the alternative — error on collision —
    /// is more confusing in conditional render flows).
    ///
    /// Framework-internal; intentionally public so the proc macro
    /// can emit calls but **not** to be invoked from author code.
    ///
    /// Uses `try_set` because the same owner that allocated the
    /// underlying signal may also be the one driving `__bind` (when
    /// the ref is created in a component body and then mounted
    /// inside the same component) — that's not a hot path but the
    /// graceful no-op keeps the API symmetric with `__unbind`.
    #[doc(hidden)]
    pub fn __bind(&self, handle: Element) {
        let _ = self.inner.try_set(Some(handle));
    }

    /// Clear the ref. Invoked at element unmount via the
    /// `on_cleanup(...)` hook emitted by `#[module_element]`
    /// so subsequent commands cannot dispatch against a recycled `Element` ID.
    ///
    /// `try_set` because the underlying signal may have already been
    /// disposed by the time this cleanup fires: `Owner::dispose`
    /// frees the owner's signal nodes (step 4) *before* running
    /// cleanups (step 6). For the typical case (ref allocated in a
    /// parent owner, element mounted in a child owner) this is a
    /// non-issue; for the degenerate case (ref allocated and
    /// mounted in the same owner) `try_set` no-ops gracefully.
    #[doc(hidden)]
    pub fn __unbind(&self) {
        let _ = self.inner.try_set(None);
    }
}

fn invoke_element_command(
    handle: Element,
    command: &str,
    parameters: WhiskerValue,
) -> Result<(), RefError> {
    match whisker_runtime::view::try_invoke_element_command(handle, command, parameters) {
        Some(Ok(())) => Ok(()),
        Some(Err(message)) => Err(RefError::DispatchFailed {
            method: command.into(),
            message,
        }),
        None => Err(RefError::DispatchFailed {
            method: command.into(),
            message: format!("element {} has no Host command binding", handle.id()),
        }),
    }
}

impl Default for ElementRef {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ElementRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementRef")
            .field("element", &self.inner.get_untracked())
            .finish()
    }
}

/// Imperative handle to any mounted element. Allocate with
/// [`ElementHandle::new`], bind via `View(element_ref: handle.r())` (or `Text`,
/// `ScrollView`, …) in `render!`.
///
/// `Copy` (the inner `ElementRef` is an arena handle), so it can be
/// captured by value into multiple event closures.
///
/// ```ignore
/// let card = ElementHandle::new();
/// render! { View(element_ref: card.r()) { /* … */ } }
/// ```
#[derive(Copy, Clone)]
pub struct ElementHandle {
    r: ElementRef,
}

impl ElementHandle {
    /// Allocate a fresh, unbound element handle.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying [`ElementRef`] — pass to a `element_ref:` prop to bind it
    /// on mount (`View(element_ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }
}

impl Default for ElementHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Imperative handle to a mounted `<scroll-view>`. Allocate with
/// [`ScrollViewHandle::new`], bind via `ScrollView(element_ref: handle.r())`
/// in `render!`, then issue scroll commands.
///
/// `Copy` (the inner `ElementRef` is an arena handle), so it can be
/// captured by value into multiple event closures.
#[derive(Copy, Clone)]
pub struct ScrollViewHandle {
    r: ElementRef,
}

impl ScrollViewHandle {
    /// Allocate a fresh, unbound scroll-view handle.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying [`ElementRef`] — pass to a `element_ref:` prop to bind
    /// it on mount (`ScrollView(element_ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// `scrollTo` — scroll to an absolute `offset` (logical pixels)
    /// along the scroll axis. `smooth` animates the scroll.
    pub fn scroll_to(&self, offset: f64, smooth: bool) {
        let _ = self.r.command(
            "scrollTo",
            WhiskerValue::map([
                ("offset", WhiskerValue::Float(offset)),
                ("smooth", WhiskerValue::Bool(smooth)),
            ]),
        );
    }

    /// `scrollBy` — scroll by a relative `offset` (logical pixels)
    /// from the current position along the scroll axis. `smooth` animates
    /// the scroll.
    pub fn scroll_by(&self, offset: f64, smooth: bool) {
        let _ = self.r.command(
            "scrollBy",
            WhiskerValue::map([
                ("offset", WhiskerValue::Float(offset)),
                ("smooth", WhiskerValue::Bool(smooth)),
            ]),
        );
    }
}

impl Default for ScrollViewHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Imperative handle to a mounted `<text>`. Allocate with
/// [`TextHandle::new`] and bind via `Text(element_ref: handle.r())` in `render!`.
///
/// `Copy` (the inner `ElementRef` is an arena handle), so it can be
/// captured by value into multiple event closures.
#[derive(Copy, Clone)]
pub struct TextHandle {
    r: ElementRef,
}

impl TextHandle {
    /// Allocate a fresh, unbound text handle.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying [`ElementRef`] — pass to a `element_ref:` prop to bind it
    /// on mount (`Text(element_ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }
}

impl Default for TextHandle {
    fn default() -> Self {
        Self::new()
    }
}
