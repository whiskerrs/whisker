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
//! Only in the signatures of `#[whisker::module_component]`-declared
//! functions, as a hidden `__ref` prop the macro emits, and inside
//! module-author-written `#[whisker::component]` wrappers that bridge
//! a Handle struct to native via `effect(...)` blocks. End-users at
//! app-level code see `VideoHandle`, `TextInputHandle`, ..., never
//! `ElementRef` directly.

use serde::de::DeserializeOwned;
use serde::Deserialize;
use whisker_runtime::reactive::{computed, RwSignal, Signal};
use whisker_runtime::view::Element;

use crate::module::WhiskerValue;

// ---------------------------------------------------------------------------
// Typed element-method results
// ---------------------------------------------------------------------------

/// Result of [`ElementRef::bounding_client_rect`] — the element's
/// layout box in LynxView coordinates (Lynx's `boundingClientRect`
/// UI method). Every field is `#[serde(default)]`, so any key the
/// platform omits reads back as `0.0` rather than failing the decode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct BoundingClientRect {
    #[serde(default)]
    pub left: f64,
    #[serde(default)]
    pub top: f64,
    #[serde(default)]
    pub right: f64,
    #[serde(default)]
    pub bottom: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

/// Result of [`ScrollViewHandle::get_scroll_info`] — the current
/// scroll offset and scrollable range of a `<scroll-view>` (Lynx's
/// `getScrollInfo` UI method). Every field is `#[serde(default)]`, so
/// whichever subset the platform's scroll UI reports populates and
/// the rest read back `0.0`: `UIScrollView` fills
/// `scroll_x`/`scroll_y`/`scroll_range`; the internal scroller fills
/// `scroll_x`/`scroll_y` plus `scroll_width`/`scroll_height`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollInfo {
    #[serde(default)]
    pub scroll_x: f64,
    #[serde(default)]
    pub scroll_y: f64,
    #[serde(default)]
    pub scroll_range: f64,
    #[serde(default)]
    pub scroll_width: f64,
    #[serde(default)]
    pub scroll_height: f64,
}

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
/// `#[module_component]`-emitted prop tables and the wrapping
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
    /// Used by `#[module_component]` macro emission and by Handle
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

    /// Invoke a platform method on the bound element. Returns the raw
    /// `WhiskerValue`, with `WhiskerValue::Error("…")` standing in for
    /// both "not bound" and "platform-side error" (loggable either
    /// way). This is the primitive a typed handle wrapper builds on
    /// (`VideoHandle::play` → `self.r.invoke("play", vec![])`);
    /// `try_invoke` / `invoke_typed` are the `Result`-returning
    /// variants for callers that want to branch on failure.
    pub fn invoke(&self, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        let Some(elem) = self.inner.get_untracked() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke(\"{method}\"): ref is not bound to a \
                 mounted element"
            ));
        };
        crate::invoke_element_method(elem, method, args)
    }

    /// Fire-and-forget invoke of a built-in Lynx UI method whose
    /// arguments are *named fields* of a params object (`scrollTo`,
    /// `scrollIntoView`, …). `params` is a single `WhiskerValue`
    /// (typically a [`WhiskerValue::map`]) passed straight through as
    /// the params object — not wrapped in `{"args": […]}` like
    /// [`invoke`](Self::invoke). The typed handle wrappers
    /// (`ScrollViewHandle::scroll_to`, …) build on this.
    pub fn invoke_with_params(&self, method: &str, params: WhiskerValue) -> WhiskerValue {
        let Some(elem) = self.inner.get_untracked() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke_with_params(\"{method}\"): ref is not bound \
                 to a mounted element"
            ));
        };
        crate::invoke_element_method_with_params(elem, method, params)
    }

    /// `scrollIntoView` — scroll this element into the visible area of
    /// its nearest scrollable ancestor. `behavior` is `"smooth"` and
    /// `block` is `"nearest"` (minimal scroll). Generic: available on
    /// any element handle.
    pub fn scroll_into_view(&self) {
        let _ = self.invoke_with_params(
            "scrollIntoView",
            WhiskerValue::map([(
                "scrollIntoViewOptions",
                WhiskerValue::map([
                    ("behavior", WhiskerValue::String("smooth".into())),
                    ("block", WhiskerValue::String("nearest".into())),
                ]),
            )]),
        );
    }

    /// Async, **result-returning** invoke — for UI methods whose
    /// return value arrives via Lynx's callback (`boundingClientRect`,
    /// `takeScreenshot`, …) rather than synchronously. Returns the raw
    /// [`WhiskerValue`], with `WhiskerValue::Error` standing in for
    /// "not bound" / dispatch failure. Typed wrappers (e.g.
    /// [`bounding_client_rect`](Self::bounding_client_rect)) build on
    /// this.
    ///
    /// Run it from an event handler / effect via `spawn_local`:
    /// `spawn_local(async move { let v = r.invoke_async("m", vec![]).await; })`.
    pub async fn invoke_async(&self, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        let Some(elem) = self.inner.get_untracked() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke_async(\"{method}\"): ref is not bound to a \
                 mounted element"
            ));
        };
        crate::invoke_element_method_async(elem, method, args).await
    }

    /// Async invoke that deserializes the result into `T`. `NotBound`
    /// when unbound; `DispatchFailed` on a platform error or a
    /// result-shape mismatch. The building block for the typed
    /// method wrappers below.
    pub async fn invoke_typed_async<T: DeserializeOwned>(
        &self,
        method: &'static str,
        args: Vec<WhiskerValue>,
    ) -> Result<T, RefError> {
        if !self.is_bound() {
            return Err(RefError::NotBound);
        }
        match self.invoke_async(method, args).await {
            WhiskerValue::Error(message) => Err(RefError::DispatchFailed {
                method: method.into(),
                message,
            }),
            other => other
                .deserialize_into::<T>()
                .map_err(|message| RefError::DispatchFailed {
                    method: method.into(),
                    message,
                }),
        }
    }

    /// `boundingClientRect` — the element's layout box in LynxView
    /// coordinates. Async: the result arrives via Lynx's UI-method
    /// callback (typically on the UI thread).
    ///
    /// ```ignore
    /// let card = ElementRef::new();   // view(ref: card) { … }
    /// spawn_local(async move {
    ///     if let Ok(rect) = card.bounding_client_rect().await {
    ///         // rect.width, rect.height, …
    ///     }
    /// });
    /// ```
    pub async fn bounding_client_rect(&self) -> Result<BoundingClientRect, RefError> {
        self.invoke_typed_async::<BoundingClientRect>("boundingClientRect", vec![])
            .await
    }

    /// `takeScreenshot` — a base64-encoded image of the element
    /// (async). Returns the encoded string.
    pub async fn take_screenshot(&self) -> Result<String, RefError> {
        self.invoke_typed_async::<String>("takeScreenshot", vec![])
            .await
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
    /// `on_cleanup(...)` hook emitted by `#[module_component]`
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

// ---------------------------------------------------------------------------
// Built-in element handles
//
// Typed wrappers around `ElementRef` (the VideoHandle pattern, applied
// to Lynx built-ins). Each handle exposes only the imperative methods
// that element type actually supports, so author code can't call
// `pause_animation()` on a `<scroll-view>`. Both `Deref` to
// `ElementRef`, so the generic methods (`bounding_client_rect`,
// `take_screenshot`, …) are callable directly on the handle. Bind with
// `image(ref: handle.r())` / `scroll_view(ref: handle.r())`.
//
// Action methods (no result) dispatch through the synchronous,
// fire-and-forget `invoke`. Result methods (`get_scroll_info`) use the
// async `invoke_typed_async` path. Methods that take a Lynx params
// object (`scrollTo`, …) are gated on the params-map bridge path and
// land separately.
// ---------------------------------------------------------------------------

/// Imperative handle to a mounted `<image>`. Allocate with
/// [`ImageHandle::new`], bind via `image(ref: handle.r())` in
/// `render!`, then drive animated-image (GIF / APNG) playback.
///
/// `Copy` (the inner `ElementRef` is an arena handle), so it can be
/// captured by value into multiple event closures.
#[derive(Copy, Clone)]
pub struct ImageHandle {
    r: ElementRef,
}

impl ImageHandle {
    /// Allocate a fresh, unbound image handle.
    pub fn new() -> Self {
        Self { r: ElementRef::new() }
    }

    /// The underlying [`ElementRef`] — pass to a `ref:` prop to bind
    /// it on mount (`image(ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// `pauseAnimation` — pause a playing animated image, holding the
    /// current frame.
    pub fn pause_animation(&self) {
        let _ = self.r.invoke("pauseAnimation", vec![]);
    }

    /// `resumeAnimation` — resume a paused animated image from the
    /// held frame.
    pub fn resume_animation(&self) {
        let _ = self.r.invoke("resumeAnimation", vec![]);
    }

    /// `stopAnimation` — stop playback and reset to the first frame.
    pub fn stop_animation(&self) {
        let _ = self.r.invoke("stopAnimation", vec![]);
    }

    /// `startAnimate` — (re)start playback from the first frame.
    pub fn start_animate(&self) {
        let _ = self.r.invoke("startAnimate", vec![]);
    }
}

impl Default for ImageHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for ImageHandle {
    type Target = ElementRef;
    fn deref(&self) -> &ElementRef {
        &self.r
    }
}

/// Imperative handle to a mounted `<scroll-view>`. Allocate with
/// [`ScrollViewHandle::new`], bind via `scroll_view(ref: handle.r())`
/// in `render!`, then query scroll state.
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
        Self { r: ElementRef::new() }
    }

    /// The underlying [`ElementRef`] — pass to a `ref:` prop to bind
    /// it on mount (`scroll_view(ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// `getScrollInfo` — current scroll offset (`scroll_x`/`scroll_y`)
    /// and scrollable range. Async: resolves once the platform reports
    /// the values back over the bridge.
    pub async fn get_scroll_info(&self) -> Result<ScrollInfo, RefError> {
        self.r
            .invoke_typed_async::<ScrollInfo>("getScrollInfo", vec![])
            .await
    }

    /// `scrollTo` — scroll to an absolute `offset` (logical pixels)
    /// along the scroll axis. `smooth` animates the scroll.
    ///
    /// `offset` is sent as a number, not a `"<n>px"` string: Android's
    /// `UIScrollView.scrollTo` reads it with `params.getDouble("offset")`
    /// (a string decodes to 0), and iOS's `toPtFromIDUnitValue` accepts
    /// a bare number as points — so a number is the one form both honor.
    pub fn scroll_to(&self, offset: f64, smooth: bool) {
        let _ = self.r.invoke_with_params(
            "scrollTo",
            WhiskerValue::map([
                ("offset", WhiskerValue::Float(offset)),
                ("smooth", WhiskerValue::Bool(smooth)),
            ]),
        );
    }

    /// `scrollTo` by child `index` — scroll so the child at `index`
    /// aligns to the scroll start. `smooth` animates the scroll.
    pub fn scroll_to_index(&self, index: i32, smooth: bool) {
        let _ = self.r.invoke_with_params(
            "scrollTo",
            WhiskerValue::map([
                ("index", WhiskerValue::Int(index as i64)),
                ("smooth", WhiskerValue::Bool(smooth)),
            ]),
        );
    }

    /// `scrollBy` — scroll by a relative `offset` (logical pixels)
    /// from the current position along the scroll axis. Always instant
    /// (Lynx's `scrollBy` doesn't honor a `smooth` flag — use
    /// [`scroll_to`](Self::scroll_to) for animated moves).
    ///
    /// `offset` is a number for the same cross-platform reason as
    /// [`scroll_to`](Self::scroll_to) (Android `getDouble` + iOS
    /// `dipToPx` / `toPtFromIDUnitValue`).
    pub fn scroll_by(&self, offset: f64) {
        let _ = self.r.invoke_with_params(
            "scrollBy",
            WhiskerValue::map([("offset", WhiskerValue::Float(offset))]),
        );
    }
}

