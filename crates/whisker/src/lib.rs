//! # Whisker
//!
//! Cross-platform mobile UI framework for Rust, built on the Lynx C++ engine.
//!
//! Most users only need:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     rsx! {
//!         page { style: "background: white;",
//!             text { "Hello, Whisker" }
//!         }
//!     }
//! }
//! ```

// `#[whisker::component]` (and other macros in this crate) emit
// `::whisker::…` paths. From inside the whisker crate itself those
// paths don't resolve naturally — Rust treats `whisker` as the
// implicit `crate::` keyword's snake-case parallel only at the
// edition's discretion. `extern crate self as whisker;` makes the
// crate's own items reachable through `::whisker::…`, which is what
// the macros expand to. Required for built-in `#[component]`-shaped
// items (e.g. `ForEach` / `Show` in `control_flow.rs`).
extern crate self as whisker;

pub use whisker_app_config as app_config;
pub use whisker_runtime as runtime;

// Type-safe CSS builder. Accessible as `whisker::css::*` (the
// namespaced form for explicit qualification) or by reaching
// through `whisker::prelude::*` (which re-exports the most common
// pieces).
pub use whisker_css as css;

// Re-export the element tag enum the macro emit references through
// `::whisker::ElementTag`. The C bridge keys element creation off
// the same enum.
pub use whisker_runtime::element::ElementTag;

pub use whisker_macros::{component, main, module_component, render};

// Phase 7-Φ.H.2 — `ElementRef<T>` is the Rust-side handle for
// invoking methods on a mounted platform component. `element_ref::<T>()`
// allocates a fresh, unbound ref; the `#[whisker::module_component]`
// macro binds it on mount when passed as the `ref:` prop.
pub use whisker_driver::{
    animate_cancel, animate_start, element_ref, invoke_element_animate, AnimateOp, AnimateOptions,
    BoundingClientRect, ElementHandle, ElementRef, RefError, ScrollInfo, ScrollViewHandle,
    TextBoundingRect, TextHandle, UiInfo,
};

// Function-only module dispatch. `PlatformModule` is the name-keyed
// handle (≈ Expo `requireNativeModule`); the `module!` macro builds
// one with the calling crate's name auto-prefixed for collision-free
// dispatch (mirrors how `#[whisker::module_component]` namespaces
// element tags).
pub use whisker_driver::module::PlatformModule;

/// The universal tagged-union value model. Crosses the native
/// boundary as both module args/returns and event payloads, so it
/// lives at the crate root rather than buried under
/// `platform_module` (where it's also re-exported for back-compat).
pub use whisker_runtime::value::WhiskerValue;

/// Typed event objects handed to `on_<event>` handlers on built-in
/// elements and `#[whisker::module_component]` view methods.
///
/// A `view(on_tap: |e| …)` handler receives a [`TouchEvent`](event::TouchEvent);
/// `on_animationend` an [`AnimationEvent`](event::AnimationEvent);
/// lifecycle / component-state events a [`CustomEvent`](event::CustomEvent).
pub mod event {
    pub use whisker_runtime::event::{
        AnimationEvent, BindType, CustomEvent, Event, Point, ScrollDetail, ScrollEvent,
        SelectionChangeEvent, SelectionDetail, Size, Target, TextLayoutDetail, TextLayoutEvent,
        TextLineInfo, Touch, TouchEvent,
    };
}

/// Build a [`PlatformModule`] handle for the native module named
/// `$name`, with the calling crate's name prepended
/// (`<crate>:<$name>`) so two crates can ship same-named modules
/// without colliding in the dispatch registry. `env!("CARGO_PKG_NAME")`
/// resolves in the *calling* crate, so the prefix is always the
/// crate that wrote the `module!(...)` call.
///
/// ```ignore
/// let store = whisker::module!("WhiskerLocalStore"); // -> <crate>:WhiskerLocalStore
/// let v = store.invoke("save", vec![key.into(), value.into()]);
/// ```
#[macro_export]
macro_rules! module {
    ($name:literal) => {
        $crate::PlatformModule::named(concat!(env!("CARGO_PKG_NAME"), ":", $name))
    };
}

// Phase 6.5a reactive surface, lifted to the top-level namespace so
// user code can `use whisker::*` and reach the typical primitives
// directly. The underlying impl lives in `whisker_runtime::reactive`
// for callers that prefer the long path.
pub use whisker_runtime::reactive::{
    computed, create_owner, dispose_owner, effect, flush, flush_mounts, mount_component,
    on_cleanup, on_mount, provide_context, resource, resource_sync, signal, unmount_component,
    use_context, with_context, with_owner, ReadSignal, Resource, ResourceState, RwSignal, Signal,
    StoredValue, WriteSignal,
};
// Async task host. `resource()` uses these internally, but they're
// also part of the user surface: components spawn ad-hoc async work
// through `spawn_local`, and `run_blocking` is the standard escape
// hatch for sync IO inside `async fn` bodies.
pub use whisker_runtime::tasks::{run_blocking, run_until_stalled, spawn_local};
mod control_flow;
mod style;

pub use style::{apply_style, Style};

// Built-in control flow — same `#[component]` form as anything a user
// could implement. The PascalCase aliases `ForEach` / `Show` are
// what `render!` resolves to.
pub use control_flow::{ForEach, ForEachProps, Show, ShowProps};
// `Children` is the conventional prop type for components that wrap
// non-kwarg child nodes in their `render!` invocation.
pub use whisker_runtime::view::Children;
// Function-shaped prop types for control-flow components — the
// `each` / `key` / `children` triple `ForEach` takes, the
// `Fallback` newtype `Show` accepts as its `fallback:` kwarg, and
// anything similar a user would have on their own component.
pub use whisker_runtime::view::{EachFn, Fallback, ItemFn, KeyFn, WhenFn};

