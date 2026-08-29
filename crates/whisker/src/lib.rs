//! # Whisker
//!
//! Cross-platform native UI framework with a retained Rust rendering core.
//!
//! Most users only need [`prelude`]:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     render! {
//!         view(style: css!(flex_grow: 1.0, background_color: Color::hex(0xffffff))) {
//!             text(value: "Hello, Whisker")
//!         }
//!     }
//! }
//! ```
//!
//! The legacy Lynx Host owns its required root `page` element and wraps
//! whatever your app returns. Other Hosts mount the returned root element
//! directly, so `page` is not part of Whisker's public element API.
//!
//! ## What's in this crate
//!
//! The `whisker` crate is an *umbrella* — almost everything here is a
//! re-export from a more specialised companion crate, surfaced through
//! a single import root so app code never needs to know which inner
//! crate owns which symbol. The conceptual groupings:
//!
//! - **Macros** ([`component`], [`main`], [`module_component`], [`render`])
//!   — proc macros that lower component definitions and the `render! { … }`
//!   DSL into builder chains over the items in [`__tags`].
//! - **Reactive primitives** — [`signal()`], [`computed()`], [`effect()`],
//!   [`on_cleanup`], [`on_mount`], [`provide_context`], [`use_context`],
//!   [`resource()`], and their handle types ([`Signal`], [`ReadSignal`],
//!   [`RwSignal`], [`Resource`], …).
//! - **Async** — [`spawn_local`], [`run_blocking`], and the instance-aware
//!   [`runtime_dispatcher()`]. [`run_on_main_thread`] remains available to the
//!   legacy Lynx host during migration.
//! - **Control flow** — [`ForEach`] (keyed list), [`Show`] (conditional).
//!   Both are written as ordinary `#[component]` functions.
//! - **CSS** — the [`css`] type-safe builder + the `css!` macro.
//! - **Built-in elements** — `view`, `text`, `scroll_view`, `list`,
//!   `fragment`. The `render!` macro lowers each
//!   tag invocation into a builder chain on the corresponding struct in
//!   [`__tags`]; the [`__tags::ElementBuilder`] trait provides the
//!   shared `style` / semantics / `on_<event>` methods.
//! - **Platform bridges** — [`PlatformModule`] + [`module!`] for
//!   function-shaped native modules, [`ElementRef`]
//!   for imperative methods on mounted components, [`ElementHandle`]
//!   et al for the typed return values.
//! - **Typed control options** — see [`attrs`] for ScrollView options.
//!
//! Everything intended for direct user code is also pulled into
//! [`prelude`]; reaching into the long paths is only necessary when
//! writing framework-level extension code.

// Lets the `#[whisker::component]` / `render!` expansions inside this
// crate (e.g. `ForEach` / `Show` in `control_flow.rs`) resolve their
// emitted `::whisker::…` paths. Without this the macros only work
// from downstream crates.
extern crate self as whisker;

pub use whisker_config as config;
pub use whisker_runtime as runtime;

pub use whisker_css as css;

// Continuous, signal-based animation engine (Flutter-style
// AnimationController + Tween). See `docs/animation-design.md`.
pub use whisker_animation as animation;
pub use whisker_animation::{AnimConfig, Animatable, AnimationController, Curve, Tween, animated};

pub use whisker_engine::whisker_protocol::{
    Accessibility, AccessibilityChecked, AccessibilityRole, AccessibilityState, ChildPolicy,
    CommandId, ElementCommandSchema, ElementEventSchema, ElementMeasurement, ElementPropertySchema,
    ElementSchema, ElementValueKind, EventId, PropertyId,
};
pub use whisker_runtime::element::ElementTag;

/// The return type of a `#[component]` / `#[whisker::main]` function —
/// an opaque handle to a mounted view subtree. Re-exported at the
/// crate root (and in the [`prelude`]) so component signatures read
/// `-> Element` without an internal `runtime::view` import.
pub use whisker_runtime::view::Element;

#[doc(hidden)]
pub use whisker_macros::builtin_component;
pub use whisker_macros::{WhiskerModule, component, main, module_component, render};

/// A platform implementation contributed by a Whisker module package.
///
/// `Definition` is intentionally associated: Desktop, Web, Android, and iOS
/// bind the same shared schema to different native implementation types.
pub trait WhiskerModule {
    /// Platform-specific declaration consumed by the generated application.
    type Definition;

    /// Builds this platform's declaration.
    fn definition() -> Self::Definition;
}

pub mod back;
mod element_ref;
pub mod focus;

pub use element_ref::{ElementHandle, ElementRef, RefError, ScrollViewHandle, TextHandle};

#[doc(hidden)]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub use whisker_driver::ffi_module as __driver_module;
pub use whisker_runtime::module::PlatformModule;

pub use attrs::ScrollSnap;
pub use whisker_runtime::event::Dataset;
/// The universal tagged-union value model. Crosses the native
/// boundary as both module args/returns and event payloads, so it
/// lives at the crate root rather than buried under
/// `platform_module` (where it's also re-exported for back-compat).
pub use whisker_value::WhiskerValue;

