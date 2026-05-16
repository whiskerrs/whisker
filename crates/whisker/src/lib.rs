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

// Re-export commonly used types so users don't need to depend on the
// inner crates directly.
pub use whisker_runtime::build;
pub use whisker_runtime::element::{Element, ElementTag};
pub use whisker_runtime::renderer::Renderer;
pub use whisker_runtime::signal::{use_signal, Signal};

pub use whisker_macros::{component, main, render, rsx};

// Re-export the new reactive primitives at the top level so user code
// can write `use whisker::*` and reach `signal`, `effect`, etc. The
// underlying impl lives in `whisker_runtime::reactive` and is still
// available there for code that prefers the long path.
pub use whisker_runtime::reactive::{
    create_owner, dispose_owner, effect, flush, flush_mounts, memo, mount_component, on_cleanup,
    on_mount, provide_context, signal, unmount_component, use_context, with_context, with_owner,
    Memo, ReadSignal, RwSignal, StoredValue, WriteSignal,
};
// Control-flow components used by the `render!` macro.
pub use whisker_runtime::view::{for_each, show};

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

/// Common imports for Whisker app code.
pub mod prelude {
    pub use crate::build::{image, page, raw_text, scroll_view, text, text_with, view};
    pub use crate::{component, main, render, rsx, use_signal};
    pub use crate::{Element, ElementTag, Signal};
    // Phase 6.5a reactive surface — the new API. Once `render!`
    // (A3) lands and the old value-tree API retires, the prelude
    // here will drop `use_signal` / `Signal` and rsx.
    pub use crate::{
        effect, memo, on_cleanup, on_mount, provide_context, signal, use_context, with_context,
        Memo, ReadSignal, RwSignal, StoredValue, WriteSignal,
    };
}
