//! `#[component]` proc-macro implementation.
//!
//! Walks the user's function signature, extracts the parameter list,
//! and emits two items:
//!
//! 1. A `#[derive(::whisker::__typed_builder::TypedBuilder)]` `XxxProps` struct
//!    whose fields mirror the function parameters. Per-field
//!    `#[builder(...)]` annotations are derived from the parameter's
//!    type (string-ish primitives + `Option<T>` get `into` /
//!    `strip_option`, `Children` gets a default) and any `#[prop(...)]`
//!    attributes the user wrote.
//! 2. A rewritten `fn xxx(__props: XxxProps) -> ElementHandle { … }`
//!    that destructures the props back into local variables and runs
//!    the user's original body inside the existing
//!    `mount_component_remountable` hot-reload machinery.
//!
//! The function signature change (positional args → single Props arg)
//! deliberately breaks the old `xxx(arg1, arg2)` calling convention
//! (issue #18 Q4): user components must be invoked through `render!`'s
//! `xxx { kwarg: value }` syntax, which expands to
//! `xxx(XxxProps::builder().kwarg(value).build())`.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{parse2, Expr, FnArg, GenericParam, Ident, ItemFn, Pat, Type};

pub fn expand(item: TokenStream2) -> TokenStream2 {
    let input: ItemFn = match parse2(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };

    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let block = &input.block;
    let fn_name = &sig.ident;
    let output = &sig.output;
    let generics = &sig.generics;

    // Generic params split:
    //   `<T: Bound, U>` → `<T: Bound, U>` (impl) + `<T, U>` (ty)
    //
    // We use both: the function and Props struct carry the full
    // bounds in declaration position, and the turbofish on the
    // `as *const ()` cast inside the body needs the type-only form
    // so the fn pointer monomorphizes correctly.
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let ty_generics_for_turbofish = ty_generics_to_turbofish(generics);

    // Collect the names of the fn's generic *type* params so
    // `builder_annotation` can recognise a prop whose type is a bare
    // generic param (`value: T`) and skip `setter(into)` — `Into<T>`
    // with an unconstrained `T` blows up call-site inference (the
    // compiler can't pick a concrete `T` from `From<i32>` candidates).
    let generic_type_params: Vec<Ident> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t.ident.clone()),
            _ => None,
        })
        .collect();

    // Walk the parameter list. For each `Typed` arg, collect:
    //   - `ident` (the binding name, used for destructuring + capture)
    //   - `ty` (the parameter's type — used to decide builder annotations)
    //   - `attrs` (`#[prop(...)]` attributes the user wrote on the param)
    let mut props_fields: Vec<TokenStream2> = Vec::new();
    let mut prop_idents: Vec<Ident> = Vec::new();
    for arg in &sig.inputs {
        let pat_type = match arg {
            FnArg::Typed(t) => t,
            FnArg::Receiver(r) => {
                return syn::Error::new(
                    r.span(),
                    "#[component] does not support method receivers (`self` / `&self`)",
                )
                .to_compile_error();
            }
        };

        let ident = match &*pat_type.pat {
            Pat::Ident(pi) => pi.ident.clone(),
            other => {
                return syn::Error::new(
                    other.span(),
                    "#[component] parameters must be plain identifiers \
                     (no destructuring patterns)",
                )
                .to_compile_error();
            }
        };

        let prop_attr = match parse_prop_attr(&pat_type.attrs) {
            Ok(p) => p,
            Err(e) => return e.to_compile_error(),
        };

        let annotation = builder_annotation(&pat_type.ty, &prop_attr, &generic_type_params);
        let ty = &pat_type.ty;
        // Strip the param's `#[prop(...)]` attribute from the Props
        // field — it's a `#[component]` directive, not something the
        // emitted struct should carry forward.
        let other_attrs: Vec<&syn::Attribute> = pat_type
            .attrs
            .iter()
            .filter(|a| !a.path().is_ident("prop"))
            .collect();
        props_fields.push(quote! {
            #(#other_attrs)*
            #annotation
            pub #ident: #ty
        });
        prop_idents.push(ident);
    }

    let props_name = props_struct_name(fn_name);

    // Generate the destructure + capture + re-clone pattern that the
    // existing remount machinery expects:
    //
    //   let XxxProps { a, b, c } = __props;
    //   let __whisker_prop_a = a;
    //   let __whisker_prop_b = b;
    //   ...
    //   let __body = Box::new(move || {
    //       let a = Clone::clone(&__whisker_prop_a);
    //       let b = Clone::clone(&__whisker_prop_b);
    //       ...
    //       ::whisker::__hot::call(move || { #block })
    //   });
    //   mount_component_remountable(<fn> as *const (), __body)
    let captures: Vec<TokenStream2> = prop_idents
        .iter()
        .map(|i| {
            let cap = format_ident!("__whisker_prop_{}", i);
            quote! { let #cap = #i; }
        })
        .collect();
    let restores: Vec<TokenStream2> = prop_idents
        .iter()
        .map(|i| {
            let cap = format_ident!("__whisker_prop_{}", i);
            quote! { let #i = ::std::clone::Clone::clone(&#cap); }
        })
        .collect();

    // `<fn>` for the `as *const ()` cast inside the body.
    //
    // Non-generic case: bare `fn_name as *const ()` works — there's
    //   only one monomorphization, so the fn ptr is unambiguous.
    // Generic case: must turbofish: `fn_name::<T, U> as *const ()`
    //   so we cast the *current monomorphization*. Each `T` =
    //   different `*const ()` value, which is what we want — the
    //   per-component remount registry keys on it.
    let fn_ptr_expr = if ty_generics_for_turbofish.is_empty() {
        quote! { #fn_name as *const () }
    } else {
        quote! { #fn_name :: < #(#ty_generics_for_turbofish),* > as *const () }
    };

    // Props struct. Generics + where clause come straight from the
    // function — typed-builder threads them through to the generated
    // `XxxPropsBuilder<...>`.
    // The Props struct mirrors the user's fn visibility. A `pub fn`
    // produces `pub struct XxxProps` (so external callers can write
    // `render! { xxx { ... } }` and the macro-expansion can resolve
    // `XxxProps::builder()`). A bare `fn` keeps the struct module-
    // private, matching the original encapsulation.
    let props_struct = quote! {
        #[derive(::whisker::__typed_builder::TypedBuilder)]
        #vis struct #props_name #impl_generics #where_clause {
            #(#props_fields),*
        }
    };

    // Rewritten function: same signature except parameters are
    // collapsed into a single `__props: XxxProps<...>` arg. Body
    // destructures and runs the existing remount machinery.
    let new_fn = quote! {
        #(#attrs)*
        #vis fn #fn_name #impl_generics (
            __props: #props_name #ty_generics
        ) #output #where_clause {
            let #props_name { #(#prop_idents),* } = __props;
            #(#captures)*

            // Mirrors the pre-rewrite #[component] body shape — see
            // `crates/whisker-macros/src/lib.rs`'s history for the
            // rationale on the two-closure layering (outer keeps
            // re-clone bookkeeping out of the subsecond-dispatched
            // inner, which has to live at the user crate's source
            // position for hot reload to find it).
            let __body: ::std::boxed::Box<
                dyn ::std::ops::Fn() -> ::whisker::runtime::view::ElementHandle + 'static,
            > = ::std::boxed::Box::new(move || {
                #(#restores)*
                ::whisker::__hot::call(move || {
                    #block
                })
            });
            ::whisker::runtime::reactive::mount_component_remountable(
                #fn_ptr_expr,
                __body,
            )
        }
    };

    quote! {
        #props_struct
        #new_fn
    }
}

