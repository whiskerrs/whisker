//! Procedural macros for `whisker-router`.
//!
//! At present the crate exports a single attribute macro:
//!
//! ```ignore
//! use whisker_router::route;
//!
//! #[route]
//! #[derive(Clone, Debug, PartialEq)]
//! pub enum AppRoute {
//!     #[at("/")]
//!     Home,
//!     #[at("/profile/:id")]
//!     Profile { id: u64 },
//!     #[at("/settings")]
//!     Settings,
//! }
//! ```
//!
//! `#[route]` reads each variant's `#[at("…")]` pattern and generates
//! a `Route` impl whose `parse` / `to_path` are byte-for-byte equivalent
//! to the hand-written example in `whisker-router::route::tests`.
//!
//! Path patterns are split on `/`; segments starting with `:` bind
//! that variant's named field with the matching name (`:id` → field
//! `id`). The field's type must implement `FromStr` for `parse` and
//! `Display` for `to_path` — `u64`, `String`, custom enums all work.
//! Tuple-struct variants (`Profile(u64)`) aren't supported in v1;
//! switch to a named-field form (`Profile { id: u64 }`) instead.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, LitStr, Variant};

/// `#[route]` attribute on an enum — generates `impl Route for Enum`.
///
/// See the crate-level docs for the supported variant + path forms.
#[proc_macro_attribute]
pub fn route(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as DeriveInput);
    let enum_ident = input.ident.clone();

    let Data::Enum(data_enum) = &mut input.data else {
        return syn::Error::new_spanned(&input, "#[route] can only be applied to enums")
            .to_compile_error()
            .into();
    };

    let mut parse_arms = Vec::<TokenStream2>::new();
    let mut to_path_arms = Vec::<TokenStream2>::new();

    for variant in data_enum.variants.iter_mut() {
        let at = match read_at(variant) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        // `#[at(...)]` is consumed here — strip it from the variant
        // so the re-emitted enum doesn't carry an unrecognised
        // attribute (rustc would reject it; helper-attribute
        // registration only works on derive macros).
        variant.attrs.retain(|a| !a.path().is_ident("at"));

        let pattern_segments = split_path(&at.value());

        match build_arms(&enum_ident, variant, &pattern_segments, at.span()) {
            Ok((parse_arm, to_path_arm)) => {
                parse_arms.push(parse_arm);
                to_path_arms.push(to_path_arm);
            }
            Err(e) => return e.to_compile_error().into(),
        }
    }

    let expanded = quote! {
        #input

        impl ::whisker_router::route::Route for #enum_ident {
            fn parse(
                __path: &str,
            ) -> ::core::result::Result<Self, ::whisker_router::route::RouteError> {
                let __n = __path.trim_end_matches('/');
                let __n = __n.strip_prefix('/').unwrap_or(__n);
                let __segments: ::std::vec::Vec<&str> = if __n.is_empty() {
                    ::std::vec::Vec::new()
                } else {
                    __n.split('/').collect()
                };
                match __segments.as_slice() {
                    #(#parse_arms)*
                    _ => ::core::result::Result::Err(
                        ::whisker_router::route::RouteError::NoMatch(__path.to_string()),
                    ),
                }
            }

            fn to_path(&self) -> ::std::string::String {
                match self {
                    #(#to_path_arms)*
                }
            }
        }
    };
    expanded.into()
}

/// Pull the `#[at("…")]` literal off a variant. Variants without one
/// are rejected — the macro can't reasonably guess a path.
fn read_at(variant: &Variant) -> syn::Result<LitStr> {
    for attr in &variant.attrs {
        if attr.path().is_ident("at") {
            return attr.parse_args::<LitStr>();
        }
    }
    Err(syn::Error::new_spanned(
        variant,
        "every #[route] variant needs an #[at(\"…\")] attribute",
    ))
}

/// `/profile/:id/edit` → `["profile", ":id", "edit"]`.
/// Empty / "/" path → empty Vec (the "this is the index" case).
fn split_path(s: &str) -> Vec<String> {
    let n = s.trim_end_matches('/');
    let n = n.strip_prefix('/').unwrap_or(n);
    if n.is_empty() {
        Vec::new()
    } else {
        n.split('/').map(str::to_string).collect()
    }
}

