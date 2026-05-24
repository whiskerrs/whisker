//! `ElementRef<T>` — Rust-side handle for invoking methods on a
//! mounted Whisker native element. Phase 7-Φ.H.2.
//!
//! The mental model mirrors React's `useRef` + ImperativeHandle
//! pattern. Author code allocates an `ElementRef<Video>` once and
//! passes it as the `ref:` prop on a `Video(...)` element inside
//! `render!`. The native_element macro captures the underlying
//! [`Element`] handle into the ref at mount time. Once captured,
//! `video.play(...)` / `video.seek(30.0)` can fire at any later
//! point in the program; the `#[whisker::element_methods]`
//! proc macro generates the typed wrappers around
//! [`ElementRef::invoke`] that do the C-bridge dispatch.
//!
//! ## Lifecycle
//!
//! ```ignore
//! # use whisker::prelude::*;
//! # use whisker_video::Video;
//! let video = element_ref::<Video>();      // unbound
//! render! {
//!     Video(ref: video, src: "https://…")  // bound on mount
//!     button(on_tap: move || { video.play(); })
//! }
//! ```
//!
//! When the element unmounts the bound [`Element`] is cleared —
//! subsequent calls return `WhiskerValue::Error`. The ref itself
//! is cheap to clone (`Rc`-backed) so callbacks can capture it
//! freely.
//!
//! ## Why an `Element` handle, not a Lynx sign
//!
//! The Rust runtime hands out opaque `Element(u32)` IDs and the
//! Lynx renderer translates them to its own `WhiskerElement*`
//! handles internally. The C bridge later resolves
//! `WhiskerElement*` → Lynx UI sign during dispatch (Phase 7-Φ.
//! H.2.7), so we never need to surface signs to Rust.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::module::WhiskerValue;
use whisker_runtime::view::Element;

/// Typed reference to a mounted Whisker native element.
///
/// `T` is a marker type — the same struct the
/// `#[whisker::platform_component]` proc macro emits for the element.
/// It anchors the inherent-impl methods the
/// `#[whisker::element_methods]` proc macro produces (so
/// `video.play()` resolves only on an `ElementRef<Video>`, not on
/// an unrelated `ElementRef<Button>`).
///
/// `Clone` produces a shared handle — the inner `Rc<Cell<...>>`
/// means both clones see the same mount / unmount transitions.
/// `Default` produces an unbound ref equivalent to
/// [`element_ref::<T>()`].
pub struct ElementRef<T> {
    inner: Rc<Cell<Option<Element>>>,
    _marker: PhantomData<T>,
}

impl<T> ElementRef<T> {
    /// Allocate a fresh, unbound ref. Use [`element_ref::<T>()`]
    /// at call sites — this constructor is mostly for the
    /// `#[whisker::platform_component]` macro's prop-default path.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Cell::new(None)),
            _marker: PhantomData,
        }
    }

    /// Currently-bound [`Element`] handle, or `None` if the ref
    /// hasn't seen a mount yet (or has been cleared by unmount).
    pub fn element(&self) -> Option<Element> {
        self.inner.get()
    }

    /// Bind the ref to `handle`. Invoked by `#[whisker::native_
    /// element]`-generated code after `create_element_by_name`.
    ///
    /// Doesn't enforce uniqueness — if author code passes the
    /// same ref to two different element call sites, the last
    /// mount wins. This matches React's `useRef` semantics for
    /// the same reason (the alternative — error on collision —
    /// is more confusing in conditional render flows).
    pub fn bind(&self, handle: Element) {
        self.inner.set(Some(handle));
    }

    /// Clear the ref. Invoked at element unmount so subsequent
    /// `invoke` calls surface as Error rather than dispatching
    /// against a recycled `Element` ID.
    pub fn clear(&self) {
        self.inner.set(None);
    }

    /// Synchronously invoke `method` on the bound element. Routes
    /// through the C bridge (`whisker_bridge_invoke_element_
    /// method`) which dispatches via Lynx's `LynxUIMethodProcessor`
    /// / `LynxUIMethodsExecutor`.
    ///
    /// Returns [`WhiskerValue::Error`] when the ref isn't currently
    /// bound (element not mounted yet, or already unmounted) so
    /// caller code can `match` on the result without worrying
    /// about panics.
    ///
    /// `#[whisker::element_methods]` generates typed wrappers
    /// around this — direct calls are mostly for the macro's
    /// emission target. Direct invocation is also useful in tests
    /// where you want to drive an element without going through
    /// the typed API.
    pub fn invoke(&self, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        let Some(handle) = self.inner.get() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke(\"{method}\"): ref is not bound to a \
                 mounted element"
            ));
        };
        crate::invoke_element_method(handle, method, args)
    }
}

impl<T> Clone for ElementRef<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            _marker: PhantomData,
        }
    }
}

impl<T> Default for ElementRef<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for ElementRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementRef")
            .field("element", &self.inner.get())
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

/// Allocate a fresh, unbound `ElementRef<T>`. Pair with a
/// `ref:` prop in `render!` to bind it on mount:
///
/// ```ignore
/// let video = element_ref::<Video>();
/// render! {
///     Video(ref: video, src: "https://example.com/clip.mp4")
/// }
/// ```
pub fn element_ref<T>() -> ElementRef<T> {
    ElementRef::new()
}
