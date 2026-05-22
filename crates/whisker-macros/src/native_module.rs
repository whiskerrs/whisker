//! `#[whisker::native_module]` — type-safe Rust proxy generator
//! for Whisker native modules.
//!
//! Input shape: a trait declaration listing one fn per method.
//! Each method's args + return are typed Rust; the macro wraps
//! them in [`WhiskerValue`] marshalling around a call to
//! `whisker::native_module::invoke` (sync) or `invoke_async`
//! (`async fn`).
//!
//! Output: a unit struct with the same name as the trait + an
//! `impl` block exposing each method as an associated function
//! returning `Result<T, WhiskerModuleError>`.
//!
//! The trait itself is consumed — there is no `impl<T> SomeTrait
//! for T` left over. Treating the input as a trait is purely
//! syntactic convenience (lets the user write familiar
//! `fn name(args)` declarations); the platform-side class is
//! the only true implementer of the module's API contract.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{
    parse2, FnArg, GenericArgument, ItemTrait, Lit, Meta, Pat, PatType, PathArguments, ReturnType,
    TraitItem, TraitItemFn, Type,
};

pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input: ItemTrait = match parse2(item.clone()) {
        Ok(t) => t,
        Err(e) => {
            return quote_spanned! { e.span() =>
                compile_error!(concat!(
                    "`#[whisker::native_module]` expects a trait declaration, e.g.\n",
                    "    #[whisker::native_module(name = \"MyStorage\")]\n",
                    "    pub trait MyStorage {\n",
                    "        fn save(key: String, value: String) -> bool;\n",
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
                "`#[whisker::native_module]` only supports `fn` items in the trait body",
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
            "`#[whisker::native_module]` accepts `name = \"...\"`",
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

    // Extract positional args, skipping `self` if present (we
    // accept but ignore it — the proxy is a unit struct).
    let mut arg_names: Vec<syn::Ident> = Vec::new();
    let mut arg_decls: Vec<TokenStream> = Vec::new();
    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => continue,
            FnArg::Typed(PatType { pat, ty, .. }) => {
                let Pat::Ident(ident_pat) = pat.as_ref() else {
                    return Err(syn::Error::new_spanned(
                        pat,
                        "native_module method args must be plain identifiers",
                    ));
                };
                let ident = ident_pat.ident.clone();
                arg_decls.push(quote! { #ident: #ty });
                arg_names.push(ident);
            }
        }
    }

    let return_type = match &sig.output {
        ReturnType::Default => syn::parse2::<Type>(quote!(()))?,
        ReturnType::Type(_, ty) => (**ty).clone(),
    };
    let unwrap_expr = emit_return_unwrap(&return_type)?;

    let invoke_path = if is_async {
        quote! { ::whisker::native_module::invoke_async }
    } else {
        quote! { ::whisker::native_module::invoke }
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
        pub #async_kw fn #name (#(#arg_decls),*)
            -> ::core::result::Result<#return_type, ::whisker::native_module::WhiskerModuleError>
        {
            let __args: ::std::vec::Vec<::whisker::native_module::WhiskerValue> =
                ::std::vec![ #( ::whisker::native_module::WhiskerValue::from(#arg_names) ),* ];
            let __result = #invoke_path (#module_name, #name_str, __args) #dot_await;
            #unwrap_expr
        }
    })
}

/// Build the match expression that unwraps a `WhiskerValue` into
/// the declared return type. Each supported type gets a
/// `match` arm; anything unrecognised falls through to
/// `Err(WhiskerModuleError::type_mismatch(_))`.
fn emit_return_unwrap(ty: &Type) -> syn::Result<TokenStream> {
    // The `__result` binding is the `WhiskerValue` produced by the
    // invoke call. The Error variant short-circuits to Err for
    // every return type.

    // Recognise the type by its surface syntax. `syn` doesn't
    // expose "is this type bool" directly — we match on the path's
    // last segment plus generic-args presence (for Option<T> /
    // Vec<u8> / WhiskerValue).
    if type_is_path(ty, &["bool"]) {
        return Ok(quote! {
            match __result {
                ::whisker::native_module::WhiskerValue::Bool(v) => ::core::result::Result::Ok(v),
                ::whisker::native_module::WhiskerValue::Error(msg) =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                __other =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(
                        ::std::format!("expected Bool, got {:?}", __other))),
            }
        });
    }

    if type_is_path(ty, &["i32"])
        || type_is_path(ty, &["i64"])
        || type_is_path(ty, &["u32"])
        || type_is_path(ty, &["u64"])
        || type_is_path(ty, &["isize"])
        || type_is_path(ty, &["usize"])
    {
        return Ok(quote! {
            match __result {
                ::whisker::native_module::WhiskerValue::Int(v) =>
                    ::core::result::Result::Ok(v as #ty),
                ::whisker::native_module::WhiskerValue::Error(msg) =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                __other =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(
                        ::std::format!("expected Int, got {:?}", __other))),
            }
        });
    }

    if type_is_path(ty, &["f32"]) || type_is_path(ty, &["f64"]) {
        return Ok(quote! {
            match __result {
                ::whisker::native_module::WhiskerValue::Float(v) =>
                    ::core::result::Result::Ok(v as #ty),
                ::whisker::native_module::WhiskerValue::Error(msg) =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                __other =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(
                        ::std::format!("expected Float, got {:?}", __other))),
            }
        });
    }

    if type_is_path(ty, &["String"]) || type_is_path(ty, &["std", "string", "String"]) {
        return Ok(quote! {
            match __result {
                ::whisker::native_module::WhiskerValue::String(v) =>
                    ::core::result::Result::Ok(v),
                ::whisker::native_module::WhiskerValue::Error(msg) =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                __other =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(
                        ::std::format!("expected String, got {:?}", __other))),
            }
        });
    }

    // Vec<u8> → Bytes
    if let Some(inner) = type_single_generic(ty, "Vec") {
        if type_is_path(inner, &["u8"]) {
            return Ok(quote! {
                match __result {
                    ::whisker::native_module::WhiskerValue::Bytes(v) =>
                        ::core::result::Result::Ok(v),
                    ::whisker::native_module::WhiskerValue::Error(msg) =>
                        ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                    __other =>
                        ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(
                            ::std::format!("expected Bytes, got {:?}", __other))),
                }
            });
        }
    }

    // Option<T>
    if let Some(inner) = type_single_generic(ty, "Option") {
        let inner_unwrap = emit_return_unwrap(inner)?;
        return Ok(quote! {
            match __result {
                ::whisker::native_module::WhiskerValue::Null =>
                    ::core::result::Result::Ok(::core::option::Option::None),
                ::whisker::native_module::WhiskerValue::Error(msg) =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                __result => {
                    let __inner: ::core::result::Result<#inner, ::whisker::native_module::WhiskerModuleError> = #inner_unwrap;
                    __inner.map(::core::option::Option::Some)
                }
            }
        });
    }

    // WhiskerValue passthrough
    if type_is_path(ty, &["WhiskerValue"])
        || type_is_path(ty, &["whisker", "native_module", "WhiskerValue"])
    {
        return Ok(quote! {
            match __result {
                ::whisker::native_module::WhiskerValue::Error(msg) =>
                    ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                __other => ::core::result::Result::Ok(__other),
            }
        });
    }

    // Unit return — accept anything (typically Null).
    if let Type::Tuple(t) = ty {
        if t.elems.is_empty() {
            return Ok(quote! {
                match __result {
                    ::whisker::native_module::WhiskerValue::Error(msg) =>
                        ::core::result::Result::Err(::whisker::native_module::WhiskerModuleError(msg)),
                    _ => ::core::result::Result::Ok(()),
                }
            });
        }
    }

    Err(syn::Error::new_spanned(
        ty,
        "unsupported return type — native_module v1 accepts \
         bool / i32 / i64 / u32 / u64 / f32 / f64 / String / Vec<u8> / Option<T> / WhiskerValue / ()",
    ))
}

/// Returns true when `ty` is `T1::T2::...::Tn` (a simple path
/// without generics) matching `segments`. The check ignores
/// leading `::` and any leading-module-path differences (e.g.
/// `String` vs `std::string::String` both work via separate
/// calls).
fn type_is_path(ty: &Type, segments: &[&str]) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    if tp.qself.is_some() {
        return false;
    }
    let path_segments: Vec<_> = tp
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    path_segments == segments
}

/// If `ty` is `Outer<Inner>` (single generic arg), returns
/// `&Inner`. Used for `Option<T>` / `Vec<u8>` recognition.
fn type_single_generic<'a>(ty: &'a Type, outer: &str) -> Option<&'a Type> {
    let Type::Path(tp) = ty else { return None };
    let last = tp.path.segments.last()?;
    if last.ident != outer {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}
