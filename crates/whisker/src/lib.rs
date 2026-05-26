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

pub use whisker_app_config as app_config;
pub use whisker_runtime as runtime;

// Re-export the element tag enum the macro emit references through
// `::whisker::ElementTag`. The C bridge keys element creation off
// the same enum.
pub use whisker_runtime::element::ElementTag;

pub use whisker_macros::{component, main, module_component, render};

// Phase 7-Φ.H.2 — `ElementRef<T>` is the Rust-side handle for
// invoking methods on a mounted platform component. `element_ref::<T>()`
// allocates a fresh, unbound ref; the `#[whisker::module_component]`
// macro binds it on mount when passed as the `ref:` prop.
pub use whisker_driver::{element_ref, ElementRef, RefError};

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
        AnimationEvent, BindType, CustomEvent, Event, Point, Target, Touch, TouchEvent,
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
// Control-flow components used by the `render!` macro.
pub use whisker_runtime::view::{for_each, show};
// `Children` is the conventional prop type for components that wrap
// non-kwarg child nodes in their `render!` invocation.
pub use whisker_runtime::view::Children;

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
    use whisker_runtime::event::{bind_typed, AnimationEvent, CustomEvent, TouchEvent};
    use whisker_runtime::reactive::Signal;
    use whisker_runtime::value::WhiskerValue;
    use whisker_runtime::view::{
        append_child, apply_attr, apply_attr_owned, apply_styles, create_element,
        set_event_listener, BindType, Element,
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
        fn style<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_styles(self.__element(), v);
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

    /// `<image>` — bitmap element. `src` is the image URL / resource.
    #[allow(non_camel_case_types)]
    pub struct image {
        handle: Element,
    }
    #[allow(non_snake_case)]
    pub fn __image_ctor() -> image {
        image {
            handle: create_element(ElementTag::Image),
        }
    }
    impl ElementBuilder for image {
        fn __element(&self) -> Element {
            self.handle
        }
    }
    impl image {
        /// `src` — image URL or resource name. Reactive-capable.
        pub fn src<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "src", v);
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
        // keys match against.
        ::subsecond::call(move || f())
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
        computed, effect, for_each, on_cleanup, on_mount, provide_context, resource, resource_sync,
        run_blocking, run_on_main_thread, show, signal, spawn_local, use_context, with_context,
        ReadSignal, Resource, ResourceState, RwSignal, Signal, StoredValue, WriteSignal,
    };
    pub use crate::{element_ref, ElementRef, RefError};
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
    pub use crate::__tags::{image, page, raw_text, scroll_view, text, view};
}
