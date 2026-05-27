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
//! - **One invoke shape** — `invoke(method, args: WhiskerValue) ->
//!   WhiskerValue` (sync, fire-and-forget) + `invoke_async` /
//!   `invoke_typed<T>` (async, result-returning), mirroring
//!   `PlatformModule::invoke` / `invoke_async`. `args` is a single
//!   `WhiskerValue` passed straight through as the method's params
//!   object; the result comes back as a `WhiskerValue`. `invoke_typed`
//!   surfaces "not bound" / "platform-side error" as `RefError`
//!   variants; `invoke` collapses both into `WhiskerValue::Error`.
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

/// Result of [`ElementHandle::bounding_client_rect`] — the element's
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

/// Result of [`TextHandle::get_text_bounding_rect`] — the layout boxes
/// of a `<text>` substring (Lynx's `getTextBoundingRect`). `bounding_rect`
/// is the union box covering `[start, end)`; `boxes` is the per-line
/// box list. All rects are in LynxView coordinates (same shape as
/// [`BoundingClientRect`]).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBoundingRect {
    #[serde(default)]
    pub bounding_rect: BoundingClientRect,
    #[serde(default)]
    pub boxes: Vec<BoundingClientRect>,
}

/// Internal decode target for `getSelectedText` — the platform returns
/// `{ "selectedText": "…" }`; [`TextHandle::get_selected_text`] unwraps
/// it to the bare `String`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectedTextResult {
    #[serde(default)]
    selected_text: String,
}

// ---------------------------------------------------------------------------
// RefError — explicit error surface for `invoke_typed`.
// ---------------------------------------------------------------------------