/// Typed event objects handed to `on_<event>` handlers on built-in
/// elements and `#[whisker::module_component]` view methods.
///
/// A `view(on_tap: |e| …)` handler receives a [`TouchEvent`](event::TouchEvent);
/// `on_animationend` an [`AnimationEvent`](event::AnimationEvent);
/// component-specific state events a [`CustomEvent`](event::CustomEvent).
pub mod event {
    pub use whisker_runtime::event::{
        AnimationEvent, BindType, CustomEvent, Event, LayoutCompleteDetail, LayoutCompleteEvent,
        Point, ScrollDetail, ScrollEvent, ScrollStateChangeDetail, ScrollStateChangeEvent,
        SelectionChangeEvent, SelectionDetail, Size, SnapDetail, SnapEvent, Target,
        TextLayoutDetail, TextLayoutEvent, TextLineInfo, Touch, TouchEvent,
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

pub use whisker_runtime::reactive::{
    ArcReadSignal, ArcRwSignal, ArcWriteSignal, Callback, ReadSignal, Resource, ResourceState,
    RwSignal, Signal, StoredValue, WriteSignal, arc_signal, computed, effect, flush, on_cleanup,
    on_mount, provide_context, resource, resource_sync, signal, use_context, with_context,
};
// Component mount/unmount + mount-queue machinery. Driven by the
// `#[component]` expansion and the hot-reload remount path, not by app
// code.
#[doc(hidden)]
pub use whisker_runtime::reactive::{flush_mounts, mount_component, unmount_component};
// Owner / scope API. Application code rarely touches these —
// `#[component]` + `on_cleanup` cover the common case. Framework
// extension code (custom control-flow, custom routers, advanced
// tests) reaches into this module to create and dispose reactive
// scopes manually.
pub use whisker_runtime::reactive::Owner;
pub use whisker_runtime::reactive::owner;
pub use whisker_runtime::tasks::{run_blocking, spawn_local};
pub use whisker_runtime::{RuntimeDispatcher, runtime_dispatcher};
// Frame-driving internal used by the host tick loop, not app code.
#[doc(hidden)]
pub use whisker_runtime::tasks::run_until_stalled;
mod control_flow;
mod style;

pub mod attrs;

pub use style::{Style, apply_style};
pub use whisker_runtime::{
    ElementAuthoringBinding, ElementModuleDefinition, ElementProviderMetadata, ElementRegistry,
    ElementRegistryBuilder, ElementRegistryError, InputDispatch, ResourceEventApply,
    RuntimeBindingError, RuntimeDrive, RuntimeDriveError, RuntimeEventError, RuntimeFrame,
    RuntimeFrameError, RuntimeInputError, RuntimeInstance, RuntimeLayoutError, RuntimeLifecycle,
    RuntimeLifecycleError, RuntimePresentError, RuntimeResourceError, SCROLL_BY_COMMAND,
    SCROLL_ENABLED_PROPERTY, SCROLL_TO_COMMAND, SCROLL_VIEW_ELEMENT_NAME, SurfaceRuntime,
    TEXT_ELEMENT_NAME, VIEW_ELEMENT_NAME, scroll_view_element_binding,
    standard_element_registrations, text_element_binding, view_element_binding,
};

pub use control_flow::{ForEach, ForEachProps, Show, ShowProps};
pub use whisker_runtime::view::{Children, TextChildren};
pub use whisker_runtime::view::{
    EachFn, Fallback, ItemFn, KeyFn, ListHandle, ListHandleError, ListRef, ListScrollTarget,
    ListSnapshot, ScrollAlignment, ScrollAxis, ScrollBehavior, WhenFn,
};

/// Built-in tag builders. The `render!` macro lowers each built-in
/// element invocation (`view(style: css!(flex_grow: 1.0), on_tap: move |_| {})`) into a
/// builder method chain on one of these types
/// (`__tags::view().style(|| "x").on_tap(|| {}).__h()`). Methods
/// internally invoke the imperative runtime primitives
/// (`create_element`, `set_specified_style`, …).
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
pub mod __element_builder {
    pub use crate::__tags::ElementBuilder;
}

#[doc(hidden)]
pub mod __tags {
    use crate::ElementTag;
    use whisker_runtime::event::{AnimationEvent, ScrollEvent, TouchEvent, bind_typed};
    use whisker_runtime::reactive::{Signal, effect};
    use whisker_runtime::view::{
        BindType, Element, append_child, apply_accessibility, apply_attr, apply_attr_bool,
        apply_dataset, apply_element_id, apply_text_max_lines, create_element,
        create_phantom_element, set_attribute_object,
    };

    // A trait, not `macro_rules!`: RA's method-completion does NOT
    // surface methods produced by a `macro_rules!` expansion inside an
    // `impl` block, whereas trait methods are first-class items it
    // indexes — provided the trait is in scope. `render!` imports it through
    // `__tags` for built-ins and through the common `__element_builder`
    // re-export for module components. End-to-end guard:
    // `crates/whisker-macros/tests/ra_completion.rs`.

    /// Shared builder methods for built-in and module element tags.
    ///
    /// Each method consumes `self` and returns it, so calls chain:
    /// `view().style(…).on_tap(…).child(…)`. Reactive-capable
    /// attributes accept any `Into<Signal<T>>` (a static value, a
    /// `ReadSignal`, an `RwSignal`, …) and re-apply on change.
    pub trait ElementBuilder: Sized {
        /// The underlying runtime element handle. Implemented by each
        /// tag struct as `self.handle`.
        #[doc(hidden)]
        fn __element(&self) -> Element;

        // ---- Styling ----------------------------------------------------

        /// Structured CSS declarations.
        ///
        /// Accepts any value that converts into [`crate::Style`] — a
        /// [`whisker_css::Css`] builder, or a reactive
        /// [`ReadSignal`](crate::ReadSignal) / [`RwSignal`](crate::RwSignal)
        /// carrying `Css`. Reactive variants re-apply the declarations via the
        /// element's internal `effect` whenever the underlying
        /// signal changes.
        ///
        /// ```ignore
        /// view(style: css!(padding: px(8), background_color: Color::hex(0xff0000)))
        /// view(style: Css::new().padding(px(8)).background_color(Color::hex(0xff0000)))
        /// view(style: computed(move || Css::new().opacity(alpha.get())))
        /// ```
        fn style<V>(self, v: V) -> Self
        where
            V: Into<crate::Style>,
        {
            crate::apply_style(self.__element(), v);
            self
        }

        // ---- Common semantics (shared by all elements) ------------------

        /// Stable identifier surfaced through event target metadata.
        fn id<V>(self, v: V) -> Self
        where
            V: Into<Signal<String>>,
        {
            apply_element_id(self.__element(), v);
            self
        }

        /// Structured metadata surfaced through event targets.
        fn dataset<V>(self, v: V) -> Self
        where
            V: Into<Signal<crate::Dataset>>,
        {
            apply_dataset(self.__element(), v);
            self
        }

        /// Common accessibility semantics for built-in and module elements.
        fn accessibility<V>(self, v: V) -> Self
        where
            V: Into<Signal<crate::Accessibility>>,
        {
            apply_accessibility(self.__element(), v);
            self
        }

        // Capture listeners fire root-to-target and bubble listeners fire
        // target-to-root. `catch` stops propagation at that listener. The
        // retained Rust scene performs hit testing and event propagation, so
        // these semantics are identical on every Host.

        /// `tap` — single tap (won't fire if the finger moved far).
        /// Bubble phase, lets the event continue up the chain.
        ///
        /// The closure receives a [`TouchEvent`] with the tap
        /// coordinates and target metadata. For "stop propagation"
        /// semantics use [`on_tap_catch`](Self::on_tap_catch);
        /// for the down-pass capture phase, the `on_capture_tap*`
        /// variants.
        ///
        /// ```ignore
        /// view(on_tap: move |e| println!("tap at {:?}", e.detail))
        /// ```
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

        // ---- Children ---------------------------------------------------

        /// Append a child handle.
        fn child(self, child: Element) -> Self {
            append_child(self.__element(), child);
            self
        }

        // ---- Ref --------------------------------------------------------

        /// Bind an [`ElementRef`](crate::ElementRef) to this element so
        /// its commands can be invoked after mount. `render!` routes the `ref:`
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

    /// `<view>` — Whisker's basic layout primitive:
    /// a rectangular box that lays out children with CSS flexbox
    /// (`<View>` in React Native, `<div>` on the web).
    ///
    /// Use `view` for any non-text grouping or layout. `view` is also the
    /// right tag for touch targets; attach `on_tap` or `on_click` there.
    ///
    /// ```ignore
    /// render! {
    ///     view(
    ///         style: css!(flex_direction: FlexDirection::Column, padding: px(16)),
    ///         on_tap: move |_| println!("tapped"),
    ///     ) {
    ///         text(value: "Title")
    ///         text(value: "Subtitle")
    ///     }
    /// }
    /// ```
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

    /// `<text>` — plain-text leaf. The raw-text node used to lower `value`
    /// is an internal runtime detail.
    ///
    /// `text` is the only element that renders text on screen. Set
    /// the content through the [`value`](Self::value) attribute
    /// (which takes any `Into<Signal<String>>`, so static strings,
    /// `ReadSignal<String>`, and computed signals all work). Font /
    /// color / size live in the `style` attribute as ordinary CSS.
    ///
    /// ```ignore
    /// let count = signal(0_i32);
    /// render! {
    ///     text {
    ///         style: css!(font_size: px(18), color: Color::hex(0x000000)),
    ///         value: computed(move || format!("count: {}", count.get())),
    ///     }
    /// }
    /// ```
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
        /// `value` — the text string (reactive-capable).
        pub fn value<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            let raw = create_element(ElementTag::RawText);
            append_child(self.handle, raw);
            apply_attr(raw, "text", v);
            self
        }

        /// Maximum displayed line count. `0` restores the unlimited default.
        pub fn max_lines<V>(self, value: V) -> Self
        where
            V: ::std::convert::Into<Signal<u32>>,
        {
            apply_text_max_lines(self.handle, value);
            self
        }
    }

    /// `<scroll-view>` — scrollable container.
    ///
    /// Use `scroll_view` for content the user should be able to pan
    /// past the viewport. For long, *virtualised* lists where only
    /// the visible items should hold platform views, reach for
    /// [`list`] instead — `scroll_view` keeps every child mounted.
    /// Direction defaults to `Vertical`; flip with
    /// [`axis`](Self::axis).
    ///
    /// ```ignore
    /// render! {
    ///     scroll_view {
    ///         style: css!(flex_grow: 1.0),
    ///         axis: ScrollAxis::Vertical,
    ///         on_scroll: |e| println!("y = {}", e.detail.scroll_top),
    ///         view { /* ... long content ... */ }
    ///     }
    /// }
    /// ```
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
        /// Logical scroll axis (vertical by default).
        pub fn axis<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<crate::ScrollAxis>>,
        {
            apply_attr(self.handle, "scroll-orientation", v);
            self
        }

        /// Enables item-aligned settling for carousels and pagers.
        pub fn snap<V>(self, snap: V) -> Self
        where
            V: ::std::convert::Into<Signal<crate::ScrollSnap>>,
        {
            let apply = |snap: crate::ScrollSnap| {
                set_attribute_object(
                    self.handle,
                    "item-snap",
                    &[
                        ("factor".to_string(), snap.factor()),
                        ("offset".to_string(), snap.offset()),
                    ],
                );
            };
            match snap.into() {
                Signal::Stored(value) => value.with(|value| apply(*value)),
                Signal::Dynamic(value) => {
                    let handle = self.handle;
                    effect(move || {
                        let snap = value.get();
                        set_attribute_object(
                            handle,
                            "item-snap",
                            &[
                                ("factor".to_string(), snap.factor()),
                                ("offset".to_string(), snap.offset()),
                            ],
                        );
                    });
                }
            }
            self
        }

        /// Controls whether one scroll gesture may pass intermediate snap
        /// points. [`ScrollSnapStop::Always`](crate::attrs::ScrollSnapStop::Always)
        /// is useful for pagers and manga readers that advance one page at a time.
        pub fn scroll_snap_stop<V>(self, value: V) -> Self
        where
            V: ::std::convert::Into<Signal<crate::attrs::ScrollSnapStop>>,
        {
            apply_attr(self.handle, "scroll-snap-stop", value);
            self
        }

        /// Enables or disables user-driven scrolling.
        pub fn scroll_enabled<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr_bool(self.handle, "enable-scroll", v);
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
    }

