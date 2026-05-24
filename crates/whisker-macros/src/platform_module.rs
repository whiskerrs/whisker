//! `#[whisker::platform_module]` — WhiskerValue-only `-sys` proxy
//! generator for Whisker native modules.
//!
//! ## Shape contract
//!
//! Input trait declares one fn per method, each with the
//! WhiskerValue-only `-sys` signature:
//!
//! ```ignore
//! #[whisker::platform_module(name = "WhiskerLocalStore")]
//! pub trait WhiskerLocalStoreSys {
//!     fn save(args: Vec<WhiskerValue>) -> WhiskerValue;
//!     fn load(args: Vec<WhiskerValue>) -> WhiskerValue;
//!     async fn fetch(args: Vec<WhiskerValue>) -> WhiskerValue;
//! }
//! ```
//!
//! Output: a unit struct + an `impl` block exposing each method
//! as an associated function that calls
//! `whisker::platform_module::invoke(module_name, method_name, args)`
//! and returns the raw [`WhiskerValue`] back to the caller. Error
//! propagation is the caller's job — the proxy doesn't lift
//! `WhiskerValue::Error` into `Result` here (the caller has full
//! context to decide how to surface a dispatch failure).
//!
//! ## Why "Sys" suffix on the trait
//!
//! The proc-macro-emitted proxy is the `-sys` layer — a thin
//! pass-through to the bridge with no type marshalling magic.
//! The `Sys` suffix mirrors Rust's `*-sys` crate convention
//! (`libc-sys`, `openssl-sys`, …) and the sibling
//! `whisker-driver-sys` crate that wraps the C ABI directly.
//! Typed call surfaces (`fn save(key: String, value: String) ->
//! Result<bool, _>`) live in a hand-written wrapper module
//! authors layer on top:
//!
//! ```ignore
//! pub struct WhiskerLocalStore;
//! impl WhiskerLocalStore {
//!     pub fn save(key: String, value: String) -> Result<bool, WhiskerModuleError> {
//!         let raw = WhiskerLocalStoreSys::save(vec![
//!             WhiskerValue::String(key),
//!             WhiskerValue::String(value),
//!         ]);
//!         match raw {
//!             WhiskerValue::Bool(b) => Ok(b),
//!             WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
//!             other => Err(WhiskerModuleError(format!("expected Bool, got {other:?}"))),
//!         }
//!     }
//! }
//! ```
//!
//! Module authors own that wrapper — it's where pure-Rust
//! ergonomics, validation, default values, etc. live. The macro
//! stays simple and predictable; the wrapper carries the
//! application-specific intent.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse2, FnArg, ItemTrait, Lit, Meta, Pat, PatType, ReturnType, TraitItem, TraitItemFn};

pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input: ItemTrait = match parse2(item.clone()) {
        Ok(t) => t,
        Err(e) => {
            return quote_spanned! { e.span() =>
                compile_error!(concat!(
                    "`#[whisker::platform_module]` expects a trait declaration, e.g.\n",
                    "    #[whisker::platform_module(name = \"MyStorage\")]\n",
                    "    pub trait MyStorageSys {\n",
                    "        fn save(args: Vec<WhiskerValue>) -> WhiskerValue;\n",
                    "    }\n",
                ));
            };
        }
    };

    let module_name = match parse_module_name(&attr, &input) {
        Ok(s) => s,
        Err(e) => return e.into_compile_error(),
    };

    let trait_name = &input.ident;
    let vis = &input.vis;
    let mut method_impls = Vec::with_capacity(input.items.len());

    for item in &input.items {
        let TraitItem::Fn(method) = item else {
            return syn::Error::new_spanned(
                item,
                "`#[whisker::platform_module]` only supports `fn` items in the trait body",
            )
            .into_compile_error();
        };
        match emit_method(&module_name, method) {
            Ok(tokens) => method_impls.push(tokens),
            Err(e) => return e.into_compile_error(),
        }
    }

    quote! {
        #vis struct #trait_name;

        impl #trait_name {
            #(#method_impls)*
        }
    }
}

