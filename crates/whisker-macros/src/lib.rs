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
use syn::{ImplItem, ItemFn, ItemImpl, parse_macro_input};

mod component;
mod css;
mod module_component;
mod render;

/// Marks one platform implementation of [`WhiskerModule`].
///
/// The implementation remains an ordinary Rust trait implementation. The
/// attribute only emits the private, conventionally-named adapter consumed by
/// Whisker's generated composition root, keeping that build detail out of the
/// module author's API.
#[allow(non_snake_case)]
#[proc_macro_attribute]
pub fn WhiskerModule(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let implementation = parse_macro_input!(item as ItemImpl);
    let Some((_, trait_path, _)) = &implementation.trait_ else {
        return syn::Error::new_spanned(
            &implementation.self_ty,
            "#[WhiskerModule] must annotate a WhiskerModule trait implementation",
        )
        .into_compile_error()
        .into();
    };
    if trait_path
        .segments
        .last()
        .is_none_or(|segment| segment.ident != "WhiskerModule")
    {
        return syn::Error::new_spanned(
            trait_path,
            "#[WhiskerModule] must annotate a WhiskerModule trait implementation",
        )
        .into_compile_error()
        .into();
    }

    let module_type = &implementation.self_ty;
    let Some(definition_type) = implementation.items.iter().find_map(|item| match item {
        ImplItem::Type(associated_type) if associated_type.ident == "Definition" => {
            Some(&associated_type.ty)
        }
        _ => None,
    }) else {
        return syn::Error::new_spanned(
            &implementation.self_ty,
            "#[WhiskerModule] requires an explicit Definition associated type",
        )
        .into_compile_error()
        .into();
    };
    quote! {
        #implementation

        #[doc(hidden)]
        #[allow(dead_code)]
        pub fn __whisker_module_definition(
        ) -> #definition_type {
            <#module_type as #trait_path>::definition()
        }
    }
    .into()
}

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

    // Deterministic hash of the app fn's token stream, so the
    // hot-reload runtime can ask "did the `app()` source change in this
    // patch?". FNV-1a over the token string: stable across compilations
    // of identical tokens (unlike `DefaultHasher`), and token-level so
    // formatting-only edits don't shift it.
    let body_hash = fnv1a64(&quote!(#func).to_string());

    let expanded = quote! {
        #func

        // `call_user_app` is `#[inline(always)]` so the wrapper body
        // lands in the USER crate's compilation unit. Whether it
        // dispatches through `subsecond::call` or invokes `#fn_name()`
        // directly is decided by `whisker`'s own `hot-reload` feature,
        // so the user crate needs no matching feature.
        fn __whisker_app_dispatch() -> ::whisker::runtime::view::Element {
            ::whisker::__main_runtime::call_user_app(#fn_name)
        }

        // Typed Rust entry used by the native Desktop composition root. The
        // generated executable links the application rlib directly, so this
        // path needs neither a C ABI nor an encoded bridge payload.
        #[doc(hidden)]
        pub fn __whisker_application() -> ::whisker::runtime::view::Element {
            __whisker_app_dispatch()
        }

        // Source-hash pair for the full-remount trigger. The dispatch
        // wrapper routes through `call_app_hash`, so after a patch the
        // runtime reads the *patch dylib's* hash. A changed value means
        // the user edited `app()` itself — which no `#[component]`
        // remount can reflect — and the bootstrap re-runs it.
        #[doc(hidden)]
        fn __whisker_app_body_hash() -> u64 {
            #body_hash
        }

        #[doc(hidden)]
        fn __whisker_app_hash_dispatch() -> u64 {
            ::whisker::__main_runtime::call_app_hash(__whisker_app_body_hash)
        }

        // `#[unsafe(no_mangle)]`, not bare `#[no_mangle]`: this macro
        // expands in the USER crate's edition, and a bare `#[no_mangle]`
        // is a hard error under edition 2024.
        // Platform-neutral retained renderer entry points. Android and iOS
        // provide only wake/present callbacks; application construction,
        // layout, and frame production stay in shared Rust.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub extern "C" fn whisker_view_create(
            width: f32,
            height: f32,
            scale: f32,
            request_frame: extern "C" fn(*mut ::std::ffi::c_void),
            request_frame_data: *mut ::std::ffi::c_void,
            bootstrap: ::whisker::__mobile_abi::BootstrapCallback,
            bootstrap_data: *mut ::std::ffi::c_void,
            measure: ::whisker::__mobile_abi::MeasureCallback,
            measure_data: *mut ::std::ffi::c_void,
            present_frame: ::whisker::__mobile_abi::PresentFrameCallback,
            present_frame_data: *mut ::std::ffi::c_void,
            resource_command: ::whisker::__mobile_abi::ResourceCommandCallback,
            resource_data: *mut ::std::ffi::c_void,
            invoke_module: ::whisker::__mobile_module::InvokeModuleCallback,
            observe_module: ::whisker::__mobile_module::ObserveModuleCallback,
            module_data: *mut ::std::ffi::c_void,
        ) -> *mut ::std::ffi::c_void {
            ::whisker::__mobile_runtime::create(
                width,
                height,
                scale,
                request_frame,
                request_frame_data,
                bootstrap,
                bootstrap_data,
                measure,
                measure_data,
                present_frame,
                present_frame_data,
                resource_command,
                resource_data,
                invoke_module,
                observe_module,
                module_data,
                __whisker_app_dispatch,
            )
        }

        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn whisker_view_tick(
            handle: *mut ::std::ffi::c_void,
            timestamp_ms: f64,
            width: f32,
            height: f32,
            scale: f32,
        ) -> bool {
            unsafe {
                ::whisker::__mobile_runtime::tick(
                    handle,
                    timestamp_ms,
                    width,
                    height,
                    scale,
                )
            }
        }

        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn whisker_view_destroy(
            handle: *mut ::std::ffi::c_void,
        ) {
            unsafe { ::whisker::__mobile_runtime::destroy(handle) }
        }

        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn whisker_view_dispatch_event(
            handle: *mut ::std::ffi::c_void,
            timestamp_ms: f64,
            node: u64,
            name: *const u8,
            name_len: usize,
            detail: *const ::whisker::__mobile_abi::WhiskerValueRaw,
        ) -> bool {
            unsafe {
                ::whisker::__mobile_runtime::dispatch_event(
                    handle,
                    timestamp_ms,
                    node,
                    name,
                    name_len,
                    detail,
                )
            }
        }

        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn whisker_view_dispatch_pointer(
            handle: *mut ::std::ffi::c_void,
            timestamp_ms: f64,
            event: u32,
            pointer_id: u64,
            pointer_kind: u32,
            x: f32,
            y: f32,
            buttons: u32,
            changed_button: i16,
        ) -> bool {
            unsafe {
                ::whisker::__mobile_runtime::dispatch_pointer(
                    handle,
                    timestamp_ms,
                    event,
                    pointer_id,
                    pointer_kind,
                    x,
                    y,
                    buttons,
                    changed_button,
                )
            }
        }

        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn whisker_view_dispatch_module_event(
            handle: *mut ::std::ffi::c_void,
            module: *const u8,
            module_len: usize,
            event: *const u8,
            event_len: usize,
            payload: *const ::whisker::__mobile_abi::WhiskerValueRaw,
        ) -> bool {
            unsafe {
                ::whisker::__mobile_runtime::dispatch_module_event(
                    handle,
                    module,
                    module_len,
                    event,
                    event_len,
                    payload,
                )
            }
        }

        #[cfg(any(target_os = "android", target_os = "ios"))]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn whisker_view_dispatch_resource_event(
            handle: *mut ::std::ffi::c_void,
            event: *const ::whisker::__mobile_abi::MobileResourceEvent,
        ) -> bool {
            unsafe {
                ::whisker::__mobile_runtime::dispatch_resource_event(handle, event)
            }
        }

        // Anchor symbol the vendored subsecond fork uses to compute
        // the ASLR slide between this dylib's static layout and its
        // runtime load address. Both the host dylib and every patch
        // dylib must export it so
        // `dlsym(RTLD_DEFAULT, "whisker_aslr_anchor")` resolves inside
        // the user's `.so`.
        //
        // A unique name rather than upstream subsecond's `main`
        // sentinel: on Android the process linker namespace already
        // holds several `main` symbols (`app_process64`'s, plus prior
        // memfd patches), so a dlsym for `main` returns the wrong one
        // and the slide math computes garbage.
        //
        // The stub never runs — it only needs to exist in the export
        // list at a known static address.
        #[cfg(not(target_arch = "wasm32"))]
        #[unsafe(no_mangle)]
        pub extern "C" fn whisker_aslr_anchor() -> ::std::ffi::c_int { 0 }
    };

    expanded.into()
}

