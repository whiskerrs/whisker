//! `#[component]` proc-macro implementation.
//!
//! Walks the user's function signature, extracts the parameter list,
//! and emits:
//!
//! 1. A `XxxProps` struct whose fields mirror the function parameters.
//! 2. A hand-rolled `XxxPropsBuilder` with one setter per parameter.
//!    Required fields panic at `.build()` if unset; `Option<T>` and
//!    `Children` props default to `None`/empty; `#[prop(default = …)]`
//!    fills in the user-supplied default.
//! 3. A rewritten `fn xxx(__props: XxxProps) -> Element { … }` that
//!    destructures the props back into local variables and runs the
//!    user's original body inside the existing
//!    `mount_component_remountable` hot-reload machinery.
//! 4. A PascalCase alias (`pub use … as XxxName`) over a private inner
//!    module — that's what render! calls and what RA surfaces in
//!    identifier completion.
//!
//! Why hand-roll the builder instead of `#[derive(TypedBuilder)]`:
//! typed-builder generates per-field type-state markers
//! (`<Name>PropsBuilder_Error_Missing_required_field_<field>` etc.)
//! that, while marked `pub`, are pulled into RA's auto-import
//! completion at the user's call site as noise even when nested in a
//! private module. The hand-rolled builder emits exactly two types:
//! the `Props` struct and one builder struct. Required-field validation
//! moves from compile-time (typed-builder's type-state) to runtime
//! (`.expect(...)` at `.build()`); the panic fires at component-mount
//! time with a clear "required field `xxx` was not set" message.
//!
//! The function signature change (positional args → single Props arg)
//! deliberately breaks the old `xxx(arg1, arg2)` calling convention:
//! user components must be invoked through `render!`'s
//! `XxxName(kwarg: value)` syntax, which expands to
//! `XxxName(XxxProps::builder().kwarg(value).build())`.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Expr, FnArg, GenericParam, Ident, ItemFn, Pat, Type, parse2};

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

    // Walk the parameter list. For each `Typed` arg, collect a
    // `Prop` (binding name + type + classification) so the emission
    // step can generate per-field tokens for the Props struct, the
    // Builder struct, the setters, and the `.build()` body.
    let mut props: Vec<Prop> = Vec::new();
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

        // Strip the param's `#[prop(...)]` attribute from forwarding
        // attrs — it's a `#[component]` directive, not something the
        // emitted Props struct should carry forward. Other attrs
        // (`#[allow(...)]`, doc comments) ride along on the Props
        // field unchanged.
        let other_attrs: Vec<syn::Attribute> = pat_type
            .attrs
            .iter()
            .filter(|a| !a.path().is_ident("prop"))
            .cloned()
            .collect();
        let kind = classify_prop(&pat_type.ty, &prop_attr, &generic_type_params);
        props.push(Prop {
            ident,
            ty: (*pat_type.ty).clone(),
            kind,
            forward_attrs: other_attrs,
        });
    }
    let prop_idents: Vec<Ident> = props.iter().map(|p| p.ident.clone()).collect();

    // ---- Generate per-field tokens for the Props struct + Builder ----
    let props_fields: Vec<TokenStream2> = props.iter().map(prop_struct_field).collect();
    let builder_fields: Vec<TokenStream2> = props.iter().map(prop_builder_field).collect();
    let builder_init: Vec<TokenStream2> = props.iter().map(prop_builder_init).collect();
    let setter_methods: Vec<TokenStream2> = props.iter().map(prop_setter_method).collect();
    let build_assignments: Vec<TokenStream2> = props.iter().map(prop_build_assignment).collect();

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

    // Force the subsecond-dispatched inner closure to capture EVERY
    // prop, not just the ones `#block` happens to reference. A `move`
    // closure captures each path it mentions by value, so touching
    // all props here pins the closure's environment layout to the
    // props signature alone. Without this, an edit that merely
    // *starts using* a previously-unused prop changes the capture
    // set — and a hot-patched body would then read the old (smaller)
    // environment through the new layout: garbage props / UB. With
    // it, the layout moves only when the props signature moves,
    // which is exactly what `__whisker_props_hash` below detects.
    let force_capture = if prop_idents.is_empty() {
        quote! {}
    } else {
        quote! {
            #[allow(unused, clippy::no_effect_underscore_binding)]
            let _ = ( #( &#prop_idents, )* );
        }
    };

    // Layout hash: FNV-1a over the props signature tokens (ident +
    // type, declaration order) — the compile-time part of what
    // determines the inner closure's captured-environment layout
    // given the forced capture above. The generated
    // `__whisker_props_hash` fn additionally folds in each prop
    // type's `size_of`/`align_of` AT ITS OWN BUILD's notion of the
    // type, so a change to a prop type's *definition* (fields added
    // to a struct the signature merely names) also shifts the value.
    // Read through subsecond dispatch at remount time, so the
    // runtime can compare "layout this site's stored closure was
    // built for" against "layout the freshly patched code expects"
    // and refuse the in-place remount on mismatch.
    let props_sig = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            let t = &p.ty;
            quote!(#i : #t).to_string()
        })
        .collect::<Vec<_>>()
        .join(", ");
    let props_hash = crate::fnv1a64(&props_sig);
    let prop_tys: Vec<syn::Type> = props.iter().map(|p| p.ty.clone()).collect();
    let props_hash_fn_expr = if ty_generics_for_turbofish.is_empty() {
        quote! { __whisker_props_hash }
    } else {
        quote! { __whisker_props_hash :: < #(#ty_generics_for_turbofish),* > }
    };

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

    // Props struct + hand-rolled Builder live inside a PRIVATE
    // module so the builder type's name doesn't surface as
    // identifier-completion noise at the user's call sites. Only
    // `XxxProps` is re-exported outward (doc-hidden); the builder
    // is reached via the `.builder()` method chain and never needs
    // to be in scope by name.
    //
    // Generics + where clause carry through from the user's fn so
    // a `#[component] fn xxx<T>(value: T)` gets a `XxxProps<T>` and
    // a `XxxPropsBuilder<T>`.
    let internal_mod = format_ident!("__{}_props_internal", fn_name);
    let builder_name = format_ident!("{}Builder", props_name);
    let props_struct = quote! {
        // No `#vis` on the module — visibility is deliberately
        // tighter than the surrounding fn so the builder type
        // stays unreachable as a bare identifier from outside.
        #[doc(hidden)]
        mod #internal_mod {
            // Pull in everything from the outer scope so prop types
            // referenced in fields (`Children`, user types, etc.)
            // resolve.
            use super::*;

            pub struct #props_name #impl_generics #where_clause {
                #(#props_fields),*
            }

            // Builder: every field becomes `Option<T>` (or, for
            // already-`Option<T>` props, `Option<Option<T>>`) so we
            // can tell "user hasn't set this" from "user set it to
            // None". `.build()` collapses each Option back into the
            // field's declared type, with the appropriate
            // default / panic-on-missing for required fields.
            //
            // The struct stays `pub` (with `#[doc(hidden)]`) because
            // a `pub` struct's `pub fn` methods are only callable
            // from outside if the struct itself is reachable. A
            // truly-private builder breaks `XxxProps::builder()
            // .setter(…).build()` from outside the mod — Rust's
            // method-resolution sees the methods as private when
            // the type is private, even though the value is held
            // by the caller. So the builder's name is necessarily
            // visible at the user's call site; `#[doc(hidden)]` is
            // the best signal we can give RA. Newer RA versions
            // honour it for auto-import filtering.
            #[doc(hidden)]
            pub struct #builder_name #impl_generics #where_clause {
                #(#builder_fields),*
            }

            impl #impl_generics #props_name #ty_generics #where_clause {
                /// Open a builder chain. `XxxProps::builder().a(…).b(…).build()`.
                pub fn builder() -> #builder_name #ty_generics {
                    #builder_name {
                        #(#builder_init),*
                    }
                }
            }

            impl #impl_generics #builder_name #ty_generics #where_clause {
                #(#setter_methods)*

                /// Materialise the Props. Required fields that the
                /// user didn't set fire a `required field `<name>` was
                /// not set` panic at mount time.
                pub fn build(self) -> #props_name #ty_generics {
                    #props_name {
                        #(#build_assignments),*
                    }
                }
            }
        }
        #[doc(hidden)]
        #vis use #internal_mod::#props_name;
    };

    // PascalCase alias is the user-facing call-site name. Visible
    // (NOT doc-hidden) because this is the canonical name in
    // render! syntax — surfacing it in completion is the whole
    // point. `non_snake_case` opt-out for the fn-as-PascalCase
    // alias.
    // `props_name` is `<PascalCase fn>Props`. Strip the suffix
    // exactly once to recover the alias name — `trim_end_matches`
    // is the wrong tool here because it greedily strips repeats
    // (`TwoPropsProps` → `Two`, dropping the user's actual name).
    let props_name_str = props_name.to_string();
    let alias_str = props_name_str
        .strip_suffix("Props")
        .unwrap_or(&props_name_str);
    let fn_name_str = fn_name.to_string();

    // Rewritten function: same signature except parameters are
    // collapsed into a single `__props: XxxProps<...>` arg. Body
    // destructures and runs the existing remount machinery.
    //
    // The fn lives inside a PRIVATE inner module so its
    // snake_case name (`art_tile`) doesn't pollute outer-scope
    // completion candidates. Only the PascalCase alias (declared
    // below) re-exports it outward. `#[doc(hidden)]` is
    // redundant given the private module, but kept as belt-and-
    // suspenders for tools that filter by doc-attribute.
    let inner_mod = format_ident!("__{}_inner", fn_name);
    let new_fn = quote! {
        #[doc(hidden)]
        mod #inner_mod {
            use super::*;

            // Props-layout hash (see the macro-side comment on
            // `props_sig`). Read through `__hot::call_hash` so a
            // just-applied patch answers with ITS layout. The
            // size_of/align_of folds evaluate in whichever build
            // this fn was compiled into — that's what lets a
            // prop-type *definition* change (same signature tokens,
            // different struct fields) shift the patch's value.
            #[doc(hidden)]
            pub fn __whisker_props_hash #impl_generics () -> u64 #where_clause {
                let mut __h: u64 = #props_hash;
                #(
                    __h = __h
                        .wrapping_mul(0x0000_0100_0000_01B3)
                        .wrapping_add(::std::mem::size_of::<#prop_tys>() as u64)
                        .wrapping_mul(0x0000_0100_0000_01B3)
                        .wrapping_add(::std::mem::align_of::<#prop_tys>() as u64);
                )*
                __h
            }

            #[doc(hidden)]
            #(#attrs)*
            pub fn #fn_name #impl_generics (
                __props: #props_name #ty_generics
            ) #output #where_clause {
                let #props_name { #(#prop_idents),* } = __props;
                #(#captures)*

                // Two-closure layering: the outer closure keeps the
                // re-clone bookkeeping out of the subsecond-dispatched
                // inner, which has to live at the user crate's source
                // position for hot reload to find it.
                let __body: ::std::boxed::Box<
                    dyn ::std::ops::Fn() -> ::whisker::runtime::view::Element + 'static,
                > = ::std::boxed::Box::new(move || {
                    #(#restores)*
                    ::whisker::__hot::call(move || {
                        #force_capture
                        #block
                    })
                });
                ::whisker::runtime::reactive::mount_component_remountable(
                    #fn_ptr_expr,
                    __body,
                    ::std::boxed::Box::new(|| {
                        ::whisker::__hot::call_hash(#props_hash_fn_expr)
                    }),
                )
            }
        }
    };

    // PascalCase alias re-exports the fn from the inner module.
    // This (and only this) is the user-facing call-site name.
    //
    // `#[component]` is expected at module level (not nested
    // inside fn bodies) — both because `pub use` only works at
    // module level and because components benefit from being
    // visible to their crate's `render!` callers.
    // Alongside the callable alias (value namespace), expose the same
    // PascalCase name as a TYPE alias to the Props struct (type
    // namespace). The two coexist because Rust keeps separate value and
    // type namespaces, and a single `use crate::Icon` imports the name
    // from *both* — so `render!` can lower a call to
    // `Icon(Icon::builder()…build())` using only the component name, and
    // users never have to import `IconProps` separately.
    let pascal_alias = if alias_str == fn_name_str {
        quote! {
            #[doc(hidden)]
            #vis use #inner_mod::#fn_name;
            #[doc(hidden)]
            #[allow(non_camel_case_types, type_alias_bounds)]
            #vis type #fn_name #impl_generics = #props_name #ty_generics;
        }
    } else {
        let alias_ident = format_ident!("{}", alias_str);
        quote! {
            #[allow(non_snake_case)]
            #vis use #inner_mod::#fn_name as #alias_ident;
            #[doc(hidden)]
            #[allow(type_alias_bounds)]
            #vis type #alias_ident #impl_generics = #props_name #ty_generics;
        }
    };

    quote! {
        #props_struct
        #new_fn
        #pascal_alias
    }
}