/// Errors that can surface from imperative element-method dispatch.
///
/// Returned by [`ElementRef::invoke_typed`]. The fire-and-forget
/// [`ElementRef::invoke`] collapses both variants into
/// `WhiskerValue::Error` instead.
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

    /// Invoke a UI method on the bound element, **fire-and-forget**.
    /// `args` is a single [`WhiskerValue`] passed straight through as the
    /// method's params object — a [`map`](WhiskerValue::map) of named
    /// fields for built-in Lynx methods (`scrollTo`'s `offset` /
    /// `smooth`, …), or [`WhiskerValue::args`] for Whisker module
    /// elements (`@WhiskerUIMethod` reads `params.args`). The platform
    /// result isn't available synchronously, so this returns immediately
    /// with `WhiskerValue::Null` (or `WhiskerValue::Error` when unbound);
    /// use [`invoke_typed`](Self::invoke_typed) when you need the result.
    ///
    /// Mirrors `PlatformModule::invoke` — the same
    /// `(method, WhiskerValue) -> WhiskerValue` shape, so element and
    /// module dispatch read alike.
    pub fn invoke(&self, method: &str, args: WhiskerValue) -> WhiskerValue {
        let Some(elem) = self.inner.get_untracked() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke(\"{method}\"): ref is not bound to a \
                 mounted element"
            ));
        };
        crate::invoke_element_method_with_params(elem, method, args)
    }

    /// Async, **result-returning** invoke — the platform method's return
    /// value arrives via Lynx's UI-method callback (typically on the UI
    /// thread). `args` is the same single [`WhiskerValue`] params object
    /// as [`invoke`](Self::invoke). Returns the raw result
    /// [`WhiskerValue`], with `WhiskerValue::Error` for "not bound" /
    /// dispatch failure. Mirrors `PlatformModule::invoke_async`.
    ///
    /// Run from an event handler / effect via `spawn_local`, or use the
    /// typed [`invoke_typed`](Self::invoke_typed).
    pub async fn invoke_async(&self, method: &str, args: WhiskerValue) -> WhiskerValue {
        let Some(elem) = self.inner.get_untracked() else {
            return WhiskerValue::Error(format!(
                "ElementRef::invoke_async(\"{method}\"): ref is not bound to a \
                 mounted element"
            ));
        };
        crate::invoke_element_method_async_with_params(elem, method, args).await
    }

    /// Async invoke that deserializes the result into `T`. `NotBound`
    /// when unbound; `DispatchFailed` on a platform error or a
    /// result-shape mismatch. The building block the typed handle
    /// methods (`ScrollViewHandle::get_scroll_info`, …) build on.
    pub async fn invoke_typed<T: DeserializeOwned>(
        &self,
        method: &str,
        args: WhiskerValue,
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
    /// so subsequent `invoke_typed` calls return
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
// `ElementRef` is the bare invoke API (bind state + `invoke*`). The
// user-facing imperative surface lives on typed handles that wrap an
// `ElementRef` and call its invoke methods: `ElementHandle` for the
// generic UI methods any element supports, plus per-element handles that
// add that element's own methods. A handle exposes only what its element
// supports, so author code can't call `pause_animation()` on a
// `<scroll-view>`. Bind with `view(ref: handle.r())` /
// `image(ref: handle.r())` / `scroll_view(ref: handle.r())`.
//
// The generic methods (`bounding_client_rect`, `take_screenshot`) are
// written out on each handle rather than shared via `Deref` / a trait —
// the small duplication keeps every handle's surface explicit and
// jump-to-definition direct. Action methods dispatch through the
// synchronous fire-and-forget `invoke`; result methods use the async
// `invoke_typed` path.
// ---------------------------------------------------------------------------

/// Imperative handle to any mounted element — the generic Lynx UI
/// methods that work regardless of tag. Allocate with
/// [`ElementHandle::new`], bind via `view(ref: handle.r())` (or `text`,
/// `page`, …) in `render!`, then call the methods below.
///
/// `Copy` (the inner `ElementRef` is an arena handle), so it can be
/// captured by value into multiple event closures.
#[derive(Copy, Clone)]
pub struct ElementHandle {
    r: ElementRef,
}

impl ElementHandle {
    /// Allocate a fresh, unbound element handle.
    pub fn new() -> Self {
        Self { r: ElementRef::new() }
    }

    /// The underlying [`ElementRef`] — pass to a `ref:` prop to bind it
    /// on mount (`view(ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// `boundingClientRect` — the element's layout box in LynxView
    /// coordinates. Async: the result arrives via Lynx's UI-method
    /// callback (typically on the UI thread).
    ///
    /// ```ignore
    /// let card = ElementHandle::new();   // view(ref: card.r()) { … }
    /// spawn_local(async move {
    ///     if let Ok(rect) = card.bounding_client_rect().await {
    ///         // rect.width, rect.height, …
    ///     }
    /// });
    /// ```
    pub async fn bounding_client_rect(&self) -> Result<BoundingClientRect, RefError> {
        self.r
            .invoke_typed::<BoundingClientRect>("boundingClientRect", WhiskerValue::Null)
            .await
    }

    /// `takeScreenshot` — a base64-encoded image of the element
    /// (async). Returns the encoded string.
    pub async fn take_screenshot(&self) -> Result<String, RefError> {
        self.r
            .invoke_typed::<String>("takeScreenshot", WhiskerValue::Null)
            .await
    }
}

impl Default for ElementHandle {
    fn default() -> Self {
        Self::new()
    }
}

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

    /// `boundingClientRect` — the element's layout box in LynxView
    /// coordinates (async).
    pub async fn bounding_client_rect(&self) -> Result<BoundingClientRect, RefError> {
        self.r
            .invoke_typed::<BoundingClientRect>("boundingClientRect", WhiskerValue::Null)
            .await
    }

    /// `takeScreenshot` — a base64-encoded image of the element (async).
    pub async fn take_screenshot(&self) -> Result<String, RefError> {
        self.r
            .invoke_typed::<String>("takeScreenshot", WhiskerValue::Null)
            .await
    }

    /// `pauseAnimation` — pause a playing animated image, holding the
    /// current frame.
    pub fn pause_animation(&self) {
        let _ = self.r.invoke("pauseAnimation", WhiskerValue::Null);
    }

    /// `resumeAnimation` — resume a paused animated image from the
    /// held frame.
    pub fn resume_animation(&self) {
        let _ = self.r.invoke("resumeAnimation", WhiskerValue::Null);
    }

    /// `stopAnimation` — stop playback and reset to the first frame.
    pub fn stop_animation(&self) {
        let _ = self.r.invoke("stopAnimation", WhiskerValue::Null);
    }

    /// `startAnimate` — (re)start playback from the first frame.
    pub fn start_animate(&self) {
        let _ = self.r.invoke("startAnimate", WhiskerValue::Null);
    }
}