/// Built-in tag builders. The `render!` macro lowers each built-in
/// element invocation (`view { style: "x", on_tap: || {} }`) into a
/// builder method chain on one of these types
/// (`__tags::view().style(|| "x").on_tap(|| {}).__h()`). Methods
/// internally invoke the imperative runtime primitives
/// (`create_element`, `set_inline_styles`, …).
///
/// **Why a builder chain instead of struct-init or imperative
/// codegen:** rust-analyzer's auto-completion picks up methods on
/// known receiver types far more reliably than field names inside
/// proc-macro-emitted struct-init expressions. The user typing
/// `view { sty|` inside `render! { … }` ends up — after the macro
/// expansion + cursor-position mapping — at `.style|(…)` in the
/// chain, which is exactly the shape RA's method-completion
/// engine knows how to drive. Same mechanism Leptos uses for its
/// `view!` DX.
///
/// Internal. Not part of the public surface — users go through
/// `render!`.
#[doc(hidden)]
pub mod __tags {
    use crate::ElementTag;
    use whisker_runtime::event::{
        bind_typed, AnimationEvent, CustomEvent, ScrollEvent, SelectionChangeEvent,
        TextLayoutEvent, TouchEvent,
    };
    use whisker_runtime::reactive::Signal;
    use whisker_runtime::value::WhiskerValue;
    use whisker_runtime::view::{
        append_child, apply_attr, apply_attr_bool, apply_attr_int, apply_attr_owned,
        create_element, create_element_by_name, create_phantom_element,
        install_list_native_item_provider, set_event_listener, set_update_list_info, BindType,
        Element,
    };

    // ---- The common builder surface -------------------------------------
    //
    // Styling, the universal Lynx attributes, the full built-in event
    // set, and children — every built-in tag shares these, so they
    // live **once** on the `ElementBuilder` trait as provided
    // methods rather than being copy-pasted across six structs.
    //
    // ## Why a trait (and not `macro_rules!`)
    //
    // An earlier note here recorded that rust-analyzer's
    // method-completion engine does *not* surface methods produced
    // by a `macro_rules!` expansion inside an `impl` block — which
    // is why the per-tag methods used to be hand-inlined. A trait is
    // different: trait methods are first-class items RA indexes and
    // completes normally, **provided the trait is in scope**. The
    // `render!` / `#[component]` expansions bring it into scope with
    // `use ::whisker::__tags::ElementBuilder as _;` right before the
    // builder chain, so `view(on_|…)` kwarg completion still works.
    // (`crates/whisker-macros/tests/ra_completion.rs` is the
    // end-to-end guard.)
    //
    // Tag-specific value attributes (`image::src`, `text::value`, …)
    // stay as inherent methods on each struct, below.

    /// Shared builder methods for every built-in element tag.
    ///
    /// Each method consumes `self` and returns it, so calls chain:
    /// `view().style(…).on_tap(…).child(…)`. Reactive-capable
    /// attributes accept any `Into<Signal<T>>` (a static value, a
    /// `ReadSignal`, an `RwSignal`, …) and re-apply on change.
    pub trait ElementBuilder: Sized {
        /// The underlying Lynx element handle. Implemented by each
        /// tag struct as `self.handle`.
        #[doc(hidden)]
        fn __element(&self) -> Element;

        // ---- Styling ----------------------------------------------------

        /// Inline CSS (`SetRawInlineStyles`).
        ///
        /// Accepts any value that converts into [`crate::Style`] — a
        /// [`whisker_css::Css`] builder, a `String` / `&str` raw CSS
        /// literal, or a reactive [`ReadSignal`] / [`RwSignal`] of
        /// either form. Reactive variants re-apply the CSS via the
        /// element's internal `effect` whenever the underlying
        /// signal changes.
        fn style<V>(self, v: V) -> Self
        where
            V: Into<crate::Style>,
        {
            crate::apply_style(self.__element(), v);
            self
        }