/// Information parsed from a `#[prop(...)]` attribute on a single
/// `#[component]` parameter.
#[derive(Default, Clone)]
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

/// One parsed `#[component]` parameter, ready to be turned into
/// Props-field, Builder-field, setter, and build()-body tokens.
struct Prop {
    /// User's parameter name. Used verbatim everywhere (field name,
    /// setter name, build()-assignment LHS, fn-body destructure ident).
    ident: Ident,
    /// User-written type — kept exactly as the user wrote it so the
    /// emitted Props struct preserves their lifetime / generic
    /// references. Builder field / setter signature are derived from
    /// this + `kind`.
    ty: Type,
    /// Per-prop emission strategy. See `PropKind` for the decision
    /// table.
    kind: PropKind,
    /// Non-`#[prop]` attributes the user wrote on this parameter
    /// (`#[allow(...)]`, doc comments). Forwarded onto the Props
    /// struct field.
    forward_attrs: Vec<syn::Attribute>,
}

/// How one Prop is wired through Props / Builder / setter / build.
enum PropKind {
    /// Required, has a concrete enough type for `Into<T>` coercion.
    /// Setter: `pub fn x(self, v: impl Into<T>) -> Self`.
    /// Build:  `self.x.expect("required field `x` was not set")`.
    Required,
    /// Required, but the type is a bare generic param (`value: T`).
    /// Setter accepts `T` directly — `Into<T>` with unconstrained
    /// `T` blows up at the call site.
    /// Setter: `pub fn x(self, v: T) -> Self`.
    /// Build:  `self.x.expect(...)`.
    RequiredGeneric,
    /// `Option<U>` prop. Builder stores `Option<Option<U>>` so we
    /// can tell "user didn't set it" (outer None) apart from "user
    /// set it to None" (outer Some, inner None — currently
    /// unreachable in render! but kept for direct construction).
    /// Setter takes the inner `U` (or `impl Into<U>` when U isn't
    /// generic) and wraps to `Some(Some(...))`. Build collapses
    /// the outer `Option` with `.unwrap_or(None)` so missing props
    /// become None.
    Optional {
        /// The inner `U` extracted from `Option<U>`.
        inner: Type,
        /// `true` when `U` is a bare generic param — drops the
        /// `Into<…>` on the setter (same reason as `RequiredGeneric`).
        inner_is_generic: bool,
    },
    /// `Children` prop. Builder field is `Option<Children>`. Setter
    /// takes `Children` directly (the type is already a wrapped
    /// `Rc<dyn Fn>` — there's no useful `Into` story). Build
    /// defaults a missing children prop to a closure returning
    /// `View::Empty`, matching the typed-builder behaviour from
    /// before.
    Children,
    /// `#[prop(default = expr)]`. Behaves like Required for the
    /// setter and like `unwrap_or_else(|| expr)` at build time.
    /// The expr is held in `default` rather than inlined into the
    /// kind variant so the variant stays Copy-ish.
    Default {
        default: Expr,
        /// Whether the type is a bare generic param (controls
        /// `Into<T>` on the setter, same as for Required).
        is_generic: bool,
    },
}