// NOTE: `autoScroll` is intentionally not exposed. Its `rate` param is
// read as a *number* on Android (`AndroidScrollView.autoScroll` →
// `params.getDouble("rate")`) but as a *unit string* on iOS
// (`LynxUIScroller`/`LynxUIScrollViewInternal` → `toPtWithUnitValue:`,
// which only accepts `NSString`). No single wire value satisfies both,
// so rather than ship a method that silently no-ops on one platform we
// leave it out until the fork's readers converge.

impl Default for ScrollViewHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for ScrollViewHandle {
    type Target = ElementRef;
    fn deref(&self) -> &ElementRef {
        &self.r
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounding_client_rect_deserializes_from_value_map() {
        // Shape mirrors what Lynx's `boundingClientRect` UI method
        // returns through the async invoke path.
        let v = WhiskerValue::map([
            ("left", WhiskerValue::Float(10.0)),
            ("top", WhiskerValue::Float(20.0)),
            ("right", WhiskerValue::Float(110.0)),
            ("bottom", WhiskerValue::Float(70.0)),
            ("width", WhiskerValue::Float(100.0)),
            ("height", WhiskerValue::Float(50.0)),
        ]);
        let rect: BoundingClientRect = v.deserialize_into().expect("deserialize rect");
        assert_eq!(rect.left, 10.0);
        assert_eq!(rect.width, 100.0);
        assert_eq!(rect.height, 50.0);
    }

    #[test]
    fn bounding_client_rect_missing_fields_default_to_zero() {
        // Integer-valued numbers + a partial body still decode (Int
        // widens to f64, missing keys default to 0.0).
        let v = WhiskerValue::map([("width", WhiskerValue::Int(42))]);
        let rect: BoundingClientRect = v.deserialize_into().expect("partial rect");
        assert_eq!(rect.width, 42.0);
        assert_eq!(rect.height, 0.0);
    }
}
