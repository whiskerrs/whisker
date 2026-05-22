//! `#[whisker::native_element]` proc-macro.
//!
//! Generates a builder-style API for a custom Lynx element identified
//! by a tag-name string. The macro shape mirrors `#[component]` —
//! same `<Name>Props` struct, same hand-rolled builder, same
//! PascalCase alias — but the function body is **auto-generated**
//! rather than supplied by the user. Each declared parameter
//! becomes an attribute on the underlying Lynx element.
//!
//! ## User syntax
//!
//! ```ignore
//! #[whisker::native_element("x-hello")]
//! pub fn x_hello(style: Signal<String>) {}
//! //                                    ^^
//! //                                    empty body — the macro replaces
//! //                                    it with the create_element_by_name
//! //                                    + apply_* sequence and rewrites
//! //                                    the return type to `Element`.
//! ```
//!
//! Rust's grammar requires a body for top-level `fn` items (no
//! `extern fn ...;` outside an `extern {}` block), so the
//! placeholder `{}` is unavoidable. The macro discards whatever
//! return type and body the user supplies — the auto-generated
//! body always returns `whisker::runtime::view::Element`.
//!
//! ## What the macro emits
//!
//! Conceptually:
//!
//! ```ignore
//! pub struct XHelloProps { style: Signal<String> }
//! impl XHelloProps {
//!     pub fn builder() -> XHelloPropsBuilder { … }
//! }
//! impl XHelloPropsBuilder {
//!     pub fn style(self, v: impl Into<Signal<String>>) -> Self { … }
//!     pub fn build(self) -> XHelloProps { … }
//! }
//! pub fn XHello(props: XHelloProps) -> Element {
//!     let h = ::whisker::runtime::view::create_element_by_name("x-hello");
//!     // For `style` specifically: route through apply_styles
//!     // (Lynx's SetRawInlineStyles). Every other declared param
//!     // becomes a `SetAttribute(name_kebab_case, value)`.
//!     ::whisker::__tags::apply_styles(h, props.style);
//!     h
//! }
//! ```
//!
//! The auto-generated body delegates static-vs-reactive dispatch to
//! the same `apply_styles` / `apply_attr` helpers that built-in tags
//! use, so a `Signal::Dynamic` prop transparently effect-wraps the
//! underlying SetAttribute / SetRawInlineStyles call — every native
//! element gets Whisker's reactive semantics for free.
//!
//! ## Call-site shape
//!
//! Same as user components and built-in tags. Inside `render!`:
//!
//! ```ignore
//! render! {
//!     XHello(style: "width: 100%; height: 8px;")
//! }
//! ```
//!
//! lowers to:
//!
//! ```ignore
//! XHello(XHelloProps::builder().style("width: 100%; height: 8px;").build())
//! ```
//!
//! ## What's NOT supported (yet)
//!
//! - **Children**: native elements with sub-elements (e.g. a custom
//!   `<x-card>{children}</x-card>`). The bridge supports
//!   `append_child` but the macro doesn't surface it yet.
//! - **Event handlers**: `on_<event>` props that map to
//!   `set_event_listener`. Easy to add — same shape as
//!   `__tags::*::on(...)`, just guarded by an attribute-name prefix
//!   in the macro.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{parse2, FnArg, Ident, ItemFn, LitStr, Pat, Type};