/// Generate the two match arms (one for `parse`, one for `to_path`)
/// for a single variant.
fn build_arms(
    enum_ident: &syn::Ident,
    variant: &Variant,
    pattern: &[String],
    at_span: proc_macro2::Span,
) -> syn::Result<(TokenStream2, TokenStream2)> {
    let variant_ident = &variant.ident;

    // Collect param-name → static-segment information.
    // A "param" segment is `:foo`; everything else is a literal we
    // pattern-match on as `&str`.
    let mut params: Vec<String> = Vec::new();
    let mut slice_pattern: Vec<TokenStream2> = Vec::new();
    for seg in pattern {
        if let Some(name) = seg.strip_prefix(':') {
            let binding = format_ident!("__seg_{}", name);
            params.push(name.to_string());
            slice_pattern.push(quote! { #binding });
        } else {
            slice_pattern.push(quote! { #seg });
        }
    }

    match &variant.fields {
        Fields::Unit => {
            if !params.is_empty() {
                return Err(syn::Error::new(
                    at_span,
                    format!(
                        "variant {variant_ident} is a unit variant — the `#[at]` \
                         pattern has {} parameter(s) but the variant has no fields \
                         to bind them to",
                        params.len()
                    ),
                ));
            }
            let parse_arm = quote! {
                [#(#slice_pattern),*] => ::core::result::Result::Ok(
                    #enum_ident::#variant_ident
                ),
            };
            let to_path_lit = render_path(pattern, |p| format!("{{{p}}}"));
            // Unit variants have no field bindings, so render_path's
            // formatting is just the literal pattern (no params).
            debug_assert!(params.is_empty());
            let _ = to_path_lit;
            let to_path_arm = build_unit_to_path(pattern, enum_ident, variant_ident);
            Ok((parse_arm, to_path_arm))
        }
        Fields::Named(named) => {
            let field_names: Vec<&syn::Ident> = named
                .named
                .iter()
                .map(|f| f.ident.as_ref().expect("named field has ident"))
                .collect();

            // Every `:foo` must match a field; every field must be
            // covered by a `:foo`. (Order is the *field declaration*
            // order in the struct — params can appear in any order
            // inside the path.)
            for p in &params {
                if !field_names.iter().any(|f| *f == p) {
                    return Err(syn::Error::new(
                        at_span,
                        format!(
                            "path param `:{p}` doesn't match any field on \
                             variant {variant_ident}"
                        ),
                    ));
                }
            }
            for f in &field_names {
                let f_name = f.to_string();
                if !params.contains(&f_name) {
                    return Err(syn::Error::new(
                        at_span,
                        format!(
                            "field `{f_name}` on variant {variant_ident} isn't \
                             bound by the `#[at]` pattern — add `:{f_name}` to it"
                        ),
                    ));
                }
            }

            // parse arm
            let parse_bindings = field_names.iter().map(|f| {
                let seg_ident = format_ident!("__seg_{}", f);
                let f_str = f.to_string();
                quote! {
                    let #f = (*#seg_ident).parse().map_err(|_| {
                        ::whisker_router::route::RouteError::BadParam {
                            param: #f_str,
                            value: (*#seg_ident).to_string(),
                        }
                    })?;
                }
            });
            let parse_arm = quote! {
                [#(#slice_pattern),*] => {
                    #(#parse_bindings)*
                    ::core::result::Result::Ok(#enum_ident::#variant_ident { #(#field_names),* })
                }
            };

            // to_path arm
            let to_path_arm = build_named_to_path(pattern, enum_ident, variant_ident, &field_names);
            Ok((parse_arm, to_path_arm))
        }
        Fields::Unnamed(_) => Err(syn::Error::new_spanned(
            variant,
            "tuple-struct variants aren't supported by #[route] v1 — \
             use a named-field form like `Profile { id: u64 }`",
        )),
    }
}

/// Helper used by debug-formatting code paths inside `build_arms` —
/// kept generic enough to be reused if we ever want a non-rust
/// string output. For now only the param-named branch calls it.
fn render_path(pattern: &[String], on_param: impl Fn(&str) -> String) -> String {
    let mut out = String::from("/");
    for (i, seg) in pattern.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        match seg.strip_prefix(':') {
            Some(name) => out.push_str(&on_param(name)),
            None => out.push_str(seg),
        }
    }
    out
}

fn build_unit_to_path(
    pattern: &[String],
    enum_ident: &syn::Ident,
    variant_ident: &syn::Ident,
) -> TokenStream2 {
    let path = if pattern.is_empty() {
        "/".to_string()
    } else {
        let mut p = String::from("/");
        for (i, seg) in pattern.iter().enumerate() {
            if i > 0 {
                p.push('/');
            }
            p.push_str(seg);
        }
        p
    };
    quote! {
        #enum_ident::#variant_ident => #path.to_string(),
    }
}

fn build_named_to_path(
    pattern: &[String],
    enum_ident: &syn::Ident,
    variant_ident: &syn::Ident,
    fields: &[&syn::Ident],
) -> TokenStream2 {
    // Build a format! template — `:id` → `{id}`, literals stay as
    // themselves. The matching `{id}` is fed by the field bindings
    // unpacked in the match arm.
    let template = render_path(pattern, |p| format!("{{{p}}}"));
    quote! {
        #enum_ident::#variant_ident { #(#fields),* } => {
            ::std::format!(#template, #(#fields = #fields),*)
        }
    }
}
