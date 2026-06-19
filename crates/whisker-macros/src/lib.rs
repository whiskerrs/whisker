//! Procedural macros for Whisker.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `whisker_app_main` and `whisker_tick` FFI exports the native
//!   host calls into; the user writes `fn app() -> Element`.
//! - [`render!`] — fine-grained renderer macro. Emits imperative
//!   `view::*` dispatch + `effect`s for dynamic parts. See
//!   `crates/whisker-macros/src/render.rs` for the grammar.
//! - [`component`] — wraps a function so it runs inside a fresh
//!   reactive owner. The owner is registered against the function's
//!   fn pointer so the hot-reload remount path can find it. See
//!   `docs/reactivity-design.md`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

mod component;
mod css;
mod module_component;
mod render;

/// Annotates the user's app function (returning `whisker::Element`) and
/// generates the FFI symbols the iOS/Android host expects.
///
/// ```ignore
/// use whisker::prelude::*;
///
/// #[whisker::main]
/// fn app() -> Element {
///     render! { view(style: "flex-grow: 1;") { text(value: "Hello") } }
/// }
/// ```
///
/// Expands to (roughly):
///
/// ```ignore
/// fn app() -> Element { /* user body */ }
///
/// #[unsafe(no_mangle)]
/// pub extern "C" fn whisker_app_main(
///     engine: *mut std::ffi::c_void,
///     request_frame: Option<extern "C" fn(*mut std::ffi::c_void)>,
///     request_frame_data: *mut std::ffi::c_void,
/// ) {
///     ::whisker::__main_runtime::run(engine, request_frame, request_frame_data, app);
/// }
///
/// #[unsafe(no_mangle)]
/// pub extern "C" fn whisker_tick(engine: *mut std::ffi::c_void) -> bool {
///     ::whisker::__main_runtime::tick(engine)
/// }
/// ```
///
/// `request_frame` is the host's "wake up the render loop" callback. The
/// runtime invokes it when a signal update marks the tree dirty so the
/// host can unpause its `CADisplayLink` (or equivalent) to schedule the
/// next tick. Pass `None` to opt into an unconditional 60Hz loop.
///
/// `whisker_tick` returns `true` when the runtime is idle after the tick;
/// the host can pause its render loop until the next `request_frame`
/// fires.
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;

    let expanded = quote! {
        #func

        // The app fn the runtime invokes every frame. Unconditionally
        // routes through `whisker::__main_runtime::call_user_app`, which
        // is `#[inline(always)]` so the wrapper body lands in the user
        // crate's compilation unit. Whether the wrapper actually
        // dispatches through `subsecond::call` (Tier 1 / hot-reload
        // on) or just invokes `#fn_name()` directly (release) is
        // decided by `whisker`'s own `hot-reload` feature flag — the
        // user crate doesn't need a matching feature of its own.
        fn __whisker_app_dispatch() -> ::whisker::runtime::view::Element {
            ::whisker::__main_runtime::call_user_app(#fn_name)
        }

        // `#[unsafe(no_mangle)]` (not bare `#[no_mangle]`): in edition
        // 2024 a bare `#[no_mangle]` is a hard error, and this macro
        // expands in the USER crate's edition. The `#[unsafe(...)]`
        // attribute spelling was stabilized in Rust 1.82 — below the
        // workspace MSRV of 1.85 — so it compiles cleanly in both 2021
        // and 2024 user crates.
        #[unsafe(no_mangle)]
        pub extern "C" fn whisker_app_main(
            engine: *mut ::std::ffi::c_void,
            request_frame: ::std::option::Option<
                extern "C" fn(*mut ::std::ffi::c_void),
            >,
            request_frame_data: *mut ::std::ffi::c_void,
        ) {
            ::whisker::__main_runtime::run(
                engine,
                request_frame,
                request_frame_data,
                __whisker_app_dispatch,
            );
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn whisker_tick(engine: *mut ::std::ffi::c_void) -> bool {
            ::whisker::__main_runtime::tick(engine)
        }

        // Anchor symbol used by Whisker's vendored subsecond fork to
        // compute the ASLR slide between this dylib's static layout
        // (cached server-side) and its runtime load address. Both the
        // host dylib and every patch dylib must export this so
        // `dlsym(RTLD_DEFAULT, "whisker_aslr_anchor")` resolves
        // unambiguously inside the user's `.so`.
        //
        // Why a unique name instead of `main` (upstream subsecond's
        // sentinel): on Android, Whisker is loaded via
        // `System.loadLibrary` into a process whose linker namespace
        // already contains several `main` symbols
        // (`app_process64`'s, plus any prior memfd patches), so a
        // dlsym for `main` returns the wrong one and the slide math
        // computes garbage. A unique name only exists in the user's
        // `.so`, so the lookup is collision-free regardless of
        // namespace order.
        //
        // The stub never runs — Whisker is JNI-loaded, never executed
        // as a process entry point. It only needs to exist in the
        // export list at a known static address.
        #[unsafe(no_mangle)]
        pub extern "C" fn whisker_aslr_anchor() -> ::std::ffi::c_int { 0 }
    };

    expanded.into()
}

/// Fine-grained renderer macro. Emits imperative element-creation
/// code that calls into [`whisker::runtime::view`] through the
/// thread-local installed renderer, and returns an [`Element`].
///
/// ```ignore
/// use whisker::prelude::*;
///
/// let handle = render! {
///     view(
///         style: "padding: 16px;",
///         on_tap: move |_| println!("tapped"),
///     ) {
///         text(value: "Hello, world")
///     }
/// };
/// ```
///
/// See `crates/whisker-macros/src/render.rs` for the full kwarg
/// grammar. Dynamic values flow through `Signal<T>` props; bare
/// `{expr}` blocks inside a children list are rejected (use
/// `text(value: <expr>)` instead).
#[proc_macro]
pub fn render(input: TokenStream) -> TokenStream {
    render::expand(input)
}

/// `css!(name: value, …)` — kwarg syntax for the [`Css`] builder.
///
/// Lowers to a [`Css::new()`] method chain (`Css::new().name(value)
/// .…`). `Css` is taken from the call site's scope, so
/// `use whisker::prelude::*` (which re-exports `Css`) is the only
/// import callers need.
///
/// The proc-macro implementation tolerates partial input from
/// rust-analyzer's completion engine: a kwarg whose value hasn't
/// been typed yet (`css!(back|`) is expanded as
/// `.<name>(())` so RA still sees a real method-call site and
/// fires its method-name completion. The unit `()` is intentionally
/// type-incorrect; the program already doesn't compile while the
/// user is mid-typing.
///
/// ```ignore
/// use whisker::prelude::*;
///
/// let s = css!(
///     background_color: Color::hex(0x1A1330),
///     padding: (px(8), px(16)),
///     border: Border::new().width(px(1)).style(BorderStyle::Solid),
/// );
/// ```
///
/// [`Css`]: whisker_css::Css
/// [`Css::new()`]: whisker_css::Css::new
#[proc_macro]
pub fn css(input: TokenStream) -> TokenStream {
    css::expand(input.into()).into()
}

/// Mark a function as a Whisker reactive component.
///
/// The macro takes the user's `fn xxx(a: A, b: B) -> Element`
/// and emits both:
///
/// 1. A `XxxProps` struct (Pascal-cased function name + `Props`)
///    derived from the parameter list, plus a hand-rolled
///    `XxxPropsBuilder` so callers can construct Props via
///    `XxxProps::builder().a(...).b(...).build()`.
///    Each setter accepts `impl Into<T>` for `Into` coercion on the
///    call side (`&str` → `String`, `i32` → `f64`, …).
///    `Option<T>` props get a strip-option setter (accept the inner
///    `T`) and default to `None` when omitted. `Children` props get
///    a default empty closure. A `#[prop(default = expr)]` attribute
///    on a parameter inserts `expr` as the field's default at `.build()`.
///    Required fields that the user didn't set panic at `.build()` with
///    `"required field `xxx` was not set"`.
///
/// 2. A rewritten `fn xxx(__props: XxxProps) -> Element` whose
///    body destructures the props back into local variables and runs
///    the user's original `#block` inside the existing
///    `mount_component_remountable` machinery (per-component
///    remount + subsecond hot-reload integration).
///
/// The signature change is deliberate: positional `xxx(a, b)`
/// invocations no longer compile. User components are now invoked
/// exclusively through `render!`'s `xxx { a: …, b: … }` syntax,
/// which the `render!` macro lowers to
/// `xxx(XxxProps::builder().a(…).b(…).build())`. This unifies the
/// call-site shape with built-in elements (`view { … }`).
///
/// ```ignore
/// use whisker::prelude::*;
///
/// #[component]
/// fn counter(initial: i32) -> Element {
///     let count = signal(initial);
///     render! { /* ... */ }
/// }
///
/// // Call site (always through `render!`):
/// render! { counter { initial: 0 } }
/// ```
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(item.into()).into()
}