/// Decide the [`PropKind`] for a given type + user `#[prop(...)]`
/// directive. The precedence (mirrors what `typed_builder` did for us
/// before):
///
/// 1. `#[prop(default = expr)]` wins regardless of type.
/// 2. `#[prop(optional)]` on a non-`Option<T>` type → upgrade to
///    `Optional { inner: T, ... }` so the user can omit. (Currently
///    user code uses `Option<T>` directly; this branch is reserved
///    for future opt-in.)
/// 3. `Children` (last path segment) → `Children` kind.
/// 4. `Option<U>` (last path segment) → `Optional { inner: U, ... }`.
/// 5. Bare generic param → `RequiredGeneric`.
/// 6. Otherwise → `Required`.
fn classify_prop(ty: &Type, attr: &PropAttr, generic_type_params: &[Ident]) -> PropKind {
    let is_generic = is_generic_type_param(ty, generic_type_params);
    if let Some(default_expr) = attr.default.clone() {
        return PropKind::Default {
            default: default_expr,
            is_generic,
        };
    }
    if attr.optional {
        if let Some(inner) = option_inner_type(ty).cloned() {
            let inner_is_generic = is_generic_type_param(&inner, generic_type_params);
            return PropKind::Optional {
                inner,
                inner_is_generic,
            };
        }
        // `#[prop(optional)]` on a non-`Option<T>` — wrap the user's
        // type into an Optional with the same inner so the
        // setter/build still typecheck. typed-builder's
        // `strip_option` did the same.
        return PropKind::Optional {
            inner: ty.clone(),
            inner_is_generic: is_generic,
        };
    }
    if is_children_type(ty) {
        return PropKind::Children;
    }
    if let Some(inner) = option_inner_type(ty).cloned() {
        let inner_is_generic = is_generic_type_param(&inner, generic_type_params);
        return PropKind::Optional {
            inner,
            inner_is_generic,
        };
    }
    if is_generic {
        return PropKind::RequiredGeneric;
    }
    PropKind::Required
}