        /// Lynx `class` attribute.
        fn class<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "class", v);
            self
        }

        /// Catch-all for any Lynx attribute not covered by a named
        /// method. Name is taken verbatim (already kebab-cased by
        /// the `render!` lowering).
        fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), name, v);
            self
        }

        // ---- Common attributes (shared by all built-in elements) --------

        /// `id` — element identifier for selection / events.
        fn id<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "id", v);
            self
        }

        /// `name` — for native-side `findViewByName` lookup.
        fn name<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "name", v);
            self
        }

        /// `data-<key>` custom attribute, surfaced back on events via
        /// `Target::dataset`.
        fn data<V>(self, key: &str, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr_owned(self.__element(), format!("data-{key}"), v);
            self
        }

        /// `accessibility-label` — VoiceOver / TalkBack text.
        fn accessibility_label<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "accessibility-label", v);
            self
        }

        /// `accessibility-trait` — `"button"` / `"image"` / `"text"`
        /// / `"none"`.
        fn accessibility_trait<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "accessibility-trait", v);
            self
        }

        /// `accessibility-element` — enable/disable a11y for this node.
        fn accessibility_element<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "accessibility-element", v);
            self
        }

        /// `user-interaction-enabled` — whether the node responds to
        /// touch events.
        fn user_interaction_enabled<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "user-interaction-enabled", v);
            self
        }

        /// `event-through` — display-only mode (pass touches through).
        fn event_through<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "event-through", v);
            self
        }

        /// `exposure-id` — opt the node into exposure detection.
        fn exposure_id<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "exposure-id", v);
            self
        }

        /// `exposure-scene` — exposure scene identifier (pairs with
        /// `exposure-id` for scoping exposure monitoring).
        fn exposure_scene<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "exposure-scene", v);
            self
        }

        /// `exposure-area` — viewport-intersection ratio threshold that
        /// counts as "exposed" (e.g. `"0.5"` or `"50%"`).
        fn exposure_area<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "exposure-area", v);
            self
        }

        /// `a11y-id` — separate identifier for accessibility nodes.
        fn a11y_id<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "a11y-id", v);
            self
        }

        /// `accessibility-elements` — customize child focus order by a
        /// comma-separated list of element ids.
        fn accessibility_elements<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "accessibility-elements", v);
            self
        }

        /// `accessibility-elements-hidden` — hide this node and its
        /// children from accessibility.
        fn accessibility_elements_hidden<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "accessibility-elements-hidden", v);
            self
        }

        /// `accessibility-exclusive-focus` — restrict accessibility
        /// focus to this node's children.
        fn accessibility_exclusive_focus<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "accessibility-exclusive-focus", v);
            self
        }

        // ---- Native touch / gesture coordination ------------------------
        //
        // Whisker delivers events through Lynx's hit-testing + reporter,
        // so these tune what reaches it: expand the hit area, hand
        // gestures to / withhold them from Lynx's native scroll, etc.
        // Advanced — most apps never need them.

        /// `hit-slop` — expand the touch-responsive area beyond the
        /// element's bounds (e.g. `"10px"`, or per-side
        /// `"{top:10,bottom:10}"`).
        fn hit_slop<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "hit-slop", v);
            self
        }

        /// `native-interaction-enabled` — let the platform layer consume
        /// gestures on this node.
        fn native_interaction_enabled<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "native-interaction-enabled", v);
            self
        }

        /// `block-native-event` — block platform gestures (e.g. an
        /// underlying native scroll) from firing outside Lynx while a
        /// touch is on this node.
        fn block_native_event<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "block-native-event", v);
            self
        }

        /// `consume-slide-event` — consume swipes within given angle
        /// ranges so an ancestor scroll doesn't also act on them
        /// (e.g. `"[[0,45]]"`).
        fn consume_slide_event<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "consume-slide-event", v);
            self
        }

        /// `pan-intercept-direction` — block swipe gestures in a
        /// direction: `horizontal` / `vertical` / `none`.
        fn pan_intercept_direction<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "pan-intercept-direction", v);
            self
        }

        /// `pan-intercept-scope` — scope of [`pan_intercept_direction`]:
        /// `self` / `ancestors` / `descendants` / ….
        fn pan_intercept_scope<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_attr(self.__element(), "pan-intercept-scope", v);
            self
        }

        /// `flatten` — Android-only: force a real Android View for this
        /// node (opts out of flattening). `false` lets Lynx flatten it.
        fn flatten<V>(self, v: V) -> Self
        where
            V: Into<Signal<bool>>,
        {
            apply_attr(self.__element(), "flatten", v);
            self
        }

        // ---- Events: touch / tap / click → `TouchEvent` -----------------
        //
        // Each touch event exposes the four Lynx handler kinds as a
        // 1:1 naming convention (so `on_tap` ↔ `bindtap`,
        // `on_tap_catch` ↔ `catchtap`, `on_capture_tap` ↔
        // `capture-bindtap`, `on_capture_tap_catch` ↔
        // `capture-catchtap`):
        //
        //   - `on_<event>`              — bubble phase, doesn't stop.
        //   - `on_<event>_catch`        — bubble phase, stops here.
        //   - `on_capture_<event>`      — capture phase, doesn't stop.
        //   - `on_capture_<event>_catch`— capture phase, stops here.
        //
        // Capture handlers fire on the way *down* the element tree
        // (root → target), bubble handlers on the way *up* (target →
        // root); a `catch` handler stops the event from continuing
        // along the chain after it fires. These set real Lynx handlers
        // so the engine's native chain does the propagation.

        /// `tap` — single tap (won't fire if the finger moved far).
        /// Bubble phase, lets the event continue up the chain.
        fn on_tap<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "tap", BindType::Bind, f);
            self
        }
        /// `tap`, bubble phase — **stops** propagation at this element.
        fn on_tap_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "tap", BindType::Catch, f);
            self
        }
        /// `tap`, capture phase (fires before descendants) — doesn't stop.
        fn on_capture_tap<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "tap", BindType::CaptureBind, f);
            self
        }
        /// `tap`, capture phase — **stops** propagation before it reaches
        /// the target.
        fn on_capture_tap_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "tap", BindType::CaptureCatch, f);
            self
        }

        /// `longpress` — ~500ms press (mutually exclusive with `tap`).
        fn on_longpress<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "longpress", BindType::Bind, f);
            self
        }
        /// `longpress`, bubble phase — **stops** propagation here.
        fn on_longpress_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "longpress", BindType::Catch, f);
            self
        }
        /// `longpress`, capture phase — doesn't stop.
        fn on_capture_longpress<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "longpress", BindType::CaptureBind, f);
            self
        }
        /// `longpress`, capture phase — **stops** propagation.
        fn on_capture_longpress_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "longpress", BindType::CaptureCatch, f);
            self
        }

        /// `click` — click on the nearest listening node.
        fn on_click<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "click", BindType::Bind, f);
            self
        }
        /// `click`, bubble phase — **stops** propagation here.
        fn on_click_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "click", BindType::Catch, f);
            self
        }
        /// `click`, capture phase — doesn't stop.
        fn on_capture_click<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "click", BindType::CaptureBind, f);
            self
        }
        /// `click`, capture phase — **stops** propagation.
        fn on_capture_click_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "click", BindType::CaptureCatch, f);
            self
        }

        /// `touchstart` — finger touches the surface.
        fn on_touchstart<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchstart", BindType::Bind, f);
            self
        }
        /// `touchstart`, bubble phase — **stops** propagation here.
        fn on_touchstart_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchstart", BindType::Catch, f);
            self
        }
        /// `touchstart`, capture phase — doesn't stop.
        fn on_capture_touchstart<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchstart", BindType::CaptureBind, f);
            self
        }
        /// `touchstart`, capture phase — **stops** propagation.
        fn on_capture_touchstart_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchstart", BindType::CaptureCatch, f);
            self
        }

        /// `touchmove` — finger moves on the surface.
        fn on_touchmove<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchmove", BindType::Bind, f);
            self
        }
        /// `touchmove`, bubble phase — **stops** propagation here.
        fn on_touchmove_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchmove", BindType::Catch, f);
            self
        }
        /// `touchmove`, capture phase — doesn't stop.
        fn on_capture_touchmove<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchmove", BindType::CaptureBind, f);
            self
        }
        /// `touchmove`, capture phase — **stops** propagation.
        fn on_capture_touchmove_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchmove", BindType::CaptureCatch, f);
            self
        }

        /// `touchend` — finger leaves the surface.
        fn on_touchend<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchend", BindType::Bind, f);
            self
        }
        /// `touchend`, bubble phase — **stops** propagation here.
        fn on_touchend_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchend", BindType::Catch, f);
            self
        }
        /// `touchend`, capture phase — doesn't stop.
        fn on_capture_touchend<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchend", BindType::CaptureBind, f);
            self
        }
        /// `touchend`, capture phase — **stops** propagation.
        fn on_capture_touchend_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchend", BindType::CaptureCatch, f);
            self
        }

        /// `touchcancel` — touch interrupted by the system / a gesture.
        fn on_touchcancel<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchcancel", BindType::Bind, f);
            self
        }
        /// `touchcancel`, bubble phase — **stops** propagation here.
        fn on_touchcancel_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchcancel", BindType::Catch, f);
            self
        }
        /// `touchcancel`, capture phase — doesn't stop.
        fn on_capture_touchcancel<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchcancel", BindType::CaptureBind, f);
            self
        }
        /// `touchcancel`, capture phase — **stops** propagation.
        fn on_capture_touchcancel_catch<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "touchcancel", BindType::CaptureCatch, f);
            self
        }

        // ---- Events: lifecycle → `CustomEvent` --------------------------

        /// `layoutchange` — reports position after layout completes.
        fn on_layoutchange<F: Fn(CustomEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "layoutchange", BindType::Bind, f);
            self
        }

        /// `uiappear` — node entered the visible screen area.
        fn on_uiappear<F: Fn(CustomEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "uiappear", BindType::Bind, f);
            self
        }

        /// `uidisappear` — node left the visible screen area.
        fn on_uidisappear<F: Fn(CustomEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "uidisappear", BindType::Bind, f);
            self
        }

        // ---- Events: animation / transition → `AnimationEvent` ----------

        /// `animationstart` — keyframe animation began.
        fn on_animationstart<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "animationstart", BindType::Bind, f);
            self
        }

        /// `animationend` — keyframe animation completed.
        fn on_animationend<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "animationend", BindType::Bind, f);
            self
        }

        /// `animationcancel` — keyframe animation interrupted.
        fn on_animationcancel<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "animationcancel", BindType::Bind, f);
            self
        }

        /// `animationiteration` — keyframe animation cycle boundary.
        fn on_animationiteration<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "animationiteration", BindType::Bind, f);
            self
        }

        /// `transitionstart` — transition animation began.
        fn on_transitionstart<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "transitionstart", BindType::Bind, f);
            self
        }

        /// `transitionend` — transition animation completed.
        fn on_transitionend<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "transitionend", BindType::Bind, f);
            self
        }

        /// `transitioncancel` — transition animation interrupted.
        fn on_transitioncancel<F: Fn(AnimationEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.__element(), "transitioncancel", BindType::Bind, f);
            self
        }

        // ---- Events: raw escape hatch -----------------------------------

        /// Bind any event by name, receiving the raw [`WhiskerValue`]
        /// body. For custom / dynamic event names not covered by a
        /// typed `on_<event>` method above. Bubble phase, doesn't stop
        /// propagation — for the catch / capture variants use
        /// [`bind`](Self::bind).
        fn on<F: Fn(WhiskerValue) + 'static>(self, event: &'static str, f: F) -> Self {
            self.bind(event, BindType::Bind, f)
        }

        /// Bind any event by name with an explicit propagation
        /// [`BindType`] (bind / catch / capture-bind / capture-catch),
        /// receiving the raw [`WhiskerValue`] body. The general escape
        /// hatch behind the typed `on_<event>` / `on_<event>_catch` /
        /// `on_capture_<event>[_catch]` methods.
        fn bind<F: Fn(WhiskerValue) + 'static>(
            self,
            event: &'static str,
            bind_type: BindType,
            f: F,
        ) -> Self {
            set_event_listener(
                self.__element(),
                event,
                bind_type,
                ::std::boxed::Box::new(f),
            );
            self
        }

        // ---- Children ---------------------------------------------------

        /// Append a child handle.
        fn child(self, child: Element) -> Self {
            append_child(self.__element(), child);
            self
        }

        // ---- Ref --------------------------------------------------------

        /// Bind an [`ElementRef`](crate::ElementRef) to this element so
        /// its methods (`bounding_client_rect`, `take_screenshot`, …)
        /// can be invoked after mount. `render!` routes the `ref:`
        /// kwarg here (`view(ref: my_ref) { … }`).
        fn bind_ref(self, r: crate::ElementRef) -> Self {
            r.__bind(self.__element());
            self
        }

        /// Finish building and return the underlying handle.
        #[doc(hidden)]
        fn __h(self) -> Element {
            self.__element()
        }
    }

    /// `<page>` — top-level container Lynx mounts as the root of an
    /// app. Holds the screen-level `style=` and a single content
    /// subtree.
    #[allow(non_camel_case_types)]
    pub struct page {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __page_ctor() -> page {
        page {
            handle: create_element(ElementTag::Page),
        }
    }
    impl ElementBuilder for page {
        fn __element(&self) -> Element {
            self.handle
        }
    }

    /// `<view>` — Lynx's flex container. The basic layout primitive:
    /// a rectangular box that lays out children with CSS flexbox
    /// (`<View>` in React Native, `<div>` on the web).
    #[allow(non_camel_case_types)]
    pub struct view {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __view_ctor() -> view {
        view {
            handle: create_element(ElementTag::View),
        }
    }
    impl ElementBuilder for view {
        fn __element(&self) -> Element {
            self.handle
        }
    }

    /// `<text>` — text container. The glyphs live in `raw_text`
    /// children the macro creates from string-literal children.
    #[allow(non_camel_case_types)]
    pub struct text {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __text_ctor() -> text {
        text {
            handle: create_element(ElementTag::Text),
        }
    }
    impl ElementBuilder for text {
        fn __element(&self) -> Element {
            self.handle
        }
    }
    impl text {
        /// `value` — the text string. Lynx renders `<text>` content
        /// from a child `<raw-text text="…">`, so this creates that
        /// child and sets its `text` attribute (reactive-capable).
        pub fn value<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            let raw = create_element(ElementTag::RawText);
            append_child(self.handle, raw);
            apply_attr(raw, "text", v);
            self
        }

        // ---- text attributes (reactive-capable) ---------------------

        /// `text-maxline` — max displayed lines (-1 = unlimited).
        pub fn text_maxline<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr(self.handle, "text-maxline", v);
            self
        }
        /// `text-selection` — allow the user to select the text.
        pub fn text_selection<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr(self.handle, "text-selection", v);
            self
        }
        /// `include-font-padding` — add font padding (Android).
        pub fn include_font_padding<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr(self.handle, "include-font-padding", v);
            self
        }
        /// `tail-color-convert` — control ellipsis color inheritance.
        pub fn tail_color_convert<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr(self.handle, "tail-color-convert", v);
            self
        }
        /// `text-single-line-vertical-align` — `normal` (default) /
        /// `top` / `center` / `bottom`.
        pub fn text_single_line_vertical_align<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "text-single-line-vertical-align", v);
            self
        }
        /// `custom-context-menu` — enable a custom selection context menu.
        pub fn custom_context_menu<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr(self.handle, "custom-context-menu", v);
            self
        }
        /// `custom-text-selection` — developer-controlled selection logic.
        pub fn custom_text_selection<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr(self.handle, "custom-text-selection", v);
            self
        }

        // ---- text-specific events (CustomEvent → bind only) ---------

        /// `layout` — fired after text layout completes. The
        /// [`TextLayoutEvent`] reports line count, per-line ranges, and
        /// the laid-out size.
        pub fn on_layout<F: Fn(TextLayoutEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "layout", BindType::Bind, f);
            self
        }

        /// `selectionchange` — fired when the selected text range
        /// changes (requires text selection to be enabled).
        pub fn on_selectionchange<F: Fn(SelectionChangeEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "selectionchange", BindType::Bind, f);
            self
        }
    }

    /// `<raw-text>` — leaf text node carrying actual glyphs. Created
    /// by the macro from string-literal children of `<text>`.
    #[allow(non_camel_case_types)]
    pub struct raw_text {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __raw_text_ctor() -> raw_text {
        raw_text {
            handle: create_element(ElementTag::RawText),
        }
    }
    impl ElementBuilder for raw_text {
        fn __element(&self) -> Element {
            self.handle
        }
    }
    impl raw_text {
        /// `text` — the glyph string. Reactive-capable.
        pub fn text<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "text", v);
            self
        }
    }

    /// `<scroll-view>` — scrollable container.
    #[allow(non_camel_case_types)]
    pub struct scroll_view {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __scroll_view_ctor() -> scroll_view {
        scroll_view {
            handle: create_element(ElementTag::ScrollView),
        }
    }
    impl ElementBuilder for scroll_view {
        fn __element(&self) -> Element {
            self.handle
        }
    }
    impl scroll_view {
        /// Scroll direction — `"vertical"` (default) or
        /// `"horizontal"`. Lynx's `scroll-orientation` attribute.
        pub fn scroll_orientation<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "scroll-orientation", v);
            self
        }

        // ---- scroll_view attributes (reactive-capable) --------------

        /// `bounces` — bounce effect at the scroll edges. Lynx
        /// reads via `IsBool()`, so the bool path is required.
        pub fn bounces<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr_bool(self.handle, "bounces", v);
            self
        }
        /// `enable-scroll` — allow the user to drag-scroll. Bool
        /// dispatch on the Lynx side.
        pub fn enable_scroll<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr_bool(self.handle, "enable-scroll", v);
            self
        }
        /// `scroll-bar-enable` — show the scrollbar indicator. Bool
        /// dispatch on the Lynx side.
        pub fn scroll_bar_enable<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr_bool(self.handle, "scroll-bar-enable", v);
            self
        }
        /// `initial-scroll-offset` — starting scroll position (px).
        /// Lynx reads via `IsNumber()`.
        pub fn initial_scroll_offset<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr_int(self.handle, "initial-scroll-offset", v);
            self
        }
        /// `initial-scroll-to-index` — child index to jump to on load.
        /// Lynx reads via `IsNumber()`.
        pub fn initial_scroll_to_index<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr_int(self.handle, "initial-scroll-to-index", v);
            self
        }
        /// `upper-threshold` — distance (px) from the top/left edge that
        /// triggers `scrolltoupper`. Lynx reads via `IsNumber()`.
        pub fn upper_threshold<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr_int(self.handle, "upper-threshold", v);
            self
        }
        /// `lower-threshold` — distance (px) from the bottom/right edge
        /// that triggers `scrolltolower`. Lynx reads via `IsNumber()`.
        pub fn lower_threshold<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr_int(self.handle, "lower-threshold", v);
            self
        }

        // ---- scroll events (CustomEvent → bind only) ----------------

        /// `scroll` — fired continuously while scrolling. The
        /// [`ScrollEvent`] `detail` carries the current offset, content
        /// size, per-event delta, and drag state.
        pub fn on_scroll<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "scroll", BindType::Bind, f);
            self
        }

        /// `scrolltoupper` — the upper/left edge reached the
        /// `upper-threshold` visible area.
        pub fn on_scrolltoupper<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "scrolltoupper", BindType::Bind, f);
            self
        }

        /// `scrolltolower` — the lower/right edge reached the
        /// `lower-threshold` visible area.
        pub fn on_scrolltolower<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "scrolltolower", BindType::Bind, f);
            self
        }

        /// `scrollend` — scrolling came to rest.
        pub fn on_scrollend<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "scrollend", BindType::Bind, f);
            self
        }

        /// `contentsizechanged` — the scrollable content size changed.
        pub fn on_contentsizechanged<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "contentsizechanged", BindType::Bind, f);
            self
        }
    }

    // ---- list / list-item (Lynx's virtualized list) ---------------------
    //
    // The standard Lynx `list` creates its items through lepus
    // `componentAtIndex` callbacks the JS framework registers — Whisker
    // has no such runtime. So the `list` builder opts into Lynx's
    // *decoupled native* list: it virtualizes / recycles the actual
    // `<list-item>` children present in the element tree (via the native
    // `ListChildrenHelper`), with no framework callbacks. That fits
    // Whisker's direct-tree model — author code writes
    // `list { list_item { … } … }` like any other container.
    //
    // Two flags gate that mode (see `list_element.cc`):
    //   • `custom-list-name="list-container"` → `ResolveEnableNativeList`
    //     Case 2 sets `disable_list_platform_implementation_ = true`. This
    //     is a *string* compare, so it survives `apply_attr`'s
    //     stringification.
    //   • the decoupled mediator additionally needs `enable_decoupled_list_`,
    //     which `ResolveEnableDecoupledList` only reads from the attr when
    //     the value `IsBool()` — a stringified attr never is, so it falls
    //     back to `LynxEnv::EnableDecoupledList()`, which defaults to
    //     `true`. So `custom-list-name` alone activates the decoupled path.

    /// `<list>` — Lynx native-virtualised, *render-props* shape. The
    /// list builder takes its items source as three kwargs (`each`,
    /// `key`, `children`) and **does not accept a body** — the macro
    /// rejects `list { … }` invocations because items can only come
    /// through the reactive props.
    ///
    /// The three setters are **type-stated**: `__h()` is only
    /// callable when all three have been supplied. Missing any of
    /// them surfaces as a compile-time error at the close of the
    /// builder chain (rather than a runtime panic).
    ///
    /// `__h()` installs:
    ///   1. an `effect` that diffs `each()` against per-key
    ///      bookkeeping, materialises new items + detaches removed
    ///      ones under the list element, eagerly computes each
    ///      item's Lynx `impl_id` (sign), and updates the shared
    ///      items Vec the native-item provider closure reads from.
    ///   2. the `NativeItemProvider` so Lynx's list machinery can
    ///      call `componentAtIndex(i)` and get a sign back without
    ///      re-entering the renderer (the sign is already cached).
    ///   3. `set_update_list_info(handle, count)` on every reactive
    ///      update — what tells Lynx how many slots to lay out.
    #[allow(non_camel_case_types)]
    pub struct list<EachF = (), KeyF = (), ChildF = ()> {
        handle: Element,
        each: EachF,
        key: KeyF,
        children: ChildF,
    }
    #[allow(non_snake_case)]
    pub fn __list_ctor() -> list<(), (), ()> {
        let handle = create_element_by_name("list");
        // Drive the list natively from its tree children rather than through
        // (absent) JS `componentAtIndex` callbacks. `custom-list-name` is the
        // string-compare flag that disables the platform list impl; the
        // decoupled mediator then activates via the env default (true).
        apply_attr::<_, ::std::string::String>(handle, "custom-list-name", "list-container");
        list {
            handle,
            each: (),
            key: (),
            children: (),
        }
    }
    impl<EachF, KeyF, ChildF> ElementBuilder for list<EachF, KeyF, ChildF> {
        fn __element(&self) -> Element {
            self.handle
        }
        // `list` doesn't accept body children — the macro is
        // responsible for rejecting `list { … }` at parse time.
        // Should the user reach this through a non-macro path, the
        // default `.child()` semantics (a regular `append_child`)
        // would still work but is not the supported shape.
    }
    impl<EachF, KeyF, ChildF> list<EachF, KeyF, ChildF> {
        /// `list-type` — `"single"` (default, one column), `"flow"`,
        /// or `"waterfall"`.
        pub fn list_type<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "list-type", v);
            self
        }
        /// `column-count` — number of columns (default 1). Lynx
        /// reads this via `IsNumber()` on the decoupled list
        /// container, so the int path is required — the stringified
        /// `apply_attr` would silently no-op.
        pub fn column_count<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr_int(self.handle, "column-count", v);
            self
        }
        /// `span-count` — preferred grid attribute on newer Lynx
        /// builds (`column-count` is marked `@deprecated` in the
        /// Lynx source). Same numeric-typed dispatch as
        /// [`column_count`]; setting both is the safe shape while
        /// older Lynx builds are still in the field.
        pub fn span_count<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<i32>>,
        {
            apply_attr_int(self.handle, "span-count", v);
            self
        }
        /// `vertical-orientation` — `true` (default) scrolls
        /// vertically, `false` horizontally. Lynx reads via
        /// `IsBool()`, so the bool path is required.
        pub fn vertical_orientation<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr_bool(self.handle, "vertical-orientation", v);
            self
        }
    }
    // ---- Type-stated render-props setters ----
    //
    // Each setter advances one type parameter from `()` to the
    // function-shaped newtype; the `__h()` finaliser is only impl'd
    // on the fully-populated state. The user can call the three in
    // any order — the render! macro emits them in whatever order
    // they appear in the source.
    impl<KeyF, ChildF> list<(), KeyF, ChildF> {
        pub fn each<T: 'static, F>(
            self,
            f: F,
        ) -> list<::whisker_runtime::view::EachFn<T>, KeyF, ChildF>
        where
            F: ::std::convert::Into<::whisker_runtime::view::EachFn<T>>,
        {
            list {
                handle: self.handle,
                each: f.into(),
                key: self.key,
                children: self.children,
            }
        }
    }
    impl<EachF, ChildF> list<EachF, (), ChildF> {
        pub fn key<T: 'static, K: 'static, F>(
            self,
            f: F,
        ) -> list<EachF, ::whisker_runtime::view::KeyFn<T, K>, ChildF>
        where
            F: ::std::convert::Into<::whisker_runtime::view::KeyFn<T, K>>,
        {
            list {
                handle: self.handle,
                each: self.each,
                key: f.into(),
                children: self.children,
            }
        }
    }
    impl<EachF, KeyF> list<EachF, KeyF, ()> {
        pub fn children<T: 'static, F>(
            self,
            f: F,
        ) -> list<EachF, KeyF, ::whisker_runtime::view::ItemFn<T>>
        where
            F: ::std::convert::Into<::whisker_runtime::view::ItemFn<T>>,
        {
            list {
                handle: self.handle,
                each: self.each,
                key: self.key,
                children: f.into(),
            }
        }
    }
    // ---- Finaliser, only on fully-populated state ----
    impl<T, K>
        list<
            ::whisker_runtime::view::EachFn<T>,
            ::whisker_runtime::view::KeyFn<T, K>,
            ::whisker_runtime::view::ItemFn<T>,
        >
    where
        T: 'static,
        K: ::std::cmp::Eq + ::std::hash::Hash + ::std::clone::Clone + 'static,
    {
        /// Finalise the builder: install the reactive-items effect +
        /// the native-item provider + the initial count broadcast.
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            let handle = self.handle;
            let each = self.each;
            let key = self.key;
            let children = self.children;

            // Shared items Vec — the provider closure (installed
            // below, reads-only) and the effect (rewrites on every
            // diff) both clone the Rc.
            let items: ::std::rc::Rc<::std::cell::RefCell<::std::vec::Vec<(Element, i32)>>> =
                ::std::rc::Rc::new(::std::cell::RefCell::new(::std::vec::Vec::new()));

            // Native item provider — reads sign by index from the
            // shared items Vec. Must NOT call back into the renderer
            // (Lynx's layout C++ that invokes this closure is itself
            // inside `with_renderer`'s RefCell borrow).
            let items_for_provider = items.clone();
            let provider = ::whisker_runtime::view::list_provider::NativeItemProvider {
                component_at_index: ::std::boxed::Box::new(move |index, _op, _reuse| {
                    items_for_provider
                        .borrow()
                        .get(index as usize)
                        .map(|&(_, sign)| sign)
                        .unwrap_or(::whisker_runtime::view::list_provider::INVALID_ITEM_INDEX)
                }),
                enqueue_component: ::std::option::Option::None,
            };
            install_list_native_item_provider(handle, provider);

            // Reactive items effect. Diffs `each()` against
            // per-key bookkeeping, materialises new items + detaches
            // removed ones under `handle`, sets `item-key` on each,
            // rebuilds the items Vec + broadcasts the new count.
            //
            // Owner cascade: per-item owners are detached
            // (`create_owner(None)`); the effect explicitly disposes
            // them on diff. When the surrounding component disposes,
            // the released list element + items are torn down
            // through their respective owner releases.
            struct ListEntry {
                owner: ::whisker_runtime::reactive::OwnerId,
                handle: Element,
            }
            let entries: ::std::rc::Rc<
                ::std::cell::RefCell<::std::collections::HashMap<K, ListEntry>>,
            > = ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections::HashMap::new()));

            ::whisker_runtime::reactive::effect(move || {
                let new_items = each.call();
                let mut new_entries: ::std::collections::HashMap<K, ListEntry> =
                    ::std::collections::HashMap::new();
                let mut new_keys: ::std::vec::Vec<K> =
                    ::std::vec::Vec::with_capacity(new_items.len());

                let mut old = ::std::mem::take(&mut *entries.borrow_mut());

                for item in new_items {
                    let k = key.call(&item);
                    if let ::std::option::Option::Some(existing) = old.remove(&k) {
                        new_entries.insert(k.clone(), existing);
                    } else {
                        let item_owner = ::whisker_runtime::reactive::create_owner(None);
                        let li = ::whisker_runtime::reactive::with_owner(item_owner, || {
                            // Auto-wrap: the user's `children(item)` returns
                            // arbitrary content (a story_row view, a custom
                            // component, etc.). Lynx's <list> requires its
                            // direct children to be UIComponent on the
                            // platform side (LynxUIListItem on iOS,
                            // UIListItem on Android — both
                            // UIComponent-typed). Wrapping in <list-item>
                            // is what realises that contract; user code
                            // never has to write list_item itself.
                            let li = create_element_by_name("list-item");
                            let content = children.call(item);
                            append_child(li, content);
                            append_child(handle, li);
                            li
                        });
                        new_entries.insert(
                            k.clone(),
                            ListEntry {
                                owner: item_owner,
                                handle: li,
                            },
                        );
                    }
                    new_keys.push(k);
                }

                // Disappeared items: detach + dispose.
                for (_, entry) in old.drain() {
                    ::whisker_runtime::view::remove_child(handle, entry.handle);
                    ::whisker_runtime::reactive::dispose_owner(entry.owner);
                }

                // Rebuild items Vec in new key order, capturing
                // each leaf handle's Lynx sign (eager, from a safe
                // scope — provider closure stays re-entrancy-safe).
                let mut new_items_vec: ::std::vec::Vec<(Element, i32)> =
                    ::std::vec::Vec::with_capacity(new_keys.len());
                for k in &new_keys {
                    if let ::std::option::Option::Some(entry) = new_entries.get(k) {
                        apply_attr_owned::<_, ::std::string::String>(
                            entry.handle,
                            ::std::string::String::from("item-key"),
                            ::std::format!("w_{}", new_items_vec.len()),
                        );
                        let sign = ::whisker_runtime::view::element_sign(entry.handle);
                        new_items_vec.push((entry.handle, sign));
                    }
                }

                let count = new_items_vec.len() as i32;
                *items.borrow_mut() = new_items_vec;
                *entries.borrow_mut() = new_entries;

                set_update_list_info(handle, count);
            });

            handle
        }
    }
    // (The list_type / column_count / vertical_orientation methods
    // moved up into the type-state-generic `impl<E, K, C> list<E, K, C>`
    // block so they're available regardless of which setters have
    // been called yet.)

    // `list_item` is an internal Lynx-side wrapper the `list`
    // render-props builder auto-creates around each item slot. It
    // realises the platform UI layer's `UIComponent` contract
    // (`LynxUIListItem : LynxUIComponent` on iOS, `UIListItem extends
    // UIComponent` on Android) that the list recycler / sticky /
    // virtualisation machinery depends on. The list builder calls
    // `create_element_by_name("list-item")` directly from its
    // `__h()` effect; user code never reaches this builder.
    #[allow(non_camel_case_types, dead_code)]
    pub(crate) struct list_item {
        handle: Element,
    }
    #[allow(non_snake_case, dead_code)]
    pub(crate) fn __list_item_ctor() -> list_item {
        list_item {
            handle: create_element_by_name("list-item"),
        }
    }
    impl ElementBuilder for list_item {
        fn __element(&self) -> Element {
            self.handle
        }
    }
    #[allow(dead_code)]
    impl list_item {
        /// `item-key` — stable identity for this item, used by the list
        /// for recycling / diffing. Should be unique among siblings.
        pub fn item_key<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "item-key", v);
            self
        }
    }

    /// `<fragment>` — *transparent grouping container*. Mounts as a
    /// phantom element ([`create_phantom_element`]) the runtime
    /// tracks in its mirror but never forwards to Lynx. Children
    /// appended under a fragment are hoisted to the fragment's
    /// nearest non-phantom ancestor in the Lynx tree, in source
    /// order — so on screen the fragment is *invisible*, while in
    /// user code it serves as a stable grouping point for reactive
    /// children.
    ///
    /// **What it's for**: Whisker's `For` / `Show` control flow
    /// (`for_each` / `show`) both `return` a fragment. Any
    /// user-defined control flow follows the same pattern — a
    /// function that allocates a fragment, installs an effect, and
    /// mutates the fragment's children — so a custom control flow
    /// looks and feels exactly like the built-in `For` / `Show`.
    ///
    /// **Restrictions**: a fragment carries no styling, attributes,
    /// or event listeners — those would have no Lynx element to
    /// attach to. The builder exposes only `.child(...)`. Fragments
    /// inside a `<list>` are not supported (use the list builder's
    /// `each` / `key` / `children` render-props instead).
    #[allow(non_camel_case_types)]
    pub struct fragment {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __fragment_ctor() -> fragment {
        fragment {
            handle: create_phantom_element(),
        }
    }
    impl ElementBuilder for fragment {
        fn __element(&self) -> Element {
            self.handle
        }
    }
}

