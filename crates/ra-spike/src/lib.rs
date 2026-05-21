//! Minimal scaffolding for the rust-analyzer completion spike.
//!
//! Mirrors the shapes whisker uses for built-in and user-component
//! emission, but stripped of all runtime concerns (no signals, no
//! effects — just enough surface to verify what RA can complete).
//!
//! Test cases live in `examples/ra_completion.rs`. Open that file
//! in VS Code, restart rust-analyzer if needed, and try the
//! completion-trigger positions marked with `// ← TEST: …`.

pub use ra_spike_macros::{
    compose_a, compose_b, compose_c, render, render_g, render_h, render_i, render_j,
};

pub struct ElementHandle(pub u32);

// ---- Built-in tag builder, "whisker built-in" shape ------------

#[doc(hidden)]
pub mod __tags {
    use super::ElementHandle;

    #[allow(non_camel_case_types)]
    pub struct view {
        pub(super) handle: ElementHandle,
    }

    #[allow(non_snake_case)]
    pub fn __view_ctor() -> view {
        view { handle: ElementHandle(0) }
    }

    impl view {
        /// Inline CSS string.
        pub fn style(self, _value: impl ::std::string::ToString + 'static) -> Self {
            self
        }
        /// Lynx class.
        pub fn class(self, _value: impl ::std::string::ToString + 'static) -> Self {
            self
        }
        /// Tap handler.
        pub fn on_tap(self, _f: impl ::std::ops::Fn() + 'static) -> Self {
            self
        }
        /// Generic event handler.
        pub fn on(
            self,
            _event: &'static str,
            _f: impl ::std::ops::Fn() + 'static,
        ) -> Self {
            self
        }
        /// Catch-all attribute.
        pub fn attr(
            self,
            _name: &'static str,
            _value: impl ::std::string::ToString + 'static,
        ) -> Self {
            self
        }
        /// Append child.
        pub fn child(self, _child: ElementHandle) -> Self {
            self
        }
        /// H-variant: text child added directly to the parent
        /// builder, NO nested `.child({ chain })`.
        pub fn text_child(self, _value: impl ::std::string::ToString + 'static) -> Self {
            self
        }
        /// Finish building.
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }
    }

    /// G-variant: builds a text element as a single free function
    /// call. Used to test whether the nested method chain inside
    /// `.child({ … })` is what disrupts completion on the parent's
    /// kwarg position.
    #[allow(non_snake_case)]
    pub fn __text_make(_value: impl ::std::string::ToString + 'static) -> ElementHandle {
        ElementHandle(0)
    }

    #[allow(non_camel_case_types)]
    pub struct text {
        pub(super) handle: ElementHandle,
    }

    #[allow(non_snake_case)]
    pub fn __text_ctor() -> text {
        text { handle: ElementHandle(0) }
    }

    impl text {
        /// Text content.
        pub fn text(self, _value: impl ::std::string::ToString + 'static) -> Self {
            self
        }
        #[allow(non_snake_case)]
        pub fn __h(self) -> ElementHandle {
            self.handle
        }
    }
}

// ---- User-component shape (variant C uses these) ---------------

pub struct ViewProps {
    pub style: ::std::option::Option<String>,
    pub on_tap: ::std::option::Option<::std::boxed::Box<dyn ::std::ops::Fn()>>,
    pub class: ::std::option::Option<String>,
}

impl ViewProps {
    /// Variant-C entry point. Mirrors typed-builder's API surface
    /// just enough to test if RA hooks into it the same way.
    pub fn builder() -> ViewPropsBuilder {
        ViewPropsBuilder
    }
}

pub struct ViewPropsBuilder;

impl ViewPropsBuilder {
    pub fn style(self, _v: impl Into<String>) -> Self {
        self
    }
    pub fn on_tap<F: ::std::ops::Fn() + 'static>(self, _f: F) -> Self {
        self
    }
    pub fn class(self, _v: impl Into<String>) -> Self {
        self
    }
    pub fn build(self) -> ViewProps {
        ViewProps {
            style: None,
            on_tap: None,
            class: None,
        }
    }
}

pub fn view(_props: ViewProps) -> ElementHandle {
    ElementHandle(0)
}

// ---- J-variant top-level helpers ---------------------------------
//
// `view()` works as a child in D because `view` is *also* a real
// `pub fn` at this crate's root (the compose_c path uses it). When
// RA's input fixup tries to resolve the children-block tokens, it
// can name-resolve `view` → that `fn`. For J's `text("…")` and
// `expr(x)` to enjoy the same treatment, the idents must also
// name-resolve. These free functions are otherwise unused — only
// here so the macro INPUT looks like calls to real functions.

pub fn text(_value: impl ::std::string::ToString) -> ElementHandle {
    ElementHandle(0)
}

pub fn expr<T: ::std::fmt::Display>(_value: T) -> ElementHandle {
    ElementHandle(0)
}
