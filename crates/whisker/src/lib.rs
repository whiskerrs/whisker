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
    Memo, ReadSignal, RwSignal, StoredValue, WriteSignal,
};
// Control-flow components used by the `render!` macro.
pub use whisker_runtime::view::{for_each, show};

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

    /// Dispatch a `#[component]` body through subsecond so that hot
    /// patches to the body (or any closure it transitively
    /// instantiates — effects, memos, event handlers) reach the
    /// running app.
    ///
    /// Without this wrapper, the body's `Rc<dyn Fn>` would be called
    /// directly via its vtable, which points at the OLD `call_it`
    /// address. Subsecond's `apply_patch` only updates the global
    /// JumpTable; old call sites that don't consult it keep running
    /// the pre-patch code. `HotFn::try_call` (what `subsecond::call`
    /// uses internally) does consult the JumpTable — looking up
    /// `<F as HotFunction>::call_it` for the wrapped closure and
    /// transmuting to the patched fn pointer.
    ///
    /// The closure passed here is anonymous — instantiated at the
    /// `#[component]` macro's expansion site in the user crate.
    /// Its `call_it` has a stable mangled name across recompiles, so
    /// patch builds keep the JumpTable entry pointing at the new
    /// address.
    ///
    /// Like `call_user_app`, this is `#[inline(always)]` so the
    /// `subsecond::call` invocation lands in the user crate's
    /// compilation unit — that's where the JumpTable expects to find
    /// the matching `call_it`.
    #[cfg(feature = "hot-reload")]
    #[inline(always)]
    pub fn call_component_body<F: FnOnce() -> ElementHandle>(body: F) -> ElementHandle {
        // `subsecond::call` wants `FnMut`. The user body is logically
        // `FnOnce` (each remount creates a fresh closure), so we
        // wrap in an `Option::take` adapter. `body` is only called
        // once per `call_component_body` invocation.
        let mut body_slot = Some(body);
        ::subsecond::call(move || (body_slot.take().expect("body called twice"))())
    }

    #[cfg(not(feature = "hot-reload"))]
    #[inline(always)]
    pub fn call_component_body<F: FnOnce() -> ElementHandle>(body: F) -> ElementHandle {
        body()
    }
}

/// Common imports for Whisker app code.
pub mod prelude {
    pub use crate::{component, main, render};
    pub use crate::ElementTag;
    pub use crate::{
        effect, for_each, memo, on_cleanup, on_mount, provide_context, run_on_main_thread, show,
        signal, use_context, with_context, Memo, ReadSignal, RwSignal, StoredValue, WriteSignal,
    };
}
