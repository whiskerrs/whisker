//! `#[component]` proc-macro implementation.
//!
//! Walks the user's function signature, extracts the parameter list,
//! and emits:
//!
//! 1. A `XxxProps` struct whose fields mirror the function parameters.
//! 2. A public PascalCase component marker and hand-rolled builder with one
//!    setter per parameter. Const-bool type state makes every non-optional
//!    prop a compile-time requirement; `Option<T>`, `Children`, and
//!    `#[prop(default = …)]` props remain optional.
//! 3. A rewritten `fn xxx(__props: XxxProps) -> Element { … }` that
//!    destructures the props back into local variables and runs the
//!    user's original body inside the existing
//!    `mount_component_remountable` hot-reload machinery.
//! 4. A PascalCase marker (`XxxName::builder()`) that is equally usable from
//!    ordinary Rust and the composition macros.
//!
//! The generated builder is hand-rolled so its helper state stays hidden from
//! rust-analyzer completion while required props are still checked by Rust's
//! type system. `render! { XxxName(prop: value) }` is only syntax sugar for
//! `XxxName::builder().prop(value).build()`.

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

    // Both forms are needed: the fn and Props struct carry the full
    // bounds in declaration position, while the turbofish on the
    // `as *const ()` cast needs the type-only form so the fn pointer
    // monomorphizes correctly.
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let ty_generics_for_turbofish = ty_generics_to_turbofish(generics);

    // A prop whose type is a bare generic param (`value: T`) must skip
    // `setter(into)`: `Into<T>` with an unconstrained `T` blows up
    // call-site inference.
    let generic_type_params: Vec<Ident> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t.ident.clone()),
            _ => None,
        })
        .collect();

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

        // `#[prop(...)]` is a `#[component]` directive, not something
        // the Props struct carries forward. Other attrs
        // (`#[allow(...)]`, doc comments) ride along unchanged.
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

    let props_fields: Vec<TokenStream2> = props.iter().map(prop_struct_field).collect();
    let builder_fields: Vec<TokenStream2> = props.iter().map(prop_builder_field).collect();
    let builder_init: Vec<TokenStream2> = props.iter().map(prop_builder_init).collect();
    let build_assignments: Vec<TokenStream2> = props.iter().map(prop_build_assignment).collect();
    let body_method = props
        .iter()
        .find(|prop| matches!(prop.kind, PropKind::Children))
        .map(|prop| {
            let ident = &prop.ident;
            quote! {
                pub fn body<F>(mut self, compose: F) -> Self
                where
                    F: ::std::ops::Fn(&mut ::whisker::ChildrenBuilder) + 'static,
                {
                    self.#ident = ::std::option::Option::Some(::std::rc::Rc::new(move || {
                        let mut body = ::whisker::ChildrenBuilder::new();
                        compose(&mut body);
                        body.finish()
                    }));
                    self
                }
            }
        });

    let props_name = props_struct_name(fn_name);

    // The destructure + capture + re-clone pattern the remount
    // machinery expects:
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

    // Touching every prop pins the subsecond-dispatched closure's
    // captured-environment layout to the props signature alone.
    // Without it, an edit that merely *starts using* a previously-unused
    // prop changes the capture set, and the hot-patched body then reads
    // the old, smaller environment through the new layout: UB.
    let force_capture = if prop_idents.is_empty() {
        quote! {}
    } else {
        quote! {
            #[allow(unused, clippy::no_effect_underscore_binding)]
            let _ = ( #( &#prop_idents, )* );
        }
    };

    // Layout hash: FNV-1a over the props signature tokens, the
    // compile-time part of the closure's captured-environment layout.
    // The generated `__whisker_props_hash` also folds in each prop
    // type's `size_of`/`align_of` AT ITS OWN BUILD, so a change to a
    // prop type's *definition* shifts the value too. Read through
    // subsecond dispatch at remount time so the runtime can refuse an
    // in-place remount when the stored closure's layout no longer
    // matches what the patched code expects.
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

    // A generic fn must be turbofished so the cast picks the CURRENT
    // monomorphization: each `T` yields a distinct `*const ()`, which
    // is what the per-component remount registry keys on.
    let fn_ptr_expr = if ty_generics_for_turbofish.is_empty() {
        quote! { #fn_name as *const () }
    } else {
        quote! { #fn_name :: < #(#ty_generics_for_turbofish),* > as *const () }
    };

    // Props + Builder live inside a PRIVATE module so the builder
    // type's name isn't identifier-completion noise at the user's call
    // sites. Only `XxxProps` is re-exported (doc-hidden); the builder
    // is reached through `.builder()` and never needs to be in scope.
    let internal_mod = format_ident!("__{}_props_internal", fn_name);
    let builder_name = format_ident!("{}Builder", props_name);
    let inner_mod = format_ident!("__{}_inner", fn_name);
    let required_state_idents: Vec<Ident> = props
        .iter()
        .filter(|prop| matches!(prop.kind, PropKind::Required | PropKind::RequiredGeneric))
        .map(|prop| format_ident!("__{}_SET", prop.ident.to_string().to_ascii_uppercase()))
        .collect();
    let mut builder_generics = generics.clone();
    for state in &required_state_idents {
        builder_generics
            .params
            .push(syn::parse_quote!(const #state: bool));
    }
    let (builder_impl_generics, builder_ty_generics, builder_where_clause) =
        builder_generics.split_for_impl();
    let original_generic_args: Vec<TokenStream2> = generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Lifetime(lifetime) => {
                let lifetime = &lifetime.lifetime;
                quote! { #lifetime }
            }
            GenericParam::Type(ty) => {
                let ident = &ty.ident;
                quote! { #ident }
            }
            GenericParam::Const(constant) => {
                let ident = &constant.ident;
                quote! { #ident }
            }
        })
        .collect();
    let builder_type = |states: &[TokenStream2]| {
        let mut arguments = original_generic_args.clone();
        arguments.extend_from_slice(states);
        if arguments.is_empty() {
            quote! { #builder_name }
        } else {
            quote! { #builder_name < #(#arguments),* > }
        }
    };
    let initial_states = required_state_idents
        .iter()
        .map(|_| quote! { false })
        .collect::<Vec<_>>();
    let ready_states = required_state_idents
        .iter()
        .map(|_| quote! { true })
        .collect::<Vec<_>>();
    let initial_builder_type = builder_type(&initial_states);
    let ready_builder_type = builder_type(&ready_states);
    let state_field = (!required_state_idents.is_empty()).then(|| {
        quote! {
            __required: ::std::marker::PhantomData<(
                #(RequiredState<#required_state_idents>,)*
            )>
        }
    });
    let state_init = (!required_state_idents.is_empty()).then(|| {
        quote! {
            __required: ::std::marker::PhantomData
        }
    });
    let field_bindings: Vec<Ident> = prop_idents
        .iter()
        .map(|ident| format_ident!("__field_{}", ident))
        .collect();
    let setter_methods: Vec<TokenStream2> = props
        .iter()
        .enumerate()
        .map(|(prop_index, prop)| {
            if !matches!(prop.kind, PropKind::Required | PropKind::RequiredGeneric) {
                return prop_setter_method(prop);
            }
            let required_index = props[..prop_index]
                .iter()
                .filter(|prop| matches!(prop.kind, PropKind::Required | PropKind::RequiredGeneric))
                .count();
            let target_states = required_state_idents
                .iter()
                .enumerate()
                .map(|(index, state)| {
                    if index == required_index {
                        quote! { true }
                    } else {
                        quote! { #state }
                    }
                })
                .collect::<Vec<_>>();
            let target_type = builder_type(&target_states);
            let ident = &prop.ident;
            let ty = &prop.ty;
            let argument = if matches!(prop.kind, PropKind::RequiredGeneric) {
                quote! { value: #ty }
            } else {
                quote! { value: impl ::std::convert::Into<#ty> }
            };
            let stored = if matches!(prop.kind, PropKind::RequiredGeneric) {
                quote! { value }
            } else {
                quote! { value.into() }
            };
            let assignments =
                prop_idents
                    .iter()
                    .zip(field_bindings.iter())
                    .map(|(field, binding)| {
                        if field == ident {
                            quote! { #field: ::std::option::Option::Some(#stored) }
                        } else {
                            quote! { #field: #binding }
                        }
                    });
            quote! {
                pub fn #ident(self, #argument) -> #target_type {
                    let Self {
                        #(#prop_idents: #field_bindings,)*
                        __required: _,
                    } = self;
                    #builder_name {
                        #(#assignments,)*
                        __required: ::std::marker::PhantomData,
                    }
                }
            }
        })
        .collect();
    let public_fn_ptr_expr = if ty_generics_for_turbofish.is_empty() {
        quote! { #inner_mod::#fn_name as *const () }
    } else {
        quote! {
            #inner_mod::#fn_name :: < #(#ty_generics_for_turbofish),* > as *const ()
        }
    };
    let props_struct = quote! {
        // No `#vis`: the module is deliberately tighter than the
        // surrounding fn so the builder stays unreachable by name.
        #[doc(hidden)]
        mod #internal_mod {
            // Prop types referenced in fields must resolve here.
            use super::*;

            pub struct #props_name #impl_generics #where_clause {
                #(#props_fields),*
            }

            // Every builder field becomes `Option<T>` (or
            // `Option<Option<T>>` for an already-`Option<T>` prop) so
            // "not set" is distinguishable from "set to None".
            //
            // The struct must stay `pub`: a private type's `pub fn`
            // methods are unreachable from outside the module even when
            // the caller holds the value, which would break
            // `Xxx::builder().setter(…).build()`. Its name is
            // therefore visible at the call site, and `#[doc(hidden)]`
            // is the only signal RA's auto-import filter can act on.
            #[doc(hidden)]
            struct RequiredState<const SET: bool>;

            pub struct #builder_name #builder_generics #builder_where_clause {
                #(#builder_fields,)*
                #state_field
            }

            impl #impl_generics #props_name #ty_generics #where_clause {
                /// Internal Props entry point used by the public component marker.
                pub fn builder() -> #initial_builder_type {
                    #builder_name {
                        #(#builder_init,)*
                        #state_init
                    }
                }
            }

            impl #builder_impl_generics #builder_name #builder_ty_generics #builder_where_clause {
                #(#setter_methods)*
                #body_method
            }

            impl #impl_generics #ready_builder_type #where_clause {
                /// Mount the component and return its rendered root.
                pub fn build(self) -> ::whisker::Element {
                    super::#inner_mod::#fn_name(#props_name {
                        #(#build_assignments),*
                    })
                }
            }
        }
        #[doc(hidden)]
        #vis use #internal_mod::#props_name;
    };

    // The PascalCase marker is the canonical builder entry point.
    //
    // Strip the `Props` suffix exactly once: `trim_end_matches` would
    // greedily strip repeats (`TwoPropsProps` → `Two`).
    let props_name_str = props_name.to_string();
    let alias_str = props_name_str
        .strip_suffix("Props")
        .unwrap_or(&props_name_str);
    let fn_name_str = fn_name.to_string();

    // The rewritten fn lives inside a PRIVATE inner module so its
    // snake_case name doesn't pollute outer-scope completion. Only the
    // The PascalCase marker below is the only public invocation entry point.
    let new_fn = quote! {
        #[doc(hidden)]
        mod #inner_mod {
            use super::*;

            // Read through `__hot::call_hash` so a just-applied patch
            // answers with ITS layout: the size_of/align_of folds
            // evaluate in whichever build this fn was compiled into,
            // which is what catches a prop-type *definition* change.
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

                // The outer closure keeps re-clone bookkeeping out of
                // the subsecond-dispatched inner one, which must sit at
                // the user crate's source position for hot reload to
                // find it.
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

    // `#[component]` must sit at module level: `pub use` only works
    // there.
    //
    let marker_ident = if alias_str == fn_name_str {
        fn_name.clone()
    } else {
        format_ident!("{}", alias_str)
    };
    let pascal_alias = quote! {
        #[allow(missing_docs)]
        #vis struct #marker_ident;

        #[allow(missing_docs)]
        impl #marker_ident {
            pub fn builder #impl_generics () -> #internal_mod::#initial_builder_type #where_clause {
                #props_name::builder()
            }

            #[doc(hidden)]
            pub fn __function_id #impl_generics () -> *const () #where_clause {
                #public_fn_ptr_expr
            }
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
    /// `Option<T>` from the caller's perspective. User code is expected
    /// to write `Option<T>` directly; this is reserved for opting into
    /// the same treatment on a wrapper type the macro can't detect.
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
    /// `Option<U>` prop. Builder stores `Option<Option<U>>` so "user
    /// didn't set it" (outer None) is distinguishable from "user set it
    /// to None" (outer Some, inner None — reachable only via direct
    /// construction, not render!).
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
    /// `Rc<dyn Fn>` — there's no useful `Into` story). Build defaults a
    /// missing children prop to a closure returning `View::Empty`.
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
/// directive. The precedence:
///
/// 1. `#[prop(default = expr)]` wins regardless of type.
/// 2. `#[prop(optional)]` on a non-`Option<T>` type → upgrade to
///    `Optional { inner: T, ... }` so the user can omit.
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
        // Wrap into an Optional with the same inner so setter/build
        // still typecheck.
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
/// the user wrote them.
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
            let option_setter = format_ident!("{}_option", ident);
            // Setter takes the inner (unwrapped) T; stored as
            // `Some(Some(v))` to record both "set" and "set to Some".
            if *inner_is_generic {
                quote! {
                    #[allow(unused_mut)]
                    pub fn #ident(mut self, value: #inner) -> Self {
                        self.#ident = ::std::option::Option::Some(
                            ::std::option::Option::Some(value)
                        );
                        self
                    }

                    #[allow(unused_mut)]
                    pub fn #option_setter(mut self, value: ::std::option::Option<#inner>) -> Self {
                        self.#ident = ::std::option::Option::Some(value);
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

                    #[allow(unused_mut)]
                    pub fn #option_setter(mut self, value: ::std::option::Option<#inner>) -> Self {
                        self.#ident = ::std::option::Option::Some(value);
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
            // Outer Option = "did the user call .ident(…)?" — collapse
            // missing to None.
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

/// Pull the type-parameter identifiers out of the function generics to
/// build a turbofish for the `as *const ()` cast. Lifetimes aren't part
/// of a turbofish; const generics aren't supported on `#[component]`.
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

        let custom: Type = parse_quote!(MyOptional);
        assert!(option_inner_type(&custom).is_none());

        let bare_option: Type = parse_quote!(Option);
        assert!(option_inner_type(&bare_option).is_none());

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

        assert!(!is_generic_type_param(&parse_quote!(Option<T>), &generics));
        assert!(!is_generic_type_param(&parse_quote!(crate::T), &generics));
        assert!(!is_generic_type_param(&parse_quote!(String), &generics));
        let t_with_args: Type = parse_quote!(T<i32>);
        assert!(!is_generic_type_param(&t_with_args, &generics));
        let reference: Type = parse_quote!(&'a str);
        assert!(!is_generic_type_param(&reference, &generics));
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
        let attr = PropAttr {
            optional: true,
            ..PropAttr::default()
        };
        let k = classify(parse_quote!(Option<String>), attr, &[]);
        assert!(matches!(k, PropKind::Optional { .. }));
    }

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
        assert!(out.contains("impl :: std :: convert :: Into < String >"));
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

    #[test]
    fn expand_emits_props_struct_and_rewritten_fn() {
        let input: TokenStream2 = quote! {
            fn card(title: String) -> Element {
                render! { View { Text { {title.clone()} } } }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct CardProps"));
        assert!(output.contains("struct CardPropsBuilder"));
        assert!(output.contains("fn card"));
        assert!(output.contains("__props : CardProps"));
        assert!(output.contains("CardProps { title }"));
        assert!(output.contains("struct Card"));
        assert!(output.contains("impl Card"));
        assert!(output.contains("CardProps :: builder"));
    }

    #[test]
    fn expand_no_param_component_emits_empty_destructure() {
        let input: TokenStream2 = quote! {
            fn header() -> Element {
                render! { View { Text { "Hi" } } }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("struct HeaderProps"));
        assert!(
            output.contains("HeaderProps { }") || output.contains("HeaderProps {}"),
            "no-param destructure should be empty braces; got: {output}"
        );
        assert!(output.contains("pub fn builder"));
        assert!(output.contains("pub fn build"));
    }

    #[test]
    fn expand_does_not_reference_typed_builder() {
        // The emission must not reference typed-builder.
        let input: TokenStream2 = quote! {
            fn card(title: String, count: i32) -> Element {
                render! { View {} }
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
                render! { View {} }
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
                render! { View {} }
            }
        };
        let output = expand(input).to_string();
        assert!(
            output.contains("compile_error"),
            "method receiver should produce a compile error; got: {output}"
        );
    }

    #[test]
    fn expand_rejects_destructuring_pattern() {
        let input: TokenStream2 = quote! {
            fn card((a, b): (i32, i32)) -> Element {
                render! { View {} }
            }
        };
        let output = expand(input).to_string();
        assert!(output.contains("compile_error"));
    }

    #[test]
    fn expand_component_marker_strips_props_suffix_once() {
        // The marker derived from `TwoPropsProps` must be `TwoProps`,
        // not `Two` — only one `Props` suffix comes off.
        let input: TokenStream2 = quote! {
            fn two_props(title: String, count: i32) -> Element {
                render! { View {} }
            }
        };
        let output = expand(input).to_string();
        assert!(
            output.contains("struct TwoProps ;"),
            "marker should be `TwoProps`, not the over-trimmed `Two`; got: {output}"
        );
    }

    #[test]
    fn expand_forwards_attribute_on_param_to_props_field() {
        let input: TokenStream2 = quote! {
            fn card(#[allow(dead_code)] title: String) -> Element {
                render! { View {} }
            }
        };
        let output = expand(input).to_string();
        assert!(
            output.contains("allow (dead_code)"),
            "user attr should appear on the Props field; got: {output}"
        );
    }

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