fn parse_module_name(attr: &TokenStream, input: &ItemTrait) -> syn::Result<String> {
    if attr.is_empty() {
        return Ok(input.ident.to_string());
    }
    // Parse `name = "..."`. The only attr key supported in v1.
    let meta: Meta = parse2(attr.clone())?;
    let Meta::NameValue(nv) = meta else {
        return Err(syn::Error::new_spanned(
            meta,
            "`#[whisker::platform_module]` accepts `name = \"...\"`",
        ));
    };
    if !nv.path.is_ident("name") {
        return Err(syn::Error::new_spanned(
            &nv.path,
            "unexpected attribute key — only `name = \"...\"` is recognised",
        ));
    }
    let syn::Expr::Lit(lit) = &nv.value else {
        return Err(syn::Error::new_spanned(
            &nv.value,
            "`name = ...` value must be a string literal",
        ));
    };
    let Lit::Str(s) = &lit.lit else {
        return Err(syn::Error::new_spanned(
            &lit.lit,
            "`name = ...` value must be a string literal",
        ));
    };
    Ok(s.value())
}

fn emit_method(module_name: &str, method: &TraitItemFn) -> syn::Result<TokenStream> {
    let sig = &method.sig;
    let name = &sig.ident;
    let name_str = name.to_string();
    let is_async = sig.asyncness.is_some();

    // Collect arg names + types as-written. We pass the single
    // expected `args: Vec<WhiskerValue>` straight through to
    // `invoke` without inspection — Rust's type checker rejects
    // anything else when the trait body fails to typecheck.
    let mut arg_idents: Vec<&syn::Ident> = Vec::new();
    let mut arg_decls: Vec<TokenStream> = Vec::new();
    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => continue, // `self` is accepted but ignored
            FnArg::Typed(PatType { pat, ty, .. }) => {
                let Pat::Ident(ident_pat) = pat.as_ref() else {
                    return Err(syn::Error::new_spanned(
                        pat,
                        "platform_module method args must be plain identifiers",
                    ));
                };
                arg_decls.push(quote! { #ident_pat: #ty });
                arg_idents.push(&ident_pat.ident);
            }
        }
    }

    // The trait MUST declare exactly one positional arg — the
    // WhiskerValue vec. Zero args means the method takes no input
    // (still legal — we forward an empty vec). More than one
    // means the author drifted away from the -sys shape; flag
    // that explicitly rather than silently building a weird call.
    if arg_idents.len() > 1 {
        return Err(syn::Error::new_spanned(
            &sig.inputs,
            "platform_module methods take exactly one `args: Vec<WhiskerValue>` parameter \
             — type-safe wrappers belong in author-owned code on top of this proxy",
        ));
    }

    let args_expr = if arg_idents.len() == 1 {
        let id = arg_idents[0];
        quote! { #id }
    } else {
        quote! { ::std::vec::Vec::<::whisker::platform_module::WhiskerValue>::new() }
    };

    // Return type defaults to `WhiskerValue` if the user wrote
    // none. Anything other than `WhiskerValue` typechecks against
    // the macro-emitted body (a `WhiskerValue` literal) and
    // fails — surfacing the mismatch with the user's own return
    // type's span.
    let return_type: TokenStream = match &sig.output {
        ReturnType::Default => quote! { ::whisker::platform_module::WhiskerValue },
        ReturnType::Type(_, ty) => quote! { #ty },
    };

    let invoke_path = if is_async {
        quote! { ::whisker::platform_module::invoke_async }
    } else {
        quote! { ::whisker::platform_module::invoke }
    };
    let dot_await = if is_async {
        quote! { .await }
    } else {
        quote! {}
    };
    let async_kw = if is_async {
        quote! { async }
    } else {
        quote! {}
    };

    Ok(quote! {
        pub #async_kw fn #name (#(#arg_decls),*) -> #return_type {
            #invoke_path (#module_name, #name_str, #args_expr) #dot_await
        }
    })
}
