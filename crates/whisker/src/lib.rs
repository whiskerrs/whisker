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
    create_owner, dispose_owner, effect, flush, flush_mounts, memo, mount_component, on_cleanup,
    on_mount, provide_context, signal, unmount_component, use_context, with_context, with_owner,
    ReadSignal, RwSignal, StoredValue, WriteSignal,
};
// Back-compat type alias. `memo()` now returns `ReadSignal<T>`; this
// alias keeps `Memo<T>` in old type signatures compiling. New code
// should write `ReadSignal<T>`.
#[allow(deprecated)]
pub use whisker_runtime::reactive::Memo;
// Control-flow components used by the `render!` macro.
pub use whisker_runtime::view::{for_each, show};
// `Children` is the conventional prop type for components that wrap
// non-kwarg child nodes in their `render!` invocation.
pub use whisker_runtime::view::Children;

// Re-export `typed_builder` so the `#[component]` macro's expansion
// can resolve `::whisker::__typed_builder::TypedBuilder` without
// requiring user crates to add `typed-builder` to their own
// dependencies. Internal — not part of the stable public surface.
#[doc(hidden)]
pub mod __typed_builder {
    pub use ::typed_builder::TypedBuilder;
}

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
    use whisker_runtime::reactive::effect;
    use whisker_runtime::view::{
        append_child, create_element, set_attribute, set_event_listener,
        set_inline_styles, ElementHandle,
    };

    // Each built-in tag is a struct + a hand-written inherent
    // `impl` block listing every method explicitly. **No
    // `macro_rules!` is used to emit methods.** The spike under
    // `crates/ra-spike` showed that rust-analyzer's
    // method-completion engine doesn't surface methods that came
    // from a `macro_rules!` expansion inside an `impl` block —
    // even though the same methods compile and pass type-check
    // fine. Inline definitions fix the completion path; the
    // duplication across six tags is the cost.

    /// `<page>` — top-level container Lynx mounts as the root of
    /// an app. Holds the screen-level `style=` (background, flex
    /// direction) and a single content subtree.
    #[allow(non_camel_case_types)]
    pub struct page { handle: ElementHandle }
    #[allow(non_snake_case)]
    pub fn __page_ctor() -> page {
        page { handle: create_element(ElementTag::Page) }
    }
    impl page {
        /// Inline CSS — value-via-closure so signal-reading
        /// expressions re-apply on each dep change.
        pub fn style<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_inline_styles(h, &f().to_string()));
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
        pub fn class<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "class", &f().to_string()));
            self
        }
        /// Catch-all for any other Lynx attribute.
        pub fn attr<F, T>(self, name: &'static str, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, name, &f().to_string()));
            self
        }
        /// Append a child handle.
        pub fn child(self, child: ElementHandle) -> Self {
            append_child(self.handle, child);
            self
        }
        /// Finish building and return the underlying handle.
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }
    }

    /// `<view>` — Lynx's flex container. The most basic layout
    /// primitive in Whisker: a rectangular box that lays out its
    /// children with CSS flexbox. Equivalent to `<View>` in
    /// React Native or `<div>` in the web.
    #[allow(non_camel_case_types)]
    pub struct view { handle: ElementHandle }
    #[allow(non_snake_case)]
    pub fn __view_ctor() -> view {
        view { handle: create_element(ElementTag::View) }
    }
    impl view {
        /// Inline CSS — value-via-closure so signal-reading
        /// expressions re-apply on each dep change.
        pub fn style<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_inline_styles(h, &f().to_string()));
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
        pub fn class<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "class", &f().to_string()));
            self
        }
        /// Catch-all for any other Lynx attribute.
        pub fn attr<F, T>(self, name: &'static str, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, name, &f().to_string()));
            self
        }
        /// Append a child handle.
        pub fn child(self, child: ElementHandle) -> Self {
            append_child(self.handle, child);
            self
        }
        /// Finish building and return the underlying handle.
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }
    }

    /// `<text>` — text container. The actual glyphs live in
    /// `raw_text` child elements that the macro creates from
    /// string-literal children.
    #[allow(non_camel_case_types)]
    pub struct text { handle: ElementHandle }
    #[allow(non_snake_case)]
    pub fn __text_ctor() -> text {
        text { handle: create_element(ElementTag::Text) }
    }
    impl text {
        pub fn style<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_inline_styles(h, &f().to_string()));
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
        pub fn class<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "class", &f().to_string()));
            self
        }
        pub fn attr<F, T>(self, name: &'static str, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, name, &f().to_string()));
            self
        }
        pub fn child(self, child: ElementHandle) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }
    }

    /// `<raw_text>` — leaf text node with a `text` attribute.
    /// Normally the macro creates these automatically from
    /// string-literal children of `text` / `view`.
    #[allow(non_camel_case_types)]
    pub struct raw_text { handle: ElementHandle }
    #[allow(non_snake_case)]
    pub fn __raw_text_ctor() -> raw_text {
        raw_text { handle: create_element(ElementTag::RawText) }
    }
    impl raw_text {
        pub fn style<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_inline_styles(h, &f().to_string()));
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
        pub fn class<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "class", &f().to_string()));
            self
        }
        pub fn attr<F, T>(self, name: &'static str, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, name, &f().to_string()));
            self
        }
        pub fn child(self, child: ElementHandle) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }

        /// The literal text content. Lynx's `text` attribute.
        pub fn text<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "text", &f().to_string()));
            self
        }
    }

    /// `<image>` — bitmap from a URL. Set `src=` (required) and
    /// optionally `style=` for sizing.
    #[allow(non_camel_case_types)]
    pub struct image { handle: ElementHandle }
    #[allow(non_snake_case)]
    pub fn __image_ctor() -> image {
        image { handle: create_element(ElementTag::Image) }
    }
    impl image {
        pub fn style<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_inline_styles(h, &f().to_string()));
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
        pub fn class<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "class", &f().to_string()));
            self
        }
        pub fn attr<F, T>(self, name: &'static str, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, name, &f().to_string()));
            self
        }
        pub fn child(self, child: ElementHandle) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }

        /// Image source URL — Lynx's `src` attribute.
        pub fn src<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "src", &f().to_string()));
            self
        }
    }

    /// `<scroll_view>` — scrollable container. Set
    /// `scroll_orientation=` to `"vertical"` or `"horizontal"`.
    #[allow(non_camel_case_types)]
    pub struct scroll_view { handle: ElementHandle }
    #[allow(non_snake_case)]
    pub fn __scroll_view_ctor() -> scroll_view {
        scroll_view { handle: create_element(ElementTag::ScrollView) }
    }
    impl scroll_view {
        pub fn style<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_inline_styles(h, &f().to_string()));
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
        pub fn class<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "class", &f().to_string()));
            self
        }
        pub fn attr<F, T>(self, name: &'static str, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, name, &f().to_string()));
            self
        }
        pub fn child(self, child: ElementHandle) -> Self {
            append_child(self.handle, child);
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }

        /// Scroll direction — `"vertical"` (default) or
        /// `"horizontal"`. Maps to Lynx's `scroll-orientation`
        /// attribute.
        pub fn scroll_orientation<F, T>(self, f: F) -> Self
        where
            F: ::std::ops::Fn() -> T + 'static,
            T: ::std::string::ToString,
        {
            let h = self.handle;
            effect(move || set_attribute(h, "scroll-orientation", &f().to_string()));
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
    use whisker_runtime::view::ElementHandle;

    #[cfg(feature = "hot-reload")]
    #[inline(always)]
    pub fn call_user_app(f: fn() -> ElementHandle) -> ElementHandle {
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
    pub fn call_user_app(f: fn() -> ElementHandle) -> ElementHandle {
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
    pub use crate::ElementTag;
    #[allow(deprecated)]
    pub use crate::Memo;
    pub use crate::{component, main, render};
    pub use crate::Children;
    pub use crate::{
        effect, for_each, memo, on_cleanup, on_mount, provide_context, run_on_main_thread, show,
        signal, use_context, with_context, ReadSignal, RwSignal, StoredValue, WriteSignal,
    };
    // Built-in tag builder constructors re-exported so RA's
    // identifier completion (`v|` → `view`, `pa|` → `page`, …)
    // works in source positions outside `render!`. The macro
    // emits these as fully-qualified path expressions; user code
    // rarely calls them directly.
    #[doc(hidden)]
    pub use crate::__tags::{image, page, raw_text, scroll_view, text, view};
}