/// One field in the public `XxxProps` struct. Types stay exactly as
/// the user wrote them — Props is the user-visible struct.
fn prop_struct_field(prop: &Prop) -> TokenStream2 {
    let ident = &prop.ident;
    let ty = &prop.ty;
    let attrs = &prop.forward_attrs;
    quote! {
        #(#attrs)*
        pub #ident: #ty
    }
}

/// One field in the internal builder struct. Every field becomes an
/// `Option<…>` so we can distinguish "set" from "not set"; `Option<T>`
/// props become `Option<Option<T>>` (outer Option = builder presence,
/// inner = the user's Option semantics).
fn prop_builder_field(prop: &Prop) -> TokenStream2 {
    let ident = &prop.ident;
    let ty = &prop.ty;
    match &prop.kind {
        PropKind::Required | PropKind::RequiredGeneric | PropKind::Children => {
            quote! { #ident: ::std::option::Option<#ty> }
        }
        PropKind::Optional { inner, .. } => {
            quote! { #ident: ::std::option::Option<::std::option::Option<#inner>> }
        }
        PropKind::Default { .. } => {
            quote! { #ident: ::std::option::Option<#ty> }
        }
    }
}

/// `field: None` literal in the builder constructor (`Props::builder()`).
fn prop_builder_init(prop: &Prop) -> TokenStream2 {
    let ident = &prop.ident;
    quote! { #ident: ::std::option::Option::None }
}

/// The setter method emitted on the builder. Signature depends on
/// the prop kind — see PropKind doc for the exact rules.
fn prop_setter_method(prop: &Prop) -> TokenStream2 {
    let ident = &prop.ident;
    let ty = &prop.ty;
    match &prop.kind {
        PropKind::Required => quote! {
            #[allow(unused_mut)]
            pub fn #ident(mut self, value: impl ::std::convert::Into<#ty>) -> Self {
                self.#ident = ::std::option::Option::Some(value.into());
                self
            }
        },
        PropKind::RequiredGeneric => quote! {
            #[allow(unused_mut)]
            pub fn #ident(mut self, value: #ty) -> Self {
                self.#ident = ::std::option::Option::Some(value);
                self
            }
        },
        PropKind::Optional {
            inner,
            inner_is_generic,
        } => {
            // Setter takes the inner (unwrapped) T — same surface
            // typed-builder's `strip_option` gave us. Stored as
            // `Some(Some(v))` to record both "set" and "set to a
            // Some-value".
            if *inner_is_generic {
                quote! {
                    #[allow(unused_mut)]
                    pub fn #ident(mut self, value: #inner) -> Self {
                        self.#ident = ::std::option::Option::Some(
                            ::std::option::Option::Some(value)
                        );
                        self
                    }
                }
            } else {
                quote! {
                    #[allow(unused_mut)]
                    pub fn #ident(mut self, value: impl ::std::convert::Into<#inner>) -> Self {
                        self.#ident = ::std::option::Option::Some(
                            ::std::option::Option::Some(value.into())
                        );
                        self
                    }
                }
            }
        }
        PropKind::Children => quote! {
            #[allow(unused_mut)]
            pub fn #ident(mut self, value: #ty) -> Self {
                self.#ident = ::std::option::Option::Some(value);
                self
            }
        },
        PropKind::Default { is_generic, .. } => {
            if *is_generic {
                quote! {
                    #[allow(unused_mut)]
                    pub fn #ident(mut self, value: #ty) -> Self {
                        self.#ident = ::std::option::Option::Some(value);
                        self
                    }
                }
            } else {
                quote! {
                    #[allow(unused_mut)]
                    pub fn #ident(mut self, value: impl ::std::convert::Into<#ty>) -> Self {
                        self.#ident = ::std::option::Option::Some(value.into());
                        self
                    }
                }
            }
        }
    }
}