// Worker-thread → main-thread marshaling. The typical use case is
// "fetch on a worker thread, update signal on the main thread":
//
//     std::thread::spawn(move || {
//         let result = blocking_fetch();
//         run_on_main_thread(move || data.set(Some(result)));
//     });
pub use whisker_runtime::main_thread::run_on_main_thread;

/// Whisker platform module invocation entry point.
///
/// Phase 7-Φ.E API surface — `WhiskerValue` tagged-union type +
/// `invoke` / `invoke_async` callers that cross the C bridge to
/// platform-side modules (Obj-C class on iOS, Kotlin class on
/// Android, both inheriting from Lynx's `LynxModule`).
///
/// The `#[whisker::platform_module]` proc macro (Phase 7-Φ.E.5)
/// generates type-safe Rust proxies that wrap [`invoke`] /
/// [`invoke_async`] — direct callers use `whisker::platform_module`
/// when they need access to the raw `WhiskerValue` enum.
pub mod platform_module {
    pub use whisker_driver::module::{
        from_raw, invoke, invoke_async, WhiskerModuleError, WhiskerValue,
    };
}

/// Internal runtime entry points used by code the `#[whisker::main]` macro
/// expands to. Not stable, not for direct use.
#[doc(hidden)]
pub mod __main_runtime {
    pub use whisker_driver::bootstrap::{run, tick};