impl Default for ImageHandle {
    fn default() -> Self {
        Self::new()
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

    /// `boundingClientRect` — the element's layout box in LynxView
    /// coordinates (async).
    pub async fn bounding_client_rect(&self) -> Result<BoundingClientRect, RefError> {
        self.r
            .invoke_typed::<BoundingClientRect>("boundingClientRect", WhiskerValue::Null)
            .await
    }

    /// `takeScreenshot` — a base64-encoded image of the element (async).
    pub async fn take_screenshot(&self) -> Result<String, RefError> {
        self.r
            .invoke_typed::<String>("takeScreenshot", WhiskerValue::Null)
            .await
    }

    /// `getScrollInfo` — current scroll offset (`scroll_x`/`scroll_y`)
    /// and scrollable range. Async: resolves once the platform reports
    /// the values back over the bridge.
    pub async fn get_scroll_info(&self) -> Result<ScrollInfo, RefError> {
        self.r
            .invoke_typed::<ScrollInfo>("getScrollInfo", WhiskerValue::Null)
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
        let _ = self.r.invoke(
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
        let _ = self.r.invoke(
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
        let _ = self.r.invoke(
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

/// Imperative handle to a mounted `<text>`. Allocate with
/// [`TextHandle::new`], bind via `text(ref: handle.r())` in `render!`,
/// then drive / read text selection.
///
/// **Android note:** the geometry methods (`get_text_bounding_rect`,
/// `set_text_selection`, `get_selected_text`) need a real text `Layout`,
/// which a *flattened* text doesn't have — they come back empty / error.
/// Set `flatten: false` on the `<text>` if you call them on Android. iOS
/// extracts boxes regardless.
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
        Self { r: ElementRef::new() }
    }

    /// The underlying [`ElementRef`] — pass to a `ref:` prop to bind it
    /// on mount (`text(ref: handle.r())`).
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// `boundingClientRect` — the element's layout box in LynxView
    /// coordinates (async).
    pub async fn bounding_client_rect(&self) -> Result<BoundingClientRect, RefError> {
        self.r
            .invoke_typed::<BoundingClientRect>("boundingClientRect", WhiskerValue::Null)
            .await
    }

    /// `takeScreenshot` — a base64-encoded image of the element (async).
    pub async fn take_screenshot(&self) -> Result<String, RefError> {
        self.r
            .invoke_typed::<String>("takeScreenshot", WhiskerValue::Null)
            .await
    }

    /// `getSelectedText` — the currently-selected substring of the text
    /// (empty if nothing is selected). Async result.
    pub async fn get_selected_text(&self) -> Result<String, RefError> {
        self.r
            .invoke_typed::<SelectedTextResult>("getSelectedText", WhiskerValue::Null)
            .await
            .map(|r| r.selected_text)
    }

    /// `getTextBoundingRect` — the layout box(es) of the substring
    /// `[start, end)` (character indices). Async result; the union box
    /// is `bounding_rect`, per-line boxes are `boxes`.
    pub async fn get_text_bounding_rect(
        &self,
        start: i32,
        end: i32,
    ) -> Result<TextBoundingRect, RefError> {
        self.r
            .invoke_typed::<TextBoundingRect>(
                "getTextBoundingRect",
                WhiskerValue::map([
                    ("start", WhiskerValue::Int(start as i64)),
                    ("end", WhiskerValue::Int(end as i64)),
                ]),
            )
            .await
    }

    /// `setTextSelection` — highlight the text between
    /// `(start_x, start_y)` and `(end_x, end_y)` (logical pixels,
    /// relative to the text component). Fire-and-forget.
    ///
    /// Coordinates are sent as numbers: Android reads them with
    /// `params.getDouble`, iOS with `toPtFromIDUnitValue` (which takes a
    /// bare number as points) — a number is the form both honor.
    pub fn set_text_selection(&self, start_x: f64, start_y: f64, end_x: f64, end_y: f64) {
        let _ = self.r.invoke(
            "setTextSelection",
            WhiskerValue::map([
                ("startX", WhiskerValue::Float(start_x)),
                ("startY", WhiskerValue::Float(start_y)),
                ("endX", WhiskerValue::Float(end_x)),
                ("endY", WhiskerValue::Float(end_y)),
            ]),
        );
    }
}

impl Default for TextHandle {
    fn default() -> Self {
        Self::new()
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