    // ---- list (Rust-owned virtualized control primitive) ----------------

    /// `list` keeps a bounded window of ordinary item subtrees mounted below
    /// the standard `ScrollView`. It is control flow like [`ForEach`], not a
    /// Host element: FramePacket contains the ScrollView, spacer Views, and
    /// visible item nodes only.
    ///
    /// Use `list` when the data set is large enough that
    /// `scroll_view` + a [`ForEach`](crate::ForEach) inside would
    /// hold too many off-screen platform views. For short,
    /// fully-mounted content prefer the simpler combo.
    ///
    /// ```ignore
    /// let items = signal(vec!["alpha".to_string(), "beta".to_string()]);
    /// render! {
    ///     list(
    ///         each: move || items.get(),
    ///         key: |s: &String| s.clone(),
    ///         children: |s: ReadSignal<String>| render! { view { text(value: s) } },
    ///     )
    /// }
    /// ```
    ///
    /// # Trade-offs
    ///
    /// The builder takes its items source as three kwargs (`each`,
    /// `key`, `children`) and **does not accept a body** — the macro
    /// rejects `list { … }` invocations because items can only come
    /// through the reactive props. The three setters are
    /// **type-stated**: `__h()` is only callable when all three have
    /// been supplied, so a missing prop is a compile-time error at
    /// the close of the builder chain rather than a runtime panic.
    ///
    /// `__h()` installs one reactive keyed reconciler and one ordinary
    /// ScrollView `scroll` listener. The Host reports geometry with the same
    /// node event path used by custom elements; no list-specific bridge call
    /// exists.
    struct ListOptions {
        content: Element,
        content_style: Option<crate::Style>,
        axis: ::whisker_runtime::view::ScrollAxis,
        start_reached_threshold: f32,
        end_reached_threshold: f32,
        on_start_reached: Option<::std::rc::Rc<dyn Fn()>>,
        on_end_reached: Option<::std::rc::Rc<dyn Fn()>>,
        header: Option<::std::rc::Rc<dyn Fn() -> Element>>,
        footer: Option<::std::rc::Rc<dyn Fn() -> Element>>,
        empty: Option<::std::rc::Rc<dyn Fn() -> Element>>,
    }