/// FNV-1a 64-bit over a string. Used for the `#[whisker::main]`
/// app-body hash and the `#[component]` props-layout hash, both of
/// which must produce the same value for the same token string in every
/// rustc process (host fat build AND thin patch build) — which rules
/// out `DefaultHasher`.
pub(crate) fn fnv1a64(s: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h = FNV_OFFSET;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
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
/// grammar. Children may be components, string literals, or `{expr}` values;
/// the Rust renderer validates them against the parent's child policy.
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
/// invocations do not compile. User components are invoked exclusively
/// through `render!`'s `Xxx(a: …, b: …)` syntax, which lowers to
/// `Xxx(XxxProps::builder().a(…).b(…).build())`. This unifies the
/// call-site shape with built-in elements (`view(…)`).
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
/// // Call site (always through `render!`, under the PascalCase alias):
/// render! { Counter(initial: 0) }
/// ```
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(item.into()).into()
}

/// Declare a Whisker-side wrapper for a Lynx-registered view module's element.
///
/// ```ignore
/// #[whisker::module_component(
///     name = "example.ui/Hello",
///     measurement = None,
/// )]
/// pub fn hello(style: Signal<String>) {}
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
/// `name` is the stable, versionless identity shared by Rust authoring and
/// independently compiled platform `WhiskerModule` declarations. Bootstrap
/// validates the strings and binds them to Rust-assigned compact IDs.
/// Properties and events receive generated numeric IDs from the function
/// signature. `Children` selects ordinary element children and
/// `TextChildren` selects normalized plain-text content.
/// The generated builder implements Whisker's common `ElementBuilder` API,
/// so universal authoring features such as `style`, `id`, accessibility,
/// gestures, and `ref` can be used without adding component-specific schema
/// entries. A declared `style` parameter remains supported for compatibility
/// and is excluded from the schema.
/// The legacy string-literal form remains available while existing Lynx
/// modules migrate.
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
/// emission details.
#[proc_macro_attribute]
pub fn module_component(attr: TokenStream, item: TokenStream) -> TokenStream {
    module_component::expand(attr.into(), item.into()).into()
}

/// Declares the schema half of a Whisker built-in component.
///
/// This internal macro accepts the same `name`, `measurement`, and function
/// signature model as [`module_component`], but deliberately does not generate
/// an authoring builder. Whisker's built-in builders retain their specialized
/// lowering and bind the generated schema to an internal `ElementTag`.
#[doc(hidden)]
#[proc_macro_attribute]
pub fn builtin_component(attr: TokenStream, item: TokenStream) -> TokenStream {
    module_component::expand_builtin(attr.into(), item.into()).into()
}