    /// Wrap one invocation of the user's `app` function for hot-patch
    /// dispatch. The `#[whisker::main]` macro calls this unconditionally
    /// from inside the user crate so we don't need a user-crate-local
    /// `hot-reload` feature flag to gate the call site.
    ///
    /// The cfg flip happens here, at whisker's compile-time, on whisker's
    /// own `hot-reload` feature:
    ///
    /// - **on** (`whisker run` / Tier 1): body is
    ///   `subsecond::call(|| f())`. The `#[inline(always)]` makes the
    ///   body land in the *user crate's* compilation unit at every
    ///   call site, so the wrapper closure's `<F as HotFunction<()>>::
    ///   call_it` monomorphization is part of `libhello_world.so`
    ///   (host) *and* `target/.whisker/patches/libhello_world.so` (patch).
    ///   That's the symbol `subsecond::apply_patch`'s JumpTable maps
    ///   host → patch; without it, hot patches don't dispatch and the
    ///   screen keeps showing pre-edit content.
    /// - **off** (release): body collapses to `f()`, `subsecond` is
    ///   not pulled in at all.
    use whisker_runtime::view::Element;

    #[cfg(feature = "hot-reload")]
    #[inline(always)]
    pub fn call_user_app(f: fn() -> Element) -> Element {
        // `move` is load-bearing: without it, `|| f()` captures `f` by
        // *reference* (the body only reads `f`, and `f`'s `Copy`-ness is
        // not enough to flip Rust to by-value capture). Subsecond's
        // `transmute_copy` reads the closure's first 8 bytes as the
        // dispatch key — by-ref capture stores `&f` (a stack address) in
        // that slot, so every lookup misses with a stack-shaped key.
        // `move` forces by-value capture so the slot holds the actual
        // `f` fn pointer, which is the runtime address the JumpTable's
        // keys match against. Clippy's `redundant_closure` lint
        // suggests replacing `move || f()` with `f` — load-bearing
        // wrong, see comment above.
        #[allow(clippy::redundant_closure)]
        {
            ::subsecond::call(move || f())
        }
    }