pub fn expand(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    // Attribute payload: a single string literal — the tag name
    // `lynx_create_fiber_element_by_name` will register against.
    let tag_name: LitStr = match parse2(attr.clone()) {
        Ok(s) => s,
        Err(_) => {
            return syn::Error::new(
                attr.span(),
                "#[whisker::native_element(\"<tag-name>\")] requires a \
                 string-literal tag name (e.g. `\"x-hello\"`, `\"x-input\"`)",
            )
            .to_compile_error();
        }
    };
    let tag_name_str = tag_name.value();

    let input: ItemFn = match parse2(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };

    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let fn_name = &sig.ident;

    if !sig.generics.params.is_empty() {
        return syn::Error::new(
            sig.generics.span(),
            "#[whisker::native_element] does not support generic parameters \
             — native elements are tag-name driven, not type-parameterised. \
             Each tag is a distinct registered Lynx UI class.",
        )
        .to_compile_error();
    }

    // Walk the parameter list. Each `Typed` arg becomes a `Prop` —
    // ident + type. We reject patterns and receivers up front.
    let mut props: Vec<Prop> = Vec::new();
    for arg in &sig.inputs {
        let pat_type = match arg {
            FnArg::Typed(t) => t,
            FnArg::Receiver(r) => {
                return syn::Error::new(
                    r.span(),
                    "#[whisker::native_element] does not support method receivers",
                )
                .to_compile_error();
            }
        };
        let ident = match &*pat_type.pat {
            Pat::Ident(pi) => pi.ident.clone(),
            other => {
                return syn::Error::new(
                    other.span(),
                    "#[whisker::native_element] parameters must be plain identifiers",
                )
                .to_compile_error();
            }
        };
        props.push(Prop {
            ident,
            ty: (*pat_type.ty).clone(),
        });
    }

    // ----- Props struct + builder ----------------------------------

    let props_name = format_ident!("{}", to_pascal_case(&fn_name.to_string()) + "Props");
    let builder_name = format_ident!("{}Builder", props_name);
    let internal_mod = format_ident!("__{}_props_internal", fn_name);

    let props_fields: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            let t = &p.ty;
            quote! { pub #i: #t }
        })
        .collect();

    let builder_fields: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            let t = &p.ty;
            quote! { #i: ::std::option::Option<#t> }
        })
        .collect();

    let builder_init: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            quote! { #i: ::std::option::Option::None }
        })
        .collect();

    // Each setter takes `impl Into<#ty>`. For Signal<T>-shaped types,
    // that means String / &str / ReadSignal<T> / RwSignal<T> / Memo<T>
    // all flow in via the existing From impls.
    let setters: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            let t = &p.ty;
            quote! {
                #[allow(unused_mut)]
                pub fn #i(mut self, value: impl ::std::convert::Into<#t>) -> Self {
                    self.#i = ::std::option::Option::Some(value.into());
                    self
                }
            }
        })
        .collect();

    let build_assignments: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            let name = i.to_string();
            let err = format!("required prop `{name}` was not set on `{tag_name_str}`");
            quote! {
                #i: self.#i.expect(#err)
            }
        })
        .collect();

    // ----- Auto-generated body --------------------------------------
    //
    // For each prop, decide whether it's `style` (→ apply_styles, i.e.
    // SetRawInlineStyles) or a regular attribute (→ apply_attr with the
    // kebab-cased prop name). Future native-element props will likely
    // want a `#[prop(attr = "data-foo")]` escape hatch for explicit
    // attribute-name overrides; not needed for the smoke test.
    // `apply_styles<V, T>` and `apply_attr<V, T>` are generic over the
    // Signal's inner T; without a hint the compiler can't pick
    // between `T = String` (identity Into on a Signal<String>) and
    // `T = Signal<String>` (wrap-as-Static on the whole Signal). We
    // hard-code `T = String` via turbofish — for v1, every native
    // element prop is `Signal<String>` and the resulting attribute /
    // inline-styles call already serialises through ToString anyway.
    // Future versions may extract T from a `Signal<U>` prop type
    // when U != String; until then this rule is documented in the
    // macro's doc-comment.
    let apply_calls: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            let name = i.to_string();
            if name == "style" {
                quote! {
                    ::whisker::__tags::apply_styles::<_, ::std::string::String>(
                        __handle, props.#i,
                    );
                }
            } else {
                let attr_name = name.replace('_', "-");
                quote! {
                    ::whisker::__tags::apply_attr::<_, ::std::string::String>(
                        __handle, #attr_name, props.#i,
                    );
                }
            }
        })
        .collect();

    let prop_idents: Vec<Ident> = props.iter().map(|p| p.ident.clone()).collect();
    let drop_unused = if prop_idents.is_empty() {
        // Quiet `unused_variables` when the user declares no props —
        // common for placeholder native elements like the `x-hello`
        // smoke test.
        quote! { let _ = props; }
    } else {
        quote! {}
    };

    let inner_mod = format_ident!("__{}_inner", fn_name);

    // PascalCase alias — same scheme as `#[component]`.
    let pascal_alias_ident = format_ident!("{}", to_pascal_case(&fn_name.to_string()));
    let fn_name_str = fn_name.to_string();
    let alias_emission = if pascal_alias_ident.to_string() == fn_name_str {
        // snake_case name already matches PascalCase (rare for native
        // elements; their convention is `x_input` → `XInput`). Skip
        // the alias to avoid `pub use … as same_name`.
        quote! {
            #[doc(hidden)]
            #vis use #inner_mod::#fn_name;
        }
    } else {
        quote! {
            #[allow(non_snake_case)]
            #vis use #inner_mod::#fn_name as #pascal_alias_ident;
        }
    };

    quote! {
        #[doc(hidden)]
        mod #internal_mod {
            use super::*;

            pub struct #props_name {
                #(#props_fields,)*
            }

            #[doc(hidden)]
            pub struct #builder_name {
                #(#builder_fields,)*
            }

            impl #props_name {
                pub fn builder() -> #builder_name {
                    #builder_name {
                        #(#builder_init,)*
                    }
                }
            }

            impl #builder_name {
                #(#setters)*

                pub fn build(self) -> #props_name {
                    #props_name {
                        #(#build_assignments,)*
                    }
                }
            }
        }

        #[doc(hidden)]
        #vis use #internal_mod::#props_name;

        #[doc(hidden)]
        mod #inner_mod {
            use super::*;
            #[doc(hidden)]
            #(#attrs)*
            pub fn #fn_name(props: #props_name) -> ::whisker::runtime::view::Element {
                #drop_unused
                let __handle = ::whisker::runtime::view::create_element_by_name(#tag_name);
                #(#apply_calls)*
                __handle
            }
        }

        #alias_emission
    }
}

struct Prop {
    ident: Ident,
    ty: Type,
}

/// `x_hello` / `x_input` → `XHello` / `XInput`. ASCII-only — native
/// element names should stay simple.
fn to_pascal_case(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut capitalize_next = true;
    for ch in snake.chars() {
        if ch == '_' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in ch.to_uppercase() {
                out.push(upper);
            }
            capitalize_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}