    fn configure_list_presentation(
        scroll_view: Element,
        options: &ListOptions,
    ) -> ::whisker_runtime::view::VirtualListLayout {
        apply_attr(scroll_view, "scroll-orientation", options.axis.to_string());
        crate::style::apply_list_content_style(
            options.content,
            options.axis,
            options.content_style.clone(),
        )
    }

    #[allow(non_camel_case_types)]
    pub struct list<EachF = (), KeyF = (), ChildF = (), RefF = (), InitialF = ()> {
        handle: Element,
        options: ListOptions,
        each: EachF,
        key: KeyF,
        children: ChildF,
        list_ref: RefF,
        initial_scroll: InitialF,
    }
    #[allow(non_snake_case)]
    pub fn __list_ctor() -> list<(), (), ()> {
        // `list` is a Rust control primitive, not a Host element. Its only
        // Host-visible container is the same built-in ScrollView that an app
        // can author directly; the Rust virtualizer mounts ordinary children
        // into a bounded window below it.
        let handle = create_element(ElementTag::ScrollView);
        let content = create_element(ElementTag::View);
        list {
            handle,
            options: ListOptions {
                content,
                content_style: None,
                axis: ::whisker_runtime::view::ScrollAxis::Vertical,
                start_reached_threshold: 0.0,
                end_reached_threshold: 0.0,
                on_start_reached: None,
                on_end_reached: None,
                header: None,
                footer: None,
                empty: None,
            },
            each: (),
            key: (),
            children: (),
            list_ref: (),
            initial_scroll: (),
        }
    }
    impl<EachF, KeyF, ChildF, RefF, InitialF> ElementBuilder
        for list<EachF, KeyF, ChildF, RefF, InitialF>
    {
        fn __element(&self) -> Element {
            self.handle
        }
        // `list` takes its items through the `each`/`key`/`children`
        // render props, never body children.
    }
    impl<EachF, KeyF, ChildF, RefF, InitialF> list<EachF, KeyF, ChildF, RefF, InitialF> {
        /// Styles the internal content View while `style:` styles the outer
        /// ScrollView viewport. A static typed style may select the constrained
        /// virtualized Grid subset documented in `docs/list-design.md`.
        pub fn content_style<V>(mut self, value: V) -> Self
        where
            V: ::std::convert::Into<crate::Style>,
        {
            self.options.content_style = Some(value.into());
            self
        }