    #[cfg(not(feature = "hot-reload"))]
    #[inline(always)]
    pub fn call_user_app(f: fn() -> Element) -> Element {
        f()
    }
}

/// Hot-reload dispatcher namespace exposed for the `#[component]`
/// macro. With the `hot-reload` feature on, this re-exports
/// `subsecond::call`; with it off, a no-op shim that just calls the
/// closure directly.
///
/// The macro emits `::whisker::__hot::call(move || { #block })`
/// **inline at the user crate's source position**. That placement is
/// the load-bearing detail: the closure type (and thus its
/// `<F as HotFunction>::call_it` monomorphization) lives at the
/// user crate's mangled path, which is what `apply_patch`'s
/// JumpTable entries key on. Wrapping the call through a helper
/// closure that lives in this crate (as the earlier
/// `call_component_body` attempt did) puts the dispatchable
/// `call_it` at a whisker-side path that the user-crate patch
/// never touches — and hot reload silently fails.
#[doc(hidden)]
pub mod __hot {
    #[cfg(feature = "hot-reload")]
    pub use ::subsecond::call;

    #[cfg(not(feature = "hot-reload"))]
    #[inline(always)]
    pub fn call<O>(mut f: impl FnMut() -> O) -> O {
        f()
    }
}

/// Common imports for Whisker app code.
pub mod prelude {
    pub use crate::Children;
    pub use crate::ElementTag;
    pub use crate::{component, main, render};
    // Phase 7-Φ.H.2 — `ElementRef<T>` + `element_ref::<T>()` for
    // imperative element-method dispatch (video.play(), etc.).
    pub use crate::{
        computed, effect, on_cleanup, on_mount, provide_context, resource, resource_sync,
        run_blocking, run_on_main_thread, signal, spawn_local, use_context, with_context,
        ReadSignal, Resource, ResourceState, RwSignal, Signal, StoredValue, WriteSignal,
    };
    // Built-in control flow (PascalCase aliases generated by
    // `#[component]` on `for_each` / `show` in `control_flow.rs`).
    pub use crate::{ForEach, ForEachProps, Show, ShowProps};
    // Function-shaped prop types for control-flow components.
    pub use crate::{
        element_ref, BoundingClientRect, ElementHandle, ElementRef, RefError, ScrollInfo,
        ScrollViewHandle, TextBoundingRect, TextHandle,
    };
    pub use crate::{EachFn, Fallback, ItemFn, KeyFn, WhenFn};
    // Type-safe CSS builder. Pulled into the prelude so
    // `Css::new().display_flex().padding(px(8))` is reachable
    // without explicit imports. Numeric-literal extension traits
    // (`px(8)`, `8.px()`, `45.deg()`, …) come in via the wildcard
    // `ext::*` re-export.
    pub use crate::css::ext::*;
    pub use crate::css::{
        AlignItems, Border, Color, Css, Display, Flex, FlexDirection, FlexWrap, JustifyContent,
        Length, NamedColor, ToCss,
    };
    // The `css!` macro lives in whisker-css's macro namespace; it
    // coexists with the `crate::css` module re-export above because
    // the macro and module namespaces are disjoint.
    pub use crate::css::css;
    // Re-export the `__tags` struct names so RA can complete
    // `vie|` → `view`, `te|` → `text`, etc. when the user is
    // typing a tag name inside render! (the macro source position
    // is a value-expression context to RA; it does identifier
    // completion against the surrounding scope). Without these
    // in scope nothing matches `vie...` and no candidates appear.
    //
    // This is safe to mix with kwarg completion (`view(sty|)`)
    // because the macro now unconditionally emits `.name(())` for
    // every partial kwarg — RA's macro-expansion completion path
    // sees the method-call shape and ignores whatever else `view`
    // resolves to in source. (Previous breakage where re-exporting
    // these blocked kwarg completion was a separate bug — the
    // prefix-match heuristic that's since been removed.)
    #[doc(hidden)]
    pub use crate::__tags::{fragment, list, page, raw_text, scroll_view, text, view};
    // `list_item` intentionally absent — the `list` render-props
    // builder auto-wraps every item internally; user code never
    // reaches for `list_item` directly.
}