/// One `field: <unwrap-expression>` line inside `.build()`'s
/// `XxxProps { … }` construction.
fn prop_build_assignment(prop: &Prop) -> TokenStream2 {
    let ident = &prop.ident;
    let missing_msg = format!("required field `{ident}` was not set");
    match &prop.kind {
        PropKind::Required | PropKind::RequiredGeneric => quote! {
            #ident: self.#ident.expect(#missing_msg)
        },
        PropKind::Optional { .. } => quote! {
            // Outer Option = "did the user call .ident(…)?"
            // — collapse missing → None so the public `Option<T>`
            // field is the user's chosen value or None.
            #ident: self.#ident.unwrap_or(::std::option::Option::None)
        },
        PropKind::Children => quote! {
            #ident: self.#ident.unwrap_or_else(|| {
                ::std::rc::Rc::new(|| ::whisker::runtime::view::View::Empty)
            })
        },
        PropKind::Default { default, .. } => {
            quote! {
                #ident: self.#ident.unwrap_or_else(|| #default)
            }
        }
    }
}

/// Extract `U` from `Option<U>` (in any of the path forms the user
/// might write — bare, `std::option::Option`, fully-qualified).
/// Returns `None` if the type isn't an `Option`.
fn option_inner_type(ty: &Type) -> Option<&Type> {
    let Type::Path(tp) = ty else { return None };
    let last = tp.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    for arg in &args.args {
        if let syn::GenericArgument::Type(inner) = arg {
            return Some(inner);
        }
    }
    None
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

    // -- Naming + helpers ---------------------------------------------------

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
    fn option_inner_type_unwraps_across_path_shapes() {
        let bare: Type = parse_quote!(Option<String>);
        let inner = option_inner_type(&bare).unwrap();
        assert!(matches!(inner, Type::Path(_)));

        let std_path: Type = parse_quote!(std::option::Option<String>);
        assert!(option_inner_type(&std_path).is_some());

        let fq_path: Type = parse_quote!(::std::option::Option<i32>);
        assert!(option_inner_type(&fq_path).is_some());

        let not_option: Type = parse_quote!(String);
        assert!(option_inner_type(&not_option).is_none());

        // `MyOptional` ends in `al` not `Option`. Not an Option.
        let custom: Type = parse_quote!(MyOptional);
        assert!(option_inner_type(&custom).is_none());

        // `Option` without generic args → not a valid Option<T> for us.
        let bare_option: Type = parse_quote!(Option);
        assert!(option_inner_type(&bare_option).is_none());

        // Non-path type (tuple).
        let tup: Type = parse_quote!((u8, u8));
        assert!(option_inner_type(&tup).is_none());
    }

    #[test]
    fn ty_generics_turbofish_extracts_only_type_params() {
        let g: syn::Generics = parse_quote!(<'a, T: Clone, const N: usize>);
        let turbofish = ty_generics_to_turbofish(&g);
        assert_eq!(turbofish.len(), 1, "lifetime and const generic skipped");
        assert_eq!(turbofish[0].to_string(), "T");
    }

    #[test]
    fn is_generic_type_param_detects_bare_t() {
        let t_param: Ident = parse_quote!(T);
        let u_param: Ident = parse_quote!(U);
        let generics = vec![t_param, u_param];

        assert!(is_generic_type_param(&parse_quote!(T), &generics));
        assert!(is_generic_type_param(&parse_quote!(U), &generics));

        // `Option<T>` — the outer type is `Option`, not bare T.
        assert!(!is_generic_type_param(&parse_quote!(Option<T>), &generics));
        // Multi-segment path with same final ident does NOT match.
        assert!(!is_generic_type_param(&parse_quote!(crate::T), &generics));
        // Concrete non-generic type.
        assert!(!is_generic_type_param(&parse_quote!(String), &generics));
        // Generic-shaped (T<X>) doesn't match.
        let t_with_args: Type = parse_quote!(T<i32>);
        assert!(!is_generic_type_param(&t_with_args, &generics));
        // Non-path type (reference) doesn't match.
        let reference: Type = parse_quote!(&'a str);
        assert!(!is_generic_type_param(&reference, &generics));
    }

    // -- #[prop(...)] attribute parser -------------------------------------

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

    #[test]
    fn parse_prop_ignores_other_attrs() {
        // #[allow(...)] etc. must not interfere with #[prop(...)] parsing.
        let attrs: Vec<syn::Attribute> = parse_quote! {
            #[allow(dead_code)]
            #[doc = "ignored"]
        };
        let parsed = parse_prop_attr(&attrs).unwrap();
        assert!(parsed.default.is_none());
        assert!(!parsed.optional);
    }

    // -- classify_prop decision table --------------------------------------

    fn classify(ty: Type, attr: PropAttr, generics: &[Ident]) -> PropKind {
        classify_prop(&ty, &attr, generics)
    }

    #[test]
    fn classify_required_for_plain_type() {
        let k = classify(parse_quote!(String), PropAttr::default(), &[]);
        assert!(matches!(k, PropKind::Required));
    }

    #[test]
    fn classify_required_generic_for_bare_t() {
        let generics = vec![parse_quote!(T)];
        let k = classify(parse_quote!(T), PropAttr::default(), &generics);
        assert!(matches!(k, PropKind::RequiredGeneric));
    }

    #[test]
    fn classify_optional_for_option_of_concrete() {
        let k = classify(parse_quote!(Option<String>), PropAttr::default(), &[]);
        match k {
            PropKind::Optional {
                inner_is_generic, ..
            } => assert!(!inner_is_generic, "concrete inner shouldn't be generic"),
            other => panic!(
                "expected Optional, got {other:?}",
                other = kind_name(&other)
            ),
        }
    }

    #[test]
    fn classify_optional_for_option_of_generic() {
        let generics = vec![parse_quote!(T)];
        let k = classify(parse_quote!(Option<T>), PropAttr::default(), &generics);
        match k {
            PropKind::Optional {
                inner_is_generic, ..
            } => assert!(
                inner_is_generic,
                "Option<T> inner T must be flagged generic"
            ),
            other => panic!(
                "expected Optional, got {other:?}",
                other = kind_name(&other)
            ),
        }
    }

    #[test]
    fn classify_children_for_children_type() {
        let k = classify(parse_quote!(Children), PropAttr::default(), &[]);
        assert!(matches!(k, PropKind::Children));
        // Qualified path also matches.
        let k = classify(parse_quote!(whisker::Children), PropAttr::default(), &[]);
        assert!(matches!(k, PropKind::Children));
    }

    #[test]
    fn classify_default_wins_over_other_kinds() {
        // #[prop(default = …)] takes precedence — even when the type
        // is Option<T> or Children.
        let attr = PropAttr {
            default: Some(parse_quote!(42)),
            ..PropAttr::default()
        };
        let k = classify(parse_quote!(Option<i32>), attr.clone(), &[]);
        assert!(matches!(
            k,
            PropKind::Default {
                is_generic: false,
                ..
            }
        ));

        let k = classify(parse_quote!(Children), attr, &[]);
        assert!(matches!(
            k,
            PropKind::Default {
                is_generic: false,
                ..
            }
        ));
    }

    #[test]
    fn classify_default_with_generic_t() {
        let generics = vec![parse_quote!(T)];
        let attr = PropAttr {
            default: Some(parse_quote!(Default::default())),
            ..PropAttr::default()
        };
        let k = classify(parse_quote!(T), attr, &generics);
        assert!(matches!(
            k,
            PropKind::Default {
                is_generic: true,
                ..
            }
        ));
    }

    #[test]
    fn classify_optional_attribute_wraps_non_option_type() {
        // `#[prop(optional)]` on `String` should treat the field as
        // Optional<String>.
        let attr = PropAttr {
            optional: true,
            ..PropAttr::default()
        };
        let k = classify(parse_quote!(String), attr, &[]);
        match k {
            PropKind::Optional {
                inner_is_generic, ..
            } => assert!(!inner_is_generic),
            other => panic!(
                "expected Optional, got {other:?}",
                other = kind_name(&other)
            ),
        }
    }

    #[test]
    fn classify_optional_attribute_on_option_uses_inner() {
        // `#[prop(optional)]` on `Option<String>` extracts the inner
        // String — same as a plain Option<String>.
        let attr = PropAttr {
            optional: true,
            ..PropAttr::default()
        };
        let k = classify(parse_quote!(Option<String>), attr, &[]);
        assert!(matches!(k, PropKind::Optional { .. }));
    }

    // -- Per-prop emission helpers (struct field / builder field / setter / build assignment) -----

    fn make_prop(ident: &str, ty: Type, kind: PropKind) -> Prop {
        Prop {
            ident: format_ident!("{}", ident),
            ty,
            kind,
            forward_attrs: vec![],
        }
    }

    #[test]
    fn prop_struct_field_keeps_user_type() {
        let p = make_prop("label", parse_quote!(String), PropKind::Required);
        let out = prop_struct_field(&p).to_string();
        assert!(
            out.contains("pub label : String"),
            "Props field uses the user's type verbatim; got: {out}"
        );
    }

    #[test]
    fn prop_struct_field_forwards_attrs() {
        let attrs: Vec<syn::Attribute> = parse_quote! {
            #[doc = "user doc"]
            #[allow(dead_code)]
        };
        let p = Prop {
            ident: format_ident!("label"),
            ty: parse_quote!(String),
            kind: PropKind::Required,
            forward_attrs: attrs,
        };
        let out = prop_struct_field(&p).to_string();
        assert!(out.contains("doc = \"user doc\""));
        assert!(out.contains("allow (dead_code)"));
    }

    #[test]
    fn prop_builder_field_wraps_required_in_option() {
        let p = make_prop("a", parse_quote!(String), PropKind::Required);
        let out = prop_builder_field(&p).to_string();
        assert!(out.contains("a : :: std :: option :: Option < String >"));
    }

    #[test]
    fn prop_builder_field_double_wraps_optional() {
        let p = make_prop(
            "b",
            parse_quote!(Option<String>),
            PropKind::Optional {
                inner: parse_quote!(String),
                inner_is_generic: false,
            },
        );
        let out = prop_builder_field(&p).to_string();
        // tokenstream display can collapse `> >` into `>>`. Accept both.
        let normalized = out.replace(" >>", " > >");
        assert!(
            normalized
                .contains(":: std :: option :: Option < :: std :: option :: Option < String > >"),
            "Option<T> prop should be Option<Option<T>> in builder; got: {out}"
        );
    }

    #[test]
    fn prop_builder_field_default_uses_outer_type() {
        let p = make_prop(
            "c",
            parse_quote!(i32),
            PropKind::Default {
                default: parse_quote!(0),
                is_generic: false,
            },
        );
        let out = prop_builder_field(&p).to_string();
        assert!(out.contains("c : :: std :: option :: Option < i32 >"));
    }

    #[test]
    fn prop_setter_required_uses_impl_into() {
        let p = make_prop("a", parse_quote!(String), PropKind::Required);
        let out = prop_setter_method(&p).to_string();
        assert!(out.contains("pub fn a"));
        assert!(out.contains("impl :: std :: convert :: Into < String >"));
        assert!(out.contains("self . a = :: std :: option :: Option :: Some (value . into ())"));
    }

    #[test]
    fn prop_setter_required_generic_takes_t_directly() {
        let p = make_prop("v", parse_quote!(T), PropKind::RequiredGeneric);
        let out = prop_setter_method(&p).to_string();
        assert!(out.contains("pub fn v (mut self , value : T)"));
        // No `impl Into<T>` on generic — would break inference.
        assert!(!out.contains("Into < T >"));
    }

    #[test]
    fn prop_setter_optional_strips_outer_option() {
        let p = make_prop(
            "alt",
            parse_quote!(Option<String>),
            PropKind::Optional {
                inner: parse_quote!(String),
                inner_is_generic: false,
            },
        );
        let out = prop_setter_method(&p).to_string();
        // Setter takes the inner String (not Option<String>).
        assert!(out.contains("impl :: std :: convert :: Into < String >"));
        // Stored as Some(Some(v.into())) — double wrap.
        assert!(out.contains("Option :: Some (:: std :: option :: Option :: Some"));
    }

    #[test]
    fn prop_setter_optional_with_generic_inner_skips_into() {
        let p = make_prop(
            "alt",
            parse_quote!(Option<T>),
            PropKind::Optional {
                inner: parse_quote!(T),
                inner_is_generic: true,
            },
        );
        let out = prop_setter_method(&p).to_string();
        assert!(out.contains("value : T"));
        assert!(!out.contains("Into < T >"));
    }

    #[test]
    fn prop_setter_children_takes_value_directly() {
        let p = make_prop("children", parse_quote!(Children), PropKind::Children);
        let out = prop_setter_method(&p).to_string();
        assert!(out.contains("value : Children"));
        // No `Into` — `Children` is already a wrapper type.
        assert!(!out.contains("Into <"));
    }

    #[test]
    fn prop_setter_default_uses_impl_into_for_concrete() {
        let p = make_prop(
            "count",
            parse_quote!(i32),
            PropKind::Default {
                default: parse_quote!(5),
                is_generic: false,
            },
        );
        let out = prop_setter_method(&p).to_string();
        assert!(out.contains("impl :: std :: convert :: Into < i32 >"));
    }

    #[test]
    fn prop_setter_default_with_generic_takes_t_directly() {
        let p = make_prop(
            "v",
            parse_quote!(T),
            PropKind::Default {
                default: parse_quote!(Default::default()),
                is_generic: true,
            },
        );
        let out = prop_setter_method(&p).to_string();
        assert!(out.contains("value : T"));
        assert!(!out.contains("Into < T >"));
    }

    #[test]
    fn prop_build_assignment_required_expects() {
        let p = make_prop("a", parse_quote!(String), PropKind::Required);
        let out = prop_build_assignment(&p).to_string();
        assert!(out.contains(". expect ("));
        assert!(out.contains("\"required field `a` was not set\""));
    }

    #[test]
    fn prop_build_assignment_required_generic_expects() {
        let p = make_prop("v", parse_quote!(T), PropKind::RequiredGeneric);
        let out = prop_build_assignment(&p).to_string();
        assert!(out.contains("\"required field `v` was not set\""));
    }

    #[test]
    fn prop_build_assignment_optional_defaults_to_none() {
        let p = make_prop(
            "alt",
            parse_quote!(Option<String>),
            PropKind::Optional {
                inner: parse_quote!(String),
                inner_is_generic: false,
            },
        );
        let out = prop_build_assignment(&p).to_string();
        // unwrap_or(None) — missing prop becomes None.
        assert!(out.contains("unwrap_or"));
        assert!(out.contains("Option :: None"));
    }

    #[test]
    fn prop_build_assignment_children_defaults_to_empty_closure() {
        let p = make_prop("children", parse_quote!(Children), PropKind::Children);
        let out = prop_build_assignment(&p).to_string();
        assert!(out.contains("unwrap_or_else"));
        assert!(out.contains("Rc :: new"));
        assert!(out.contains("View :: Empty"));
    }

    #[test]
    fn prop_build_assignment_default_uses_user_expr() {
        let p = make_prop(
            "count",
            parse_quote!(i32),
            PropKind::Default {
                default: parse_quote!(99),
                is_generic: false,
            },
        );
        let out = prop_build_assignment(&p).to_string();
        assert!(out.contains("unwrap_or_else"));
        assert!(out.contains("99"));
    }

    // -- expand() end-to-end shape ----------------------------------------

    #[test]
    fn expand_emits_props_struct_and_rewritten_fn() {
        let input: TokenStream2 = quote! {
            fn card(title: String) -> Element {
                render! { view { text { {title.clone()} } } }
            }
        };
        let output = expand(input).to_string();
        // Props struct lives inside the hidden inner mod.
        assert!(output.contains("struct CardProps"));
        assert!(output.contains("struct CardPropsBuilder"));
        assert!(output.contains("fn card"));
        assert!(output.contains("__props : CardProps"));
        assert!(output.contains("CardProps { title }"));
        // PascalCase alias is emitted.
        assert!(output.contains("use __card_inner :: card as Card"));
    }

    #[test]
    fn expand_no_param_component_emits_empty_destructure() {
        let input: TokenStream2 = quote! {
            fn header() -> Element {
                render! { view { text { "Hi" } } }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct HeaderProps"));
        assert!(
            output.contains("HeaderProps { }") || output.contains("HeaderProps {}"),
            "no-param destructure should be empty braces; got: {output}"
        );
        // No setters for a zero-param component, just builder()+build().
        assert!(output.contains("pub fn builder"));
        assert!(output.contains("pub fn build"));
    }

    #[test]
    fn expand_does_not_reference_typed_builder() {
        // Regression: we replaced typed-builder with a hand-rolled
        // builder. The emission must not reference the old crate
        // path or its derive macro.
        let input: TokenStream2 = quote! {
            fn card(title: String, count: i32) -> Element {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(!output.contains("typed_builder"));
        assert!(!output.contains("TypedBuilder"));
        assert!(!output.contains("__typed_builder"));
    }

    #[test]
    fn expand_generic_component_uses_turbofish() {
        let input: TokenStream2 = quote! {
            fn typed<T: Clone + 'static>(value: T) -> Element {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct TypedProps"));
        assert!(
            output.contains("typed :: < T >") || output.contains("typed::<T>"),
            "generic fn should use turbofish for fn-ptr cast; got: {output}"
        );
    }

    #[test]
    fn expand_rejects_method_receiver() {
        let input: TokenStream2 = quote! {
            fn card(&self, title: String) -> Element {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        // The expansion replaces the body with a compile_error!() —
        // detect it via the macro path.
        assert!(
            output.contains("compile_error"),
            "method receiver should produce a compile error; got: {output}"
        );
    }

    #[test]
    fn expand_rejects_destructuring_pattern() {
        let input: TokenStream2 = quote! {
            fn card((a, b): (i32, i32)) -> Element {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("compile_error"));
    }

    #[test]
    fn expand_props_alias_strips_props_suffix_once() {
        // Regression test for the `TwoPropsProps -> Two` greedy-trim
        // bug. The alias derived from `TwoPropsProps` must be
        // `TwoProps`, not `Two`.
        let input: TokenStream2 = quote! {
            fn two_props(title: String, count: i32) -> Element {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(
            output.contains("as TwoProps"),
            "alias should be `TwoProps`, not the over-trimmed `Two`; got: {output}"
        );
    }

    #[test]
    fn expand_forwards_attribute_on_param_to_props_field() {
        // Non-`#[prop]` attrs ride along on the emitted Props field.
        let input: TokenStream2 = quote! {
            fn card(#[allow(dead_code)] title: String) -> Element {
                render! { view {} }
            }
        };
        let output = expand(input).to_string();
        assert!(
            output.contains("allow (dead_code)"),
            "user attr should appear on the Props field; got: {output}"
        );
    }

    // -- Helper used by classify_* assertions -----------------------------

    fn kind_name(k: &PropKind) -> &'static str {
        match k {
            PropKind::Required => "Required",
            PropKind::RequiredGeneric => "RequiredGeneric",
            PropKind::Optional { .. } => "Optional",
            PropKind::Children => "Children",
            PropKind::Default { .. } => "Default",
        }
    }
}
