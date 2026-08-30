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
#[path = "builtins/mod.rs"]
pub mod __tags;

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