/// Information parsed from a `#[prop(...)]` attribute on a single
/// `#[component]` parameter.
#[derive(Default)]
struct PropAttr {
    /// `#[prop(default = expr)]` — emit `#[builder(default = expr)]`
    /// so callers may omit this prop and the builder fills in
    /// `expr` for them.
    default: Option<Expr>,
    /// `#[prop(optional)]` — equivalent to declaring the type as
    /// `Option<T>` from the caller's perspective. Emits
    /// `#[builder(default, setter(into, strip_option))]`. Currently
    /// the macro is conservative and the user is expected to write
    /// `Option<T>` directly; this attribute is reserved for future
    /// use (e.g., explicit opt-in to `strip_option` on a wrapper
    /// type the macro can't auto-detect).
    optional: bool,
}

fn parse_prop_attr(attrs: &[syn::Attribute]) -> syn::Result<PropAttr> {
    let mut out = PropAttr::default();
    for attr in attrs {
        if !attr.path().is_ident("prop") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                let value = meta.value()?;
                let expr: Expr = value.parse()?;
                out.default = Some(expr);
                Ok(())
            } else if meta.path.is_ident("optional") {
                out.optional = true;
                Ok(())
            } else {
                Err(meta.error(
                    "unknown `#[prop(...)]` setting; supported: \
                     `default = <expr>`, `optional`",
                ))
            }
        })?;
    }
    Ok(out)
}

