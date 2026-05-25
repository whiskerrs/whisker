//! `ElementRef` — Rust-side handle for invoking methods on a mounted
//! Whisker platform component.
//!
//! Phase N redesign (see `docs/phase-n-ref-api-design.md`):
//!
//! - **Non-generic** — `ElementRef` carries no marker type. End-users
//!   never see `ElementRef` in component signatures; they hold typed
//!   `XxxHandle` structs and let the wrapping `#[whisker::component]`
//!   own the internal `ElementRef` that bridges native invocations.
//! - **`RwSignal`-backed binding** — the inner `Option<Element>` lives
//!   in the reactive runtime so `bound()` returns a `Signal<bool>`
//!   that `effect(...)` / `computed(...)` / `text(value: ...)` can
//!   observe. The hot-path `invoke()` reads via `get_untracked()` so
//!   imperative dispatch never accidentally subscribes its caller.
//! - **`Result<_, RefError>`** — `try_invoke` / `invoke_typed<T>`
//!   surface "not bound" and "platform-side error" as distinct error
//!   variants. The legacy `invoke()` returns
//!   `WhiskerValue` (with `WhiskerValue::Error` on failure) for
//!   transitional `#[whisker::element_methods]` compatibility.
//!
//! ## Where `ElementRef` appears
//!
//! Only in the signatures of `#[whisker::platform_component]`-declared
//! functions, as a hidden `__ref` prop the macro emits, and inside
//! module-author-written `#[whisker::component]` wrappers that bridge
//! a Handle struct to native via `effect(...)` blocks. End-users at
//! app-level code see `VideoHandle`, `TextInputHandle`, ..., never
//! `ElementRef` directly.

use whisker_runtime::reactive::{computed, RwSignal, Signal};
use whisker_runtime::view::Element;

use crate::module::WhiskerValue;

// ---------------------------------------------------------------------------
// RefError — explicit error surface for `try_invoke` / `invoke_typed`.
// ---------------------------------------------------------------------------

/// Errors that can surface from imperative element-method dispatch.
///
/// Returned by [`ElementRef::try_invoke`] and
/// [`ElementRef::invoke_typed`]. The legacy `invoke()` collapses both
/// variants into `WhiskerValue::Error` for backward compat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    /// Ref isn't bound to a mounted element. Either the component
    /// hasn't been rendered yet, or it has unmounted. Most UI
    /// fire-and-forget callers want to silently ignore this — that's
    /// what `let _ = sys.invoke(...);` inside a bridge `effect`
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

// ---------------------------------------------------------------------------
// ElementRef
// ---------------------------------------------------------------------------

/// Framework-internal handle to a mounted platform element. Lives in
/// `#[platform_component]`-emitted prop tables and the wrapping
/// `#[component]`s that drive a Handle. Not part of an app-author's
/// surface — Handles wrap this.
///
/// `Clone` produces a shared handle (same backing `RwSignal` arena
/// slot), so any number of bridge `effect`s can hold their own
/// `ElementRef` clone and observe the same mount / unmount events.
#[derive(Copy, Clone)]
pub struct ElementRef {
    /// Single source of truth: holds the currently-bound `Element`
    /// (or `None` while unmounted), and is the `Signal` that
    /// [`bound()`] derives from.
    inner: RwSignal<Option<Element>>,
}

impl ElementRef {
    /// Allocate a fresh, unbound ref.
    ///
    /// Used by `#[platform_component]` macro emission and by Handle
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
    /// For reactive observation, use [`bound()`].
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

    /// Strict invoke: returns `Err(RefError)` when unbound or when
    /// the platform side surfaces a `WhiskerValue::Error`.
    ///
    /// Bridge `effect(...)` blocks inside Handle wrapper components
    /// typically swallow the error: `let _ = sys.try_invoke(...);`.
    /// Authors that want explicit error surfacing call this directly.
    pub fn try_invoke(
        &self,
        method: &'static str,
        args: Vec<WhiskerValue>,
    ) -> Result<WhiskerValue, RefError> {
        let elem = self.inner.get_untracked().ok_or(RefError::NotBound)?;
        match crate::invoke_element_method(elem, method, args) {
            WhiskerValue::Error(message) => Err(RefError::DispatchFailed {
                method: method.into(),
                message,
            }),
            v => Ok(v),
        }
    }

    /// Strict typed invoke: dispatches and converts the result via
    /// `TryFrom<WhiskerValue>`. Type-mismatch errors collapse into
    /// `RefError::DispatchFailed` with a synthesised message.
    ///
    /// `T::Error` must convert into `String` so the macro can
    /// uniformly surface the mismatch as a dispatch failure.
    pub fn invoke_typed<T>(
        &self,
        method: &'static str,
        args: Vec<WhiskerValue>,
    ) -> Result<T, RefError>
    where
        T: TryFrom<WhiskerValue>,
        T::Error: std::fmt::Display,
    {
        let v = self.try_invoke(method, args)?;
        T::try_from(v).map_err(|e| RefError::DispatchFailed {
            method: method.into(),
            message: e.to_string(),
        })
    }

    /// Legacy invoke: returns `WhiskerValue`, with
    /// `WhiskerValue::Error("…")` standing in for both "not bound"
    /// and "platform-side error". Kept for transitional
    /// `#[whisker::element_methods]` compatibility (Phase N-3 removes
    /// the macro alongside this method).
    pub fn invoke(&self, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        let Some(elem) = self.inner.get_untracked() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke(\"{method}\"): ref is not bound to a \
                 mounted element"
            ));
        };
        crate::invoke_element_method(elem, method, args)
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
    /// `on_cleanup(...)` hook emitted by `#[platform_component]`
    /// so subsequent `try_invoke` calls return
    /// `Err(RefError::NotBound)` rather than dispatching against a
    /// recycled `Element` ID.
    ///
    /// `try_set` because the underlying signal may have already been
    /// disposed by the time this cleanup fires: `dispose_owner`
    /// frees the owner's signal nodes (step 4) *before* running
    /// cleanups (step 6). For the typical case (ref allocated in a
    /// parent owner, element mounted in a child owner) this is a
    /// non-issue; for the degenerate case (ref allocated and
    /// mounted in the same owner) `try_set` no-ops gracefully.
    #[doc(hidden)]
    pub fn __unbind(&self) {
        let _ = self.inner.try_set(None);
    }

    /// Deprecated public alias for [`__bind`] kept until Phase N-3
    /// migration. Don't call from author code.
    #[doc(hidden)]
    pub fn bind(&self, handle: Element) {
        self.__bind(handle);
    }

    /// Deprecated public alias for [`__unbind`] kept until Phase N-3
    /// migration. Don't call from author code.
    #[doc(hidden)]
    pub fn clear(&self) {
        self.__unbind();
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

/// Allocate a fresh, unbound `ElementRef`. Pair with a `ref:` prop in
/// `render!` to bind it on mount.
///
/// The generic parameter is **ignored**. It's kept on the function
/// signature so existing callers like `element_ref::<VideoProps>()`
/// keep compiling through the Phase N migration window. Phase N-3
/// will drop this shim in favour of typed `XxxHandle::new()`
/// constructors.
///
/// ```ignore
/// let r = ElementRef::new();
/// render! {
///     VideoSys(ref: r.clone(), src: "https://example.com/clip.mp4")
/// }
/// ```
pub fn element_ref<T: ?Sized>() -> ElementRef {
    let _ = std::marker::PhantomData::<*const T>;
    ElementRef::new()
}