        /// Selects the virtualized main axis. The default is vertical.
        pub fn axis(mut self, axis: ::whisker_runtime::view::ScrollAxis) -> Self {
            self.options.axis = axis;
            self
        }

        /// Enables or disables user-driven scrolling without disabling
        /// imperative ListHandle operations.
        pub fn scroll_enabled<V>(self, value: V) -> Self
        where
            V: ::std::convert::Into<Signal<bool>>,
        {
            apply_attr_bool(self.handle, "enable-scroll", value);
            self
        }

        /// Logical-pixel distance from the start edge at which
        /// `on_start_reached` becomes active.
        pub fn start_reached_threshold(mut self, value: f32) -> Self {
            self.options.start_reached_threshold = value.max(0.0);
            self
        }

        /// Logical-pixel distance from the end edge at which
        /// `on_end_reached` becomes active.
        pub fn end_reached_threshold(mut self, value: f32) -> Self {
            self.options.end_reached_threshold = value.max(0.0);
            self
        }

        /// Fires once when scrolling enters the configured start threshold.
        pub fn on_start_reached<F: Fn() + 'static>(mut self, callback: F) -> Self {
            self.options.on_start_reached = Some(::std::rc::Rc::new(callback));
            self
        }

        /// Fires once when scrolling enters the configured end threshold.
        pub fn on_end_reached<F: Fn() + 'static>(mut self, callback: F) -> Self {
            self.options.on_end_reached = Some(::std::rc::Rc::new(callback));
            self
        }

        /// Builds persistent content before the virtualized item range.
        pub fn header<F: Fn() -> Element + 'static>(mut self, content: F) -> Self {
            self.options.header = Some(::std::rc::Rc::new(content));
            self
        }

        /// Builds persistent content after the virtualized item range.
        pub fn footer<F: Fn() -> Element + 'static>(mut self, content: F) -> Self {
            self.options.footer = Some(::std::rc::Rc::new(content));
            self
        }

        /// Builds the content shown while the item source is empty.
        pub fn empty<F: Fn() -> Element + 'static>(mut self, content: F) -> Self {
            self.options.empty = Some(::std::rc::Rc::new(content));
            self
        }

        /// Fired continuously while scrolling. Geometry is normalized by the
        /// standard ScrollView event contract on every Host.
        pub fn on_scroll<F: Fn(ScrollEvent) + 'static>(self, f: F) -> Self {
            bind_typed(self.handle, "scroll", BindType::Bind, f);
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
    impl<EachF, KeyF, ChildF, InitialF> list<EachF, KeyF, ChildF, (), InitialF> {
        /// Binds the typed Rust List controller. Unlike ordinary element refs,
        /// this also exposes key/index resolution and cached snapshots.
        pub fn bind_ref<K: 'static>(
            self,
            list_ref: ::whisker_runtime::view::ListRef<K>,
        ) -> list<EachF, KeyF, ChildF, ::whisker_runtime::view::ListRef<K>, InitialF> {
            list {
                handle: self.handle,
                options: self.options,
                each: self.each,
                key: self.key,
                children: self.children,
                list_ref,
                initial_scroll: self.initial_scroll,
            }
        }
    }

    impl<KeyF, ChildF, RefF, InitialF> list<(), KeyF, ChildF, RefF, InitialF> {
        pub fn each<T: 'static, F>(
            self,
            f: F,
        ) -> list<::whisker_runtime::view::EachFn<T>, KeyF, ChildF, RefF, InitialF>
        where
            F: ::std::convert::Into<::whisker_runtime::view::EachFn<T>>,
        {
            list {
                handle: self.handle,
                options: self.options,
                each: f.into(),
                key: self.key,
                children: self.children,
                list_ref: self.list_ref,
                initial_scroll: self.initial_scroll,
            }
        }
    }
    impl<EachF, ChildF, RefF, InitialF> list<EachF, (), ChildF, RefF, InitialF> {
        /// Stable logical identity extractor, matching [`ForEach`](crate::ForEach).
        pub fn key<T: 'static, K: 'static, F>(
            self,
            f: F,
        ) -> list<EachF, ::whisker_runtime::view::KeyFn<T, K>, ChildF, RefF, InitialF>
        where
            F: ::std::convert::Into<::whisker_runtime::view::KeyFn<T, K>>,
        {
            list {
                handle: self.handle,
                options: self.options,
                each: self.each,
                key: f.into(),
                children: self.children,
                list_ref: self.list_ref,
                initial_scroll: self.initial_scroll,
            }
        }
    }
    impl<EachF, KeyF, RefF, InitialF> list<EachF, KeyF, (), RefF, InitialF> {
        /// Builds one keyed row. The signal is updated when data for the same
        /// key changes; leaving the mounted window disposes its owner.
        pub fn children<T: 'static, F>(
            self,
            f: F,
        ) -> list<
            EachF,
            KeyF,
            ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
            RefF,
            InitialF,
        >
        where
            F: ::std::convert::Into<
                    ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
                >,
        {
            list {
                handle: self.handle,
                options: self.options,
                each: self.each,
                key: self.key,
                children: f.into(),
                list_ref: self.list_ref,
                initial_scroll: self.initial_scroll,
            }
        }
    }

    impl<EachF, KeyF, ChildF, RefF> list<EachF, KeyF, ChildF, RefF, ()> {
        /// Applies one logical target after the initial source snapshot is
        /// indexed. Key targets are checked against the List's key type at
        /// compile time.
        pub fn initial_scroll<K: 'static>(
            self,
            target: ::whisker_runtime::view::ListScrollTarget<K>,
        ) -> list<EachF, KeyF, ChildF, RefF, ::whisker_runtime::view::ListScrollTarget<K>> {
            list {
                handle: self.handle,
                options: self.options,
                each: self.each,
                key: self.key,
                children: self.children,
                list_ref: self.list_ref,
                initial_scroll: target,
            }
        }
    }

    #[doc(hidden)]
    pub trait ListInitialScroll<K> {
        fn into_target(self) -> Option<::whisker_runtime::view::ListScrollTarget<K>>;
    }

    impl<K> ListInitialScroll<K> for () {
        fn into_target(self) -> Option<::whisker_runtime::view::ListScrollTarget<K>> {
            None
        }
    }

    impl<K> ListInitialScroll<K> for ::whisker_runtime::view::ListScrollTarget<K> {
        fn into_target(self) -> Option<::whisker_runtime::view::ListScrollTarget<K>> {
            Some(self)
        }
    }
    // ---- Finaliser, only on fully-populated state ----
    impl<T, K, InitialF>
        list<
            ::whisker_runtime::view::EachFn<T>,
            ::whisker_runtime::view::KeyFn<T, K>,
            ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
            (),
            InitialF,
        >
    where
        T: ::std::clone::Clone + 'static,
        K: ::std::cmp::Eq + ::std::hash::Hash + ::std::clone::Clone + 'static,
        InitialF: ListInitialScroll<K>,
    {
        /// Finalises the Rust-owned keyed windowing core.
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            let virtual_layout = configure_list_presentation(self.handle, &self.options);
            let handle = self.handle;
            let content = self.options.content;
            let axis = self.options.axis;
            let each = self.each;
            let key = self.key;
            let children = self.children;
            let header = self.options.header.map(|content| content());
            let footer = self.options.footer.map(|content| content());
            let empty = self.options.empty.map(|content| content());

            ::whisker_runtime::view::virtualize(
                handle,
                content,
                move || each.call(),
                move |t: &T| key.call(t),
                move |item| children.call(item),
                ::whisker_runtime::view::VirtualListOptions {
                    axis,
                    layout: virtual_layout,
                    list_ref: None,
                    initial_scroll: self.initial_scroll.into_target(),
                    start_reached_threshold: self.options.start_reached_threshold,
                    end_reached_threshold: self.options.end_reached_threshold,
                    on_start_reached: self.options.on_start_reached,
                    on_end_reached: self.options.on_end_reached,
                    header,
                    footer,
                    empty,
                },
            );

            handle
        }
    }

    impl<T, K, InitialF>
        list<
            ::whisker_runtime::view::EachFn<T>,
            ::whisker_runtime::view::KeyFn<T, K>,
            ::whisker_runtime::view::ItemFn<::whisker_runtime::reactive::ReadSignal<T>>,
            ::whisker_runtime::view::ListRef<K>,
            InitialF,
        >
    where
        T: ::std::clone::Clone + 'static,
        K: ::std::cmp::Eq + ::std::hash::Hash + ::std::clone::Clone + 'static,
        InitialF: ListInitialScroll<K>,
    {
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            let virtual_layout = configure_list_presentation(self.handle, &self.options);
            let handle = self.handle;
            let content = self.options.content;
            let axis = self.options.axis;
            let each = self.each;
            let key = self.key;
            let children = self.children;
            let header = self.options.header.map(|content| content());
            let footer = self.options.footer.map(|content| content());
            let empty = self.options.empty.map(|content| content());

            ::whisker_runtime::view::virtualize(
                handle,
                content,
                move || each.call(),
                move |t: &T| key.call(t),
                move |item| children.call(item),
                ::whisker_runtime::view::VirtualListOptions {
                    axis,
                    layout: virtual_layout,
                    list_ref: Some(self.list_ref),
                    initial_scroll: self.initial_scroll.into_target(),
                    start_reached_threshold: self.options.start_reached_threshold,
                    end_reached_threshold: self.options.end_reached_threshold,
                    on_start_reached: self.options.on_start_reached,
                    on_end_reached: self.options.on_end_reached,
                    header,
                    footer,
                    empty,
                },
            );

            handle
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

/// Marshal a closure onto the active runtime thread.
///
/// Host integrations should capture
/// [`runtime_dispatcher()`] while a [`RuntimeInstance`] is executing and post
/// through that instance-aware handle. This process-global helper remains for
/// compatibility with code that has not yet captured a dispatcher.
///
/// ```ignore
/// let dispatcher = runtime_dispatcher().unwrap();
/// std::thread::spawn(move || {
///     let result = blocking_fetch();
///     dispatcher.post(move || data.set(Some(result)));
/// });
/// ```
pub use whisker_runtime::main_thread::run_on_main_thread;

/// Whisker platform module invocation entry point.
///
/// API surface for calling Host modules through the active Host binding:
/// the [`WhiskerValue`] tagged-union type plus the
/// [`invoke`](platform_module::invoke) /
/// [`invoke_async`](platform_module::invoke_async) entry points. The
/// platform side may be Swift/Kotlin over FFI or a direct Rust implementation.
///
/// The `#[whisker::platform_module]` proc macro generates type-safe
/// Rust proxies that wrap `invoke` / `invoke_async`; reach into this
/// module directly only when you need the raw [`WhiskerValue`] enum.
pub mod platform_module {
    pub use whisker_runtime::module::{invoke, invoke_async};
    pub use whisker_runtime::value::{WhiskerModuleError, WhiskerValue};
}

/// Internal runtime entry points used by code the `#[whisker::main]` macro
/// expands to. Not stable, not for direct use.
#[doc(hidden)]
pub mod __main_runtime {
    /// Wrap one invocation of the user's `app` function for hot-patch
    /// dispatch. The `#[whisker::main]` macro calls this unconditionally
    /// from inside the user crate so we don't need a user-crate-local
    /// `hot-reload` feature flag to gate the call site.
    ///
    /// The cfg flip happens here, at whisker's compile-time, on whisker's
    /// own `hot-reload` feature:
    ///
    /// - **on** (`whisker run` / hot reload): body is
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
        init_tokio_runtime();
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
        init_tokio_runtime();
        f()
    }

    #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
    fn init_tokio_runtime() {
        use std::sync::Once;
        static START: Once = Once::new();
        START.call_once(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("whisker-tokio")
                .build()
                .expect("whisker: build tokio runtime");
            let runtime: &'static tokio::runtime::Runtime = Box::leak(Box::new(runtime));
            std::mem::forget(runtime.enter());
        });
    }

    #[cfg(not(all(feature = "tokio", not(target_arch = "wasm32"))))]
    fn init_tokio_runtime() {}

    /// Same dispatch shape as [`call_user_app`], for the
    /// `__whisker_app_body_hash` fn the `#[whisker::main]` macro
    /// emits. Routing the hash read through `subsecond::call` means
    /// that after a patch is applied, this returns the hash baked
    /// into the *patch dylib* — a changed value is the full-remount
    /// signal that `app()` itself was edited and needs a full
    /// re-run. The `move` and `#[inline(always)]` are load-bearing
    /// for the same reasons documented on `call_user_app`.
    #[cfg(feature = "hot-reload")]
    #[inline(always)]
    pub fn call_app_hash(f: fn() -> u64) -> u64 {
        #[allow(clippy::redundant_closure)]
        {
            ::subsecond::call(move || f())
        }
    }

    #[cfg(not(feature = "hot-reload"))]
    #[inline(always)]
    pub fn call_app_hash(f: fn() -> u64) -> u64 {
        f()
    }
}