/// Compute the `#[builder(...)]` annotation for one prop, given its
/// type and any `#[prop(...)]` directives. Decision table:
///
/// 1. `#[prop(default = expr)]` → `#[builder(default = expr)]` plus
///    the type-default annotation below (so callers can still omit
///    or supply the prop).
/// 2. `Children` (last path segment) → `#[builder(default = …empty
///    closure…)]` so a component can declare `children: Children`
///    without forcing every call site to pass one.
/// 3. `Option<T>` (last path segment) → `#[builder(default,
///    setter(into, strip_option))]`. Callers may omit the prop or
///    pass the inner `T` directly (typed-builder auto-wraps with
///    `Some`).
/// 4. Bare generic-type param (e.g. `value: T`) → `#[builder()]` (no
///    `setter(into)`). With unconstrained `T`, `Into<T>` inference
///    fails at the call site (multiple `From<...>` candidates).
/// 5. Otherwise → `#[builder(setter(into))]`. Required prop; `Into`
///    coercion lets `&str` flow into `String` props, `i32` into
///    `f64`, etc.
fn builder_annotation(ty: &Type, attr: &PropAttr, generic_type_params: &[Ident]) -> TokenStream2 {
    let is_generic = is_generic_type_param(ty, generic_type_params);
    let into_or_none = if is_generic {
        // Bare generic param — emit `setter()` without `into`.
        quote! {}
    } else {
        quote! { setter(into) }
    };
    if let Some(default_expr) = &attr.default {
        // User-supplied default. Pass `setter(into)` only when not
        // generic.
        if is_generic {
            return quote! { #[builder(default = #default_expr)] };
        }
        return quote! { #[builder(default = #default_expr, #into_or_none)] };
    }
    if attr.optional {
        if is_generic {
            return quote! { #[builder(default, setter(strip_option))] };
        }
        return quote! {
            #[builder(default, setter(into, strip_option))]
        };
    }
    if is_children_type(ty) {
        return quote! {
            #[builder(default = ::std::rc::Rc::new(
                || ::whisker::runtime::view::View::Empty
            ))]
        };
    }
    if is_option_type(ty) {
        // `Option<T>` where T is generic — still need strip_option,
        // but no `into` since the inner T isn't known.
        if is_option_of_generic(ty, generic_type_params) {
            return quote! {
                #[builder(default, setter(strip_option))]
            };
        }
        return quote! {
            #[builder(default, setter(into, strip_option))]
        };
    }
    if is_generic {
        // No annotation at all. typed-builder accepts an unannotated
        // field; emitting `#[builder()]` errors with "Expected
        // builder(…)".
        return quote! {};
    }
    quote! { #[builder(setter(into))] }
}

/// Is this type one of the fn's generic type parameters? Only
/// matches bare-ident path types (`T`, not `Option<T>` or
/// `Vec<T>`).
fn is_generic_type_param(ty: &Type, generic_type_params: &[Ident]) -> bool {
    if let Type::Path(tp) = ty {
        if tp.qself.is_none() && tp.path.segments.len() == 1 {
            let seg = &tp.path.segments[0];
            if seg.arguments.is_empty() {
                return generic_type_params.contains(&seg.ident);
            }
        }
    }
    false
}

/// Is this type `Option<T>` where the T is a generic type param?
fn is_option_of_generic(ty: &Type, generic_type_params: &[Ident]) -> bool {
    let Type::Path(tp) = ty else { return false };
    let Some(last) = tp.path.segments.last() else {
        return false;
    };
    if last.ident != "Option" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return false;
    };
    for arg in &args.args {
        if let syn::GenericArgument::Type(inner) = arg {
            if is_generic_type_param(inner, generic_type_params) {
                return true;
            }
        }
    }
    false
}

