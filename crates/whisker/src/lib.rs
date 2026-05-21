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

pub use whisker_macros::{component, main, render};

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
    use whisker_runtime::reactive::{effect, Signal};
    use whisker_runtime::view::{
        append_child, create_element, set_attribute, set_event_listener, set_inline_styles, Element,
    };

    // Each built-in tag is a struct + a hand-written inherent
    // `impl` block listing every method explicitly. **No
    // `macro_rules!` is used to emit methods.** Earlier
    // experiments (kept as integration tests in
    // `crates/whisker-macros/tests/ra_completion.rs`) showed
    // that rust-analyzer's method-completion engine doesn't
    // surface methods that came from a `macro_rules!` expansion
    // inside an `impl` block — even though the same methods
    // compile and pass type-check fine. Inline definitions fix
    // the completion path; the duplication across six tags is
    // the cost.
    //
    // **Body dispatch via free functions.** Each `.style(v)` /
    // `.attr(name, v)` / etc. delegates to a free helper that
    // matches on `Signal<T>` and either calls the underlying
    // `set_attribute` / `set_inline_styles` once (Static) or
    // wraps the call in `effect(move || …)` (Dynamic). Helpers
    // are free fns, not associated methods, so the inline impl
    // blocks stay terse without macro-rules expansion getting
    // in the way of RA.

    /// Apply an inline-styles value to `h`, picking a static vs
    /// reactive code path based on the [`Signal<T>`] variant. The
    /// `Dynamic` case wraps the read in an `effect` so the
    /// returned [`ReadSignal<T>::get`] call registers the source
    /// as a dependency.
    fn apply_styles<V, T>(h: Element, v: V)
    where
        V: ::std::convert::Into<Signal<T>>,
        T: ::std::string::ToString + ::std::clone::Clone + 'static,
    {
        match v.into() {
            Signal::Static(t) => set_inline_styles(h, &t.to_string()),
            Signal::Dynamic(sig) => {
                effect(move || set_inline_styles(h, &sig.get().to_string()));
            }
        }
    }

    /// Apply a named attribute value to `h`. Same Static / Dynamic
    /// dispatch as [`apply_styles`].
    fn apply_attr<V, T>(h: Element, name: &'static str, v: V)
    where
        V: ::std::convert::Into<Signal<T>>,
        T: ::std::string::ToString + ::std::clone::Clone + 'static,
    {
        match v.into() {
            Signal::Static(t) => set_attribute(h, name, &t.to_string()),
            Signal::Dynamic(sig) => {
                effect(move || set_attribute(h, name, &sig.get().to_string()));
            }
        }
    }

    /// `<page>` — top-level container Lynx mounts as the root of
    /// an app. Holds the screen-level `style=` (background, flex
    /// direction) and a single content subtree.
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
    impl page {
        /// Inline CSS — value-via-closure so signal-reading
        /// expressions re-apply on each dep change.
        /// Inline CSS. Accepts a static value (`String`),
        /// a [`ReadSignal<String>`] / [`RwSignal<String>`] for
        /// reactive updates, or any other `Into<Signal<String>>`
        /// source.
        ///
        /// `T` is fixed to `String` (rather than a generic
        /// `T: ToString`) to keep the `Into<Signal<T>>` inference
        /// path unambiguous: with a generic T, `ReadSignal<String>`
        /// could match both `From<T>` (with T=ReadSignal<String>)
        /// and `From<ReadSignal<T>>` (with T=String). Fixing T at
        /// the call site removes the ambiguity entirely.
        ///
        /// [`ReadSignal<String>`]: ::whisker_runtime::reactive::ReadSignal
        /// [`RwSignal<String>`]: ::whisker_runtime::reactive::RwSignal
        pub fn style<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_styles(self.handle, v);
            self
        }
        /// Tap handler (Lynx `tap` event).
        pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, f: F) -> Self {
            set_event_listener(self.handle, "tap", ::std::boxed::Box::new(f));
            self
        }
        /// Generic event handler.
        pub fn on<F: ::std::ops::Fn() + 'static>(self, event: &'static str, f: F) -> Self {
            set_event_listener(self.handle, event, ::std::boxed::Box::new(f));
            self
        }
        /// Lynx class name.
        /// Lynx `class` attribute. Same Signal<String> contract as
        /// [`style`](Self::style).
        pub fn class<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "class", v);
            self
        }
        /// Catch-all for any other Lynx attribute.
        /// Catch-all for any other Lynx attribute. Same Signal<String>
        /// contract as [`style`](Self::style).
        pub fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, name, v);
            self
        }
        /// Append a child handle.
        pub fn child(self, child: Element) -> Self {
            append_child(self.handle, child);
            self
        }
        /// Finish building and return the underlying handle.
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            self.handle
        }
    }

    /// `<view>` — Lynx's flex container. The most basic layout
    /// primitive in Whisker: a rectangular box that lays out its
    /// children with CSS flexbox. Equivalent to `<View>` in
    /// React Native or `<div>` in the web.
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
    impl view {
        /// Inline CSS — value-via-closure so signal-reading
        /// expressions re-apply on each dep change.
        /// Inline CSS. Accepts a static value (`String`),
        /// a [`ReadSignal<String>`] / [`RwSignal<String>`] for
        /// reactive updates, or any other `Into<Signal<String>>`
        /// source.
        ///
        /// `T` is fixed to `String` (rather than a generic
        /// `T: ToString`) to keep the `Into<Signal<T>>` inference
        /// path unambiguous: with a generic T, `ReadSignal<String>`
        /// could match both `From<T>` (with T=ReadSignal<String>)
        /// and `From<ReadSignal<T>>` (with T=String). Fixing T at
        /// the call site removes the ambiguity entirely.
        ///
        /// [`ReadSignal<String>`]: ::whisker_runtime::reactive::ReadSignal
        /// [`RwSignal<String>`]: ::whisker_runtime::reactive::RwSignal
        pub fn style<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_styles(self.handle, v);
            self
        }
        /// Tap handler (Lynx `tap` event).
        pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, f: F) -> Self {
            set_event_listener(self.handle, "tap", ::std::boxed::Box::new(f));
            self
        }
        /// Generic event handler.
        pub fn on<F: ::std::ops::Fn() + 'static>(self, event: &'static str, f: F) -> Self {
            set_event_listener(self.handle, event, ::std::boxed::Box::new(f));
            self
        }
        /// Lynx class name.
        /// Lynx `class` attribute. Same Signal<String> contract as
        /// [`style`](Self::style).
        pub fn class<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "class", v);
            self
        }
        /// Catch-all for any other Lynx attribute.
        /// Catch-all for any other Lynx attribute. Same Signal<String>
        /// contract as [`style`](Self::style).
        pub fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, name, v);
            self
        }
        /// Append a child handle.
        pub fn child(self, child: Element) -> Self {
            append_child(self.handle, child);
            self
        }
        /// Finish building and return the underlying handle.
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            self.handle
        }
    }

    /// `<text>` — text container. The actual glyphs live in
    /// `raw_text` child elements that the macro creates from
    /// string-literal children.
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
    impl text {
        /// Inline CSS. Accepts a static value (`String`),
        /// a [`ReadSignal<String>`] / [`RwSignal<String>`] for
        /// reactive updates, or any other `Into<Signal<String>>`
        /// source.
        ///
        /// `T` is fixed to `String` (rather than a generic
        /// `T: ToString`) to keep the `Into<Signal<T>>` inference
        /// path unambiguous: with a generic T, `ReadSignal<String>`
        /// could match both `From<T>` (with T=ReadSignal<String>)
        /// and `From<ReadSignal<T>>` (with T=String). Fixing T at
        /// the call site removes the ambiguity entirely.
        ///
        /// [`ReadSignal<String>`]: ::whisker_runtime::reactive::ReadSignal
        /// [`RwSignal<String>`]: ::whisker_runtime::reactive::RwSignal
        pub fn style<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_styles(self.handle, v);
            self
        }
        pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, f: F) -> Self {
            set_event_listener(self.handle, "tap", ::std::boxed::Box::new(f));
            self
        }
        pub fn on<F: ::std::ops::Fn() + 'static>(self, event: &'static str, f: F) -> Self {
            set_event_listener(self.handle, event, ::std::boxed::Box::new(f));
            self
        }
        /// Lynx `class` attribute. Same Signal<String> contract as
        /// [`style`](Self::style).
        pub fn class<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "class", v);
            self
        }
        /// Catch-all for any other Lynx attribute. Same Signal<String>
        /// contract as [`style`](Self::style).
        pub fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, name, v);
            self
        }
        pub fn child(self, child: Element) -> Self {
            append_child(self.handle, child);
            self
        }
        /// Text content. Creates a `raw_text` child element under
        /// the hood and reactively keeps its `text` attribute in
        /// sync with the closure's return value. This is the
        /// kwarg-styled replacement for the old bare-`"hi"`
        /// string-literal child support — see render.rs for why
        /// (rust-analyzer fixup needs every children item to be
        /// kwarg-shape).
        /// Text content. Creates a `raw_text` child element under
        /// the hood and applies its `text` attribute via the
        /// [`Signal<String>`] machinery.
        pub fn value<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            let raw = create_element(ElementTag::RawText);
            append_child(self.handle, raw);
            apply_attr(raw, "text", v);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            self.handle
        }
    }

    /// `<raw_text>` — leaf text node with a `text` attribute.
    /// Normally the macro creates these automatically from
    /// string-literal children of `text` / `view`.
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
    impl raw_text {
        /// Inline CSS. Accepts a static value (`String`),
        /// a [`ReadSignal<String>`] / [`RwSignal<String>`] for
        /// reactive updates, or any other `Into<Signal<String>>`
        /// source.
        ///
        /// `T` is fixed to `String` (rather than a generic
        /// `T: ToString`) to keep the `Into<Signal<T>>` inference
        /// path unambiguous: with a generic T, `ReadSignal<String>`
        /// could match both `From<T>` (with T=ReadSignal<String>)
        /// and `From<ReadSignal<T>>` (with T=String). Fixing T at
        /// the call site removes the ambiguity entirely.
        ///
        /// [`ReadSignal<String>`]: ::whisker_runtime::reactive::ReadSignal
        /// [`RwSignal<String>`]: ::whisker_runtime::reactive::RwSignal
        pub fn style<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_styles(self.handle, v);
            self
        }
        pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, f: F) -> Self {
            set_event_listener(self.handle, "tap", ::std::boxed::Box::new(f));
            self
        }
        pub fn on<F: ::std::ops::Fn() + 'static>(self, event: &'static str, f: F) -> Self {
            set_event_listener(self.handle, event, ::std::boxed::Box::new(f));
            self
        }
        /// Lynx `class` attribute. Same Signal<String> contract as
        /// [`style`](Self::style).
        pub fn class<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "class", v);
            self
        }
        /// Catch-all for any other Lynx attribute. Same Signal<String>
        /// contract as [`style`](Self::style).
        pub fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, name, v);
            self
        }
        pub fn child(self, child: Element) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            self.handle
        }

        /// The literal text content. Lynx's `text` attribute.
        pub fn text<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "text", v);
            self
        }
    }

    /// `<image>` — bitmap from a URL. Set `src=` (required) and
    /// optionally `style=` for sizing.
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
    impl image {
        /// Inline CSS. Accepts a static value (`String`),
        /// a [`ReadSignal<String>`] / [`RwSignal<String>`] for
        /// reactive updates, or any other `Into<Signal<String>>`
        /// source.
        ///
        /// `T` is fixed to `String` (rather than a generic
        /// `T: ToString`) to keep the `Into<Signal<T>>` inference
        /// path unambiguous: with a generic T, `ReadSignal<String>`
        /// could match both `From<T>` (with T=ReadSignal<String>)
        /// and `From<ReadSignal<T>>` (with T=String). Fixing T at
        /// the call site removes the ambiguity entirely.
        ///
        /// [`ReadSignal<String>`]: ::whisker_runtime::reactive::ReadSignal
        /// [`RwSignal<String>`]: ::whisker_runtime::reactive::RwSignal
        pub fn style<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_styles(self.handle, v);
            self
        }
        pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, f: F) -> Self {
            set_event_listener(self.handle, "tap", ::std::boxed::Box::new(f));
            self
        }
        pub fn on<F: ::std::ops::Fn() + 'static>(self, event: &'static str, f: F) -> Self {
            set_event_listener(self.handle, event, ::std::boxed::Box::new(f));
            self
        }
        /// Lynx `class` attribute. Same Signal<String> contract as
        /// [`style`](Self::style).
        pub fn class<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "class", v);
            self
        }
        /// Catch-all for any other Lynx attribute. Same Signal<String>
        /// contract as [`style`](Self::style).
        pub fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, name, v);
            self
        }
        pub fn child(self, child: Element) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            self.handle
        }

        /// Image source URL — Lynx's `src` attribute.
        pub fn src<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "src", v);
            self
        }
    }

    /// `<scroll_view>` — scrollable container. Set
    /// `scroll_orientation=` to `"vertical"` or `"horizontal"`.
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
    impl scroll_view {
        /// Inline CSS. Accepts a static value (`String`),
        /// a [`ReadSignal<String>`] / [`RwSignal<String>`] for
        /// reactive updates, or any other `Into<Signal<String>>`
        /// source.
        ///
        /// `T` is fixed to `String` (rather than a generic
        /// `T: ToString`) to keep the `Into<Signal<T>>` inference
        /// path unambiguous: with a generic T, `ReadSignal<String>`
        /// could match both `From<T>` (with T=ReadSignal<String>)
        /// and `From<ReadSignal<T>>` (with T=String). Fixing T at
        /// the call site removes the ambiguity entirely.
        ///
        /// [`ReadSignal<String>`]: ::whisker_runtime::reactive::ReadSignal
        /// [`RwSignal<String>`]: ::whisker_runtime::reactive::RwSignal
        pub fn style<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_styles(self.handle, v);
            self
        }
        pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, f: F) -> Self {
            set_event_listener(self.handle, "tap", ::std::boxed::Box::new(f));
            self
        }
        pub fn on<F: ::std::ops::Fn() + 'static>(self, event: &'static str, f: F) -> Self {
            set_event_listener(self.handle, event, ::std::boxed::Box::new(f));
            self
        }
        /// Lynx `class` attribute. Same Signal<String> contract as
        /// [`style`](Self::style).
        pub fn class<V>(self, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, "class", v);
            self
        }
        /// Catch-all for any other Lynx attribute. Same Signal<String>
        /// contract as [`style`](Self::style).
        pub fn attr<V>(self, name: &'static str, v: V) -> Self
        where
            V: ::std::convert::Into<Signal<::std::string::String>>,
        {
            apply_attr(self.handle, name, v);
            self
        }
        pub fn child(self, child: Element) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> Element {
            self.handle
        }

        /// Scroll direction — `"vertical"` (default) or
        /// `"horizontal"`. Maps to Lynx's `scroll-orientation`
        /// attribute.
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
    pub use crate::{
        computed, effect, for_each, on_cleanup, on_mount, provide_context, resource, resource_sync,
        run_blocking, run_on_main_thread, show, signal, spawn_local, use_context, with_context,
        ReadSignal, Resource, ResourceState, RwSignal, Signal, StoredValue, WriteSignal,
    };
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