/// Internal FFI runtime entry points generated by [`main`](crate::main).
#[doc(hidden)]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub mod __driver_runtime {
    pub use whisker_driver::ffi_runtime::{
        create, destroy, dispatch_event, dispatch_module_event, dispatch_pointer,
        dispatch_resource_event, tick,
    };
}

/// Stable C-ABI types referenced by code emitted from [`main`](crate::main).
#[doc(hidden)]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub use whisker_driver::abi as __driver_abi;

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

    /// Dispatch a `#[component]`-generated props-layout-hash fn
    /// through subsecond, so after a patch the caller reads the
    /// *patch dylib's* hash. Same 8-byte fn-pointer-capture shape as
    /// `__main_runtime::call_user_app` — the `move` and
    /// `#[inline(always)]` are load-bearing for the same reasons
    /// documented there.
    #[cfg(feature = "hot-reload")]
    #[inline(always)]
    pub fn call_hash(f: fn() -> u64) -> u64 {
        #[allow(clippy::redundant_closure)]
        {
            ::subsecond::call(move || f())
        }
    }

    #[cfg(not(feature = "hot-reload"))]
    #[inline(always)]
    pub fn call_hash(f: fn() -> u64) -> u64 {
        f()
    }
}

/// Common imports for Whisker app code.
///
/// `use whisker::prelude::*;` brings everything an everyday
/// component / app needs into scope in one line. The contents map
/// conceptually to:
///
/// - **Macros** for definition + templating — [`component`],
///   [`main`], [`render`].
/// - **Reactive primitives** — [`signal()`], [`computed()`],
///   [`effect()`], [`on_cleanup`], [`on_mount`], context APIs,
///   [`resource()`] / [`resource_sync`], plus all the handle types
///   ([`Signal`], [`ReadSignal`], [`RwSignal`], …).
/// - **Async** — [`spawn_local`], [`run_blocking`], and
///   [`runtime_dispatcher()`].
/// - **Control flow** — [`ForEach`], [`Show`], plus the
///   function-shaped prop types ([`EachFn`], [`KeyFn`], [`ItemFn`],
///   [`WhenFn`], [`Fallback`]).
/// - **Refs** — [`ElementRef`] (construct with `ElementRef::new()`),
///   and the typed
///   handles ([`ElementHandle`], [`ScrollViewHandle`], [`TextHandle`],
///   [`RefError`]).
/// - **CSS** — [`Css`](crate::css::Css), the builder API,
///   numeric extension traits (`8.px()`, `45.deg()`, …), and the
///   `css!` macro.
/// - **Built-in element tags** — `view`, `text`, `scroll_view`,
///   `list`, `fragment` (re-exported from the
///   hidden [`__tags`] module so rust-analyzer
///   completes `vie|` → `view` inside `render!`).
/// - **Typed control options** — [`ScrollAxis`](crate::ScrollAxis),
///   [`ScrollSnap`](crate::ScrollSnap), and
///   [`ScrollSnapStop`](crate::attrs::ScrollSnapStop).
///
/// Framework-level code (custom control flow, custom routers, tests
/// that bootstrap reactive scopes) reaches past the prelude into
/// [`crate::runtime`] / [`crate::owner`] / [`crate::platform_module`].
pub mod prelude {
    pub use crate::Children;
    pub use crate::css::ext::*;
    pub use crate::css::{
        AlignItems, Border, Color, Css, CustomPropertyName, Display, Flex, FlexDirection, FlexWrap,
        JustifyContent, Length, NamedColor, Size, StyleProperty, ToCss,
    };
    pub use crate::{
        ArcReadSignal, ArcRwSignal, ArcWriteSignal, Callback, ReadSignal, Resource, ResourceState,
        RwSignal, Signal, StoredValue, WriteSignal, arc_signal, computed, effect, on_cleanup,
        on_mount, provide_context, resource, resource_sync, run_blocking, run_on_main_thread,
        runtime_dispatcher, signal, spawn_local, use_context, with_context,
    };
    pub use crate::{EachFn, Fallback, ItemFn, KeyFn, WhenFn};
    pub use crate::{Element, ElementTag};
    pub use crate::{
        ElementHandle, ElementRef, ListHandle, ListHandleError, ListRef, ListScrollTarget,
        ListSnapshot, RefError, ScrollAlignment, ScrollAxis, ScrollBehavior, ScrollViewHandle,
        TextHandle,
    };
    pub use crate::{ForEach, ForEachProps, Show, ShowProps};
    pub use crate::{component, main, render};
    // The `css!` macro coexists with the `crate::css` module
    // re-export above because the macro and module namespaces are
    // disjoint.
    pub use crate::attrs::{ScrollSnap, ScrollSnapStop};
    pub use crate::css::css;
    pub use crate::{
        Accessibility, AccessibilityChecked, AccessibilityRole, AccessibilityState, Dataset,
    };
    // Re-exporting the `__tags` struct names is what lets RA complete
    // `vie|` → `view`, `te|` → `text`, etc. inside render! — the
    // macro source position is a value-expression context, so RA does
    // identifier completion against the surrounding scope. Mixing
    // these with kwarg completion (`view(sty|)`) is safe because the
    // macro unconditionally emits `.name(())` for every partial
    // kwarg, so RA's macro-expansion completion path sees the
    // method-call shape regardless of what `view` resolves to.
    #[doc(hidden)]
    pub use crate::__tags::{fragment, list, scroll_view, text, view};
    // A separate list-item builder is intentionally absent — the `list` render-props
    // builder auto-wraps every item internally.
}
