//! # Tuft
//!
//! Cross-platform mobile UI framework for Rust, built on the Lynx C++ engine.
//!
//! Most users only need:
//!
//! ```ignore
//! use tuft::prelude::*;
//!
//! #[tuft::main]
//! fn app() -> Element {
//!     rsx! {
//!         page { style: "background: white;",
//!             text { "Hello, Tuft" }
//!         }
//!     }
//! }
//! ```

pub use tuft_app_config as app_config;
pub use tuft_runtime as runtime;

// Re-export commonly used types so users don't need to depend on the
// inner crates directly.
pub use tuft_runtime::build;
pub use tuft_runtime::element::{Element, ElementTag};
pub use tuft_runtime::renderer::Renderer;
pub use tuft_runtime::signal::{use_signal, Signal};

pub use tuft_macros::{main, rsx};

/// Internal runtime entry points used by code the `#[tuft::main]` macro
/// expands to. Not stable, not for direct use.
#[doc(hidden)]
pub mod __main_runtime {
    pub use tuft_driver::bootstrap::{run, tick};

    /// Wrap one invocation of the user's `app` function for hot-patch
    /// dispatch. The `#[tuft::main]` macro calls this unconditionally
    /// from inside the user crate so we don't need a user-crate-local
    /// `hot-reload` feature flag to gate the call site.
    ///
    /// The cfg flip happens here, at tuft's compile-time, on tuft's
    /// own `hot-reload` feature:
    ///
    /// - **on** (`tuft run` / Tier 1): body is
    ///   `subsecond::call(|| f())`. The `#[inline(always)]` makes the
    ///   body land in the *user crate's* compilation unit at every
    ///   call site, so the wrapper closure's `<F as HotFunction<()>>::
    ///   call_it` monomorphization is part of `libhello_world.so`
    ///   (host) *and* `target/.tuft/patches/libhello_world.so` (patch).
    ///   That's the symbol `subsecond::apply_patch`'s JumpTable maps
    ///   host → patch; without it, hot patches don't dispatch and the
    ///   screen keeps showing pre-edit content.
    /// - **off** (release): body collapses to `f()`, `subsecond` is
    ///   not pulled in at all.
    #[cfg(feature = "hot-reload")]
    #[inline(always)]
    pub fn call_user_app(f: fn() -> crate::Element) -> crate::Element {
        ::subsecond::call(|| f())
    }

    #[cfg(not(feature = "hot-reload"))]
    #[inline(always)]
    pub fn call_user_app(f: fn() -> crate::Element) -> crate::Element {
        f()
    }
}

/// Common imports for Tuft app code.
pub mod prelude {
    pub use crate::build::{image, page, raw_text, scroll_view, text, text_with, view};
    pub use crate::{Element, ElementTag, Signal};
    pub use crate::{main, rsx, use_signal};
}