/// Heuristic: is this type `Option<T>` (in any of the path forms the
/// user might write — bare, `std::option::Option`, fully-qualified)?
fn is_option_type(ty: &Type) -> bool {
    last_path_ident(ty).map(|i| i == "Option").unwrap_or(false)
}

/// Heuristic: is this type `Children` (or a path ending in
/// `Children`)? The macro only matches the suffix because users may
/// alias the type or reach it through `whisker::Children`,
/// `whisker::runtime::view::Children`, etc.
fn is_children_type(ty: &Type) -> bool {
    last_path_ident(ty)
        .map(|i| i == "Children")
        .unwrap_or(false)
}

fn last_path_ident(ty: &Type) -> Option<Ident> {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().map(|s| s.ident.clone())
    } else {
        None
    }
}

/// `card` → `CardProps`. `my_component` → `MyComponentProps`.
fn props_struct_name(fn_name: &Ident) -> Ident {
    let snake = fn_name.to_string();
    let mut camel = String::with_capacity(snake.len() + 5);
    let mut upper_next = true;
    for c in snake.chars() {
        if c == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            camel.extend(c.to_uppercase());
            upper_next = false;
        } else {
            camel.push(c);
        }
    }
    camel.push_str("Props");
    Ident::new(&camel, fn_name.span())
}