/// Declare a Whisker-side wrapper for a Lynx-registered view module's element.
///
/// ```ignore
/// #[whisker::module_component("Hello")]
/// pub fn hello(style: Signal<String>) -> Element;
/// ```
///
/// Generates the same Props + builder + PascalCase-alias surface as
/// `#[component]`, but the function body is **auto-generated**: it
/// calls `view::create_element_by_name(tag)` and then applies each
/// declared prop as either an inline-style (for the `style` prop) or
/// a SetAttribute (everything else, kebab-cased). Static vs reactive
/// dispatch goes through the same `apply_styles` / `apply_attr`
/// helpers built-in tags use, so a `Signal::Dynamic` prop transparently
/// effect-wraps the attribute write.
///
/// The tag string passed to Lynx at runtime is
/// `<cargo-crate-name>:<attr-tag>` — the macro auto-prepends
/// `env!("CARGO_PKG_NAME")` so two unrelated module packages can
/// both declare a component named `Hello` without colliding in
/// Lynx's behaviour registry. The matching platform-side
/// `@WhiskerModule` DSL `Name(...)` is namespaced the same way by
/// the per-platform codegen.
///
/// Imperative methods on a mounted element are dispatched through
/// the element's `ElementRef` (`ref:` prop) via
/// `ElementRef::invoke(method, args)` — there is no separate
/// element-method declaration macro.
///
/// Call-site shape mirrors built-in tags + user components:
///
/// ```ignore
/// render! {
///     Hello(style: "width: 100%; height: 8px;")
/// }
/// ```
///
/// See `crates/whisker-macros/src/module_component.rs` for the
/// emission details (children props are NOT yet supported; tracked
/// in follow-ups).
#[proc_macro_attribute]
pub fn module_component(attr: TokenStream, item: TokenStream) -> TokenStream {
    module_component::expand(attr.into(), item.into()).into()
}