/// Pull the type-parameter identifiers out of the function generics
/// so we can build a turbofish for the `as *const ()` cast. Lifetimes
/// and const generics are skipped (lifetimes aren't part of
/// turbofish; const generics aren't yet supported on
/// `#[component]`).
fn ty_generics_to_turbofish(generics: &syn::Generics) -> Vec<TokenStream2> {
    generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => {
                let name = &t.ident;
                Some(quote! { #name })
            }
            GenericParam::Lifetime(_) | GenericParam::Const(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn props_struct_name_pascal_case_conversion() {
        let id: Ident = parse_quote!(card);
        assert_eq!(props_struct_name(&id).to_string(), "CardProps");

        let id: Ident = parse_quote!(my_component);
        assert_eq!(props_struct_name(&id).to_string(), "MyComponentProps");

        let id: Ident = parse_quote!(tab_item);
        assert_eq!(props_struct_name(&id).to_string(), "TabItemProps");

        let id: Ident = parse_quote!(x);
        assert_eq!(props_struct_name(&id).to_string(), "XProps");
    }

    #[test]
    fn option_type_detected_across_path_shapes() {
        let bare: Type = parse_quote!(Option<String>);
        assert!(is_option_type(&bare));

        let std_path: Type = parse_quote!(std::option::Option<String>);
        assert!(is_option_type(&std_path));

        let fq_path: Type = parse_quote!(::std::option::Option<i32>);
        assert!(is_option_type(&fq_path));

        let not_option: Type = parse_quote!(String);
        assert!(!is_option_type(&not_option));

        let custom_with_option_substring: Type = parse_quote!(MyOptional);
        assert!(!is_option_type(&custom_with_option_substring));
    }

    #[test]
    fn children_type_detected_across_path_shapes() {
        let bare: Type = parse_quote!(Children);
        assert!(is_children_type(&bare));

        let qualified: Type = parse_quote!(whisker::Children);
        assert!(is_children_type(&qualified));

        let runtime_path: Type = parse_quote!(::whisker::runtime::view::Children);
        assert!(is_children_type(&runtime_path));

        let other: Type = parse_quote!(MyChildren);
        assert!(!is_children_type(&other));
    }

    #[test]
    fn ty_generics_turbofish_extracts_only_type_params() {
        let g: syn::Generics = parse_quote!(<'a, T: Clone, const N: usize>);
        let turbofish = ty_generics_to_turbofish(&g);
        assert_eq!(turbofish.len(), 1, "lifetime and const generic skipped");
        assert_eq!(turbofish[0].to_string(), "T");
    }

    #[test]
    fn parse_prop_default_attribute() {
        let attrs: Vec<syn::Attribute> = parse_quote! {
            #[prop(default = 42)]
        };
        let parsed = parse_prop_attr(&attrs).unwrap();
        assert!(parsed.default.is_some());
        assert!(!parsed.optional);
    }

    #[test]
    fn parse_prop_optional_attribute() {
        let attrs: Vec<syn::Attribute> = parse_quote! {
            #[prop(optional)]
        };
        let parsed = parse_prop_attr(&attrs).unwrap();
        assert!(parsed.optional);
        assert!(parsed.default.is_none());
    }

    #[test]
    fn parse_prop_unknown_key_errors() {
        let attrs: Vec<syn::Attribute> = parse_quote! {
            #[prop(unknown_setting = 1)]
        };
        match parse_prop_attr(&attrs) {
            Ok(_) => panic!("expected error for unknown prop setting"),
            Err(e) => assert!(e.to_string().contains("unknown")),
        }
    }

    /// Sanity check on the overall expansion shape — we don't try to
    /// re-parse the output (the function calls into runtime symbols
    /// the macro crate doesn't have access to), but we confirm the
    /// Props struct + rewritten fn both come out and the fn body has
    /// the destructure.
    #[test]
    fn expand_emits_props_struct_and_rewritten_fn() {
        let input: TokenStream2 = quote! {
            fn card(title: String) -> ElementHandle {
                render! { view { text { {title.clone()} } } }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct CardProps"));
        assert!(output.contains("fn card"));
        assert!(output.contains("__props : CardProps"));
        assert!(output.contains("CardProps { title }"));
    }

    #[test]
    fn expand_handles_no_param_component() {
        let input: TokenStream2 = quote! {
            fn header() -> ElementHandle {
                render! { view { text { "Hi" } } }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct HeaderProps"));
        assert!(output.contains("fn header"));
        // No-param destructure should be `HeaderProps { }` (just braces).
        assert!(
            output.contains("HeaderProps { }") || output.contains("HeaderProps {}"),
            "no-param destructure should be empty braces, got: {output}"
        );
    }

    #[test]
    fn expand_handles_option_param() {
        let input: TokenStream2 = quote! {
            fn x(label: Option<String>) -> ElementHandle {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct XProps"));
        assert!(
            output.contains("strip_option"),
            "Option<T> param should emit strip_option, got: {output}"
        );
    }

    #[test]
    fn expand_handles_generic_component() {
        let input: TokenStream2 = quote! {
            fn typed<T: Clone + 'static>(value: T) -> ElementHandle {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct TypedProps"));
        // Turbofish form of the fn ptr cast inside the body
        assert!(
            output.contains("typed :: < T >") || output.contains("typed::<T>"),
            "generic fn should use turbofish for fn-ptr cast, got: {output}"
        );
    }

    #[test]
    fn generic_param_skips_into_setter() {
        // Field whose type is a bare generic param must NOT get
        // `setter(into)`, otherwise `Into<T>` with unconstrained `T`
        // breaks call-site inference. Concrete fields keep `into`.
        let input: TokenStream2 = quote! {
            fn typed<T: Clone + 'static>(value: T, label: String) -> ElementHandle {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        // The String field still carries setter(into).
        assert!(
            output.contains("setter (into)") || output.contains("setter(into)"),
            "non-generic String field should keep setter(into); got: {output}"
        );
        // The generic field is annotated with `#[builder()]` only —
        // i.e. no `setter(into)` clause is attached to its declaration.
        // (Easier to assert this through the wider shape than a regex.)
        // We confirm by counting `setter (into)` occurrences: 1 (the
        // String field), and crucially the `value : T` field must be
        // preceded by a bare `#[builder ()]` annotation.
        let n = output.matches("setter (into)").count() + output.matches("setter(into)").count();
        assert_eq!(
            n, 1,
            "exactly one setter(into) expected (the non-generic field); got {n} in: {output}"
        );
    }

    #[test]
    fn option_of_generic_skips_into_setter() {
        // `Option<T>` where T is generic: still need strip_option,
        // but `into` would need a known target type → must be skipped.
        let input: TokenStream2 = quote! {
            fn typed<T: Clone + 'static>(value: Option<T>) -> ElementHandle {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(
            output.contains("strip_option"),
            "Option<T> must still strip_option; got: {output}"
        );
        let n = output.matches("into").count();
        // Should be zero `into` for the field-level setter. (We can
        // still match "into" inside other tokens, but typed-builder
        // setter clause uses the bare `into` ident, so the safer
        // assertion is that the field-level annotation does not have
        // `setter (into ,` or `setter(into,`.)
        assert!(
            !output.contains("setter (into ,") && !output.contains("setter(into,"),
            "Option<T> with generic T must not emit setter(into,...); got: {output}"
        );
        let _ = n;
    }

    #[test]
    fn is_generic_type_param_detects_bare_t() {
        let t_param: Ident = parse_quote!(T);
        let u_param: Ident = parse_quote!(U);
        let generics = vec![t_param, u_param];

        let bare_t: Type = parse_quote!(T);
        assert!(is_generic_type_param(&bare_t, &generics));

        let bare_u: Type = parse_quote!(U);
        assert!(is_generic_type_param(&bare_u, &generics));

        // `Option<T>` — the outer type is `Option`, not bare T.
        let opt_t: Type = parse_quote!(Option<T>);
        assert!(!is_generic_type_param(&opt_t, &generics));

        // Concrete type with same name does NOT exist by definition,
        // but a path that has multiple segments should not match.
        let path_t: Type = parse_quote!(crate::T);
        assert!(!is_generic_type_param(&path_t, &generics));

        // Non-generic type.
        let string_ty: Type = parse_quote!(String);
        assert!(!is_generic_type_param(&string_ty, &generics));
    }

    #[test]
    fn is_option_of_generic_detects_inner_t() {
        let t_param: Ident = parse_quote!(T);
        let generics = vec![t_param];

        let opt_t: Type = parse_quote!(Option<T>);
        assert!(is_option_of_generic(&opt_t, &generics));

        // Option<String> — inner is not a generic param.
        let opt_string: Type = parse_quote!(Option<String>);
        assert!(!is_option_of_generic(&opt_string, &generics));

        // Plain String is not an option at all.
        let plain: Type = parse_quote!(String);
        assert!(!is_option_of_generic(&plain, &generics));
    }
}
