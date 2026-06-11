//! `#[whisker::module_component]` proc-macro.
//!
//! Generates a builder-style API for a custom Lynx element identified
//! by a tag-name string. The macro shape mirrors `#[component]` —
//! same `<Name>Props` struct, same hand-rolled builder, same
//! PascalCase alias — but the function body is **auto-generated**
//! rather than supplied by the user. Each declared parameter
//! becomes either an attribute, an inline-style write, an event
//! handler, or the children list on the underlying Lynx element,
//! depending on its name + type.
//!
//! ## User syntax
//!
//! ```ignore
//! #[whisker::module_component("x-input")]
//! pub fn x_input(
//!     value: Signal<String>,                // → SetAttribute("value", …) — Static / Dynamic dispatch
//!     placeholder: Signal<String>,          // → SetAttribute("placeholder", …)
//!     style: Signal<String>,                // → SetRawInlineStyles(…)
//!     checked: Signal<bool>,                // → SetAttribute("checked", "true" / "false") via ToString
//!     on_focus: (),                         // → event::bind_unit("focus", Fn())
//!     on_input: TouchEvent,                 // → event::bind_typed::<TouchEvent>("input", Fn(TouchEvent))
//!     children: Children,                   // → child views attached to this element
//! ) {}
//! //  ^^
//! //  empty body — the macro replaces it.
//! ```
//!
//! Rust's grammar requires a body for top-level `fn` items (no
//! `extern fn ...;` outside an `extern {}` block), so the
//! placeholder `{}` is unavoidable. The macro discards whatever
//! return type and body the user supplies — the auto-generated
//! body always returns `whisker::runtime::view::Element`.
//!
//! ## Prop classification
//!
//! The macro inspects each declared parameter and classifies it:
//!
//! | Name pattern | Type pattern         | Treated as                         |
//! |--------------|----------------------|------------------------------------|
//! | any          | `Children`           | Children block                     |
//! | `on_*`       | `()`                 | Event handler, payload ignored     |
//! | `on_*`       | `E: Deserialize`     | Event handler, body deserialized into `E` (`TouchEvent`, `WhiskerValue`, …) |
//! | `style`      | `Signal<String>` etc.| Inline-styles (SetRawInlineStyles) |
//! | other        | `Signal<T>`          | Attribute, dispatch on Static/Dynamic |
//! | other        | `T`                  | Attribute, static set-once         |
//!
//! For the value-prop rows, `T` must implement `ToString + Clone +
//! 'static` (every primitive plus `String`/`&str`).
//!
//! ## What the macro emits
//!
//! Conceptually:
//!
//! ```ignore
//! pub struct XInputProps {
//!     pub value: Signal<String>,
//!     pub on_input: ::std::boxed::Box<dyn ::std::ops::Fn(TouchEvent) + 'static>,
//!     pub children: Children,
//!     /* … */
//! }
//! impl XInputProps {
//!     pub fn builder() -> XInputPropsBuilder { … }
//! }
//! impl XInputPropsBuilder {
//!     pub fn value(self, v: impl Into<Signal<String>>) -> Self { … }
//!     pub fn on_input<F: Fn(TouchEvent) + 'static>(self, f: F) -> Self { … }
//!     pub fn children(self, c: Children) -> Self { … }
//!     pub fn build(self) -> XInputProps { … }
//! }
//! pub fn XInput(props: XInputProps) -> Element {
//!     let h = view::create_element_by_name("x-input");
//!     apply_attr::<_, String>(h, "value", props.value);
//!     event::bind_typed::<TouchEvent, _>(h, "input", props.on_input);
//!     let view: View = (props.children)();
//!     view.attach_to(h);
//!     h
//! }
//! ```
//!
//! ## Call-site shape
//!
//! Same as user components and built-in tags. Inside `render!`:
//!
//! ```ignore
//! let (text, set_text) = signal(String::new());
//! render! {
//!     XInput(
//!         value: text,
//!         on_input: move |new_value| set_text.set(new_value),
//!     )
//! }
//! ```

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    parse2, FnArg, GenericArgument, Ident, ItemFn, LitStr, Pat, PathArguments, Type, TypePath,
    TypeTuple,
};

pub fn expand(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    // Attribute payload: a single string literal — the tag name
    // `lynx_create_fiber_element_by_name` will register against.
    let tag_name: LitStr = match parse2(attr.clone()) {
        Ok(s) => s,
        Err(_) => {
            return syn::Error::new(
                attr.span(),
                "#[whisker::module_component(\"<tag-name>\")] requires a \
                 string-literal tag name (e.g. `\"Hello\"`, `\"Input\"`)",
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
            "#[whisker::module_component] does not support generic parameters \
             — platform components are tag-name driven, not type-parameterised. \
             Each tag is a distinct registered Lynx UI class.",
        )
        .to_compile_error();
    }

    let mut props: Vec<Prop> = Vec::new();
    for arg in &sig.inputs {
        let pat_type = match arg {
            FnArg::Typed(t) => t,
            FnArg::Receiver(r) => {
                return syn::Error::new(
                    r.span(),
                    "#[whisker::module_component] does not support method receivers",
                )
                .to_compile_error();
            }
        };
        let ident = match &*pat_type.pat {
            Pat::Ident(pi) => pi.ident.clone(),
            other => {
                return syn::Error::new(
                    other.span(),
                    "#[whisker::module_component] parameters must be plain identifiers",
                )
                .to_compile_error();
            }
        };
        let ty = (*pat_type.ty).clone();
        let kind = match classify(&ident, &ty) {
            Ok(k) => k,
            Err(e) => return e.to_compile_error(),
        };
        props.push(Prop { ident, ty, kind });
    }

    let props_name = format_ident!("{}", to_pascal_case(&fn_name.to_string()) + "Props");
    let builder_name = format_ident!("{}Builder", props_name);
    let internal_mod = format_ident!("__{}_props_internal", fn_name);

    // ----- Per-prop tokens -------------------------------------------

    let props_fields: Vec<TokenStream2> = props.iter().map(prop_struct_field).collect();

    let builder_fields: Vec<TokenStream2> = props.iter().map(prop_builder_field).collect();

    let builder_init: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            quote! { #i: ::std::option::Option::None }
        })
        .collect();

    let setters: Vec<TokenStream2> = props.iter().map(prop_setter).collect();

    let build_assignments: Vec<TokenStream2> = props
        .iter()
        .map(|p| prop_build_assignment(p, &tag_name_str))
        .collect();

    let apply_calls: Vec<TokenStream2> = props.iter().map(prop_apply_call).collect();

    // ----- Drop-unused guard for prop-less elements ------------------

    let drop_unused = if props.is_empty() {
        quote! { let _ = props; }
    } else {
        quote! {}
    };

    let inner_mod = format_ident!("__{}_inner", fn_name);

    // PascalCase alias — same scheme as `#[component]`.
    let pascal_alias_ident = format_ident!("{}", to_pascal_case(&fn_name.to_string()));
    let fn_name_str = fn_name.to_string();
    // Alongside the value-namespace `pub use` we emit a type alias of
    // the same name pointing at the Props struct. Rust keeps value and
    // type namespaces separate, so `XZeroProps` resolves to the fn in
    // call position and to `XZeroPropsProps` in type position — letting
    // `render!` write `Alias::builder()` with only the alias imported,
    // no separate `…Props` import. See `#[component]` for the twin.
    let alias_emission = if pascal_alias_ident == fn_name_str.as_str() {
        quote! {
            #[doc(hidden)]
            #vis use #inner_mod::#fn_name;
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            #vis type #fn_name = #props_name;
        }
    } else {
        quote! {
            #[allow(non_snake_case)]
            #vis use #inner_mod::#fn_name as #pascal_alias_ident;
            #[doc(hidden)]
            #vis type #pascal_alias_ident = #props_name;
        }
    };

    // Every platform component implicitly carries a `__ref:
    // Option<ElementRef>` Props field. `render!` recognises
    // `ref: <expr>` at the call site and routes it to the
    // `.with_ref(expr)` setter the macro emits below. Inside the
    // body we `bind(__handle)` the ref after creating the element,
    // so the `ElementRef` reaches a live `Element` handle as soon
    // as the call site hits this code path.

    quote! {
        #[doc(hidden)]
        mod #internal_mod {
            use super::*;

            pub struct #props_name {
                #(#props_fields,)*
                /// Implicit `ref:` prop. Bound to the freshly-created
                /// element inside the macro-emitted body so user code
                /// can invoke element methods after mount.
                pub __ref: ::std::option::Option<::whisker::ElementRef>,
            }

            #[doc(hidden)]
            pub struct #builder_name {
                #(#builder_fields,)*
                pub __ref: ::std::option::Option<::whisker::ElementRef>,
            }

            impl #props_name {
                pub fn builder() -> #builder_name {
                    #builder_name {
                        #(#builder_init,)*
                        __ref: ::std::option::Option::None,
                    }
                }
            }

            impl #builder_name {
                #(#setters)*

                /// Bind an `ElementRef` to this element on mount.
                /// `render!` routes the `ref:` kwarg here. Takes
                /// the ref by value (a `Copy` slotmap handle) so
                /// callers can keep theirs for later `invoke` calls.
                pub fn with_ref(
                    mut self,
                    r: ::whisker::ElementRef,
                ) -> Self {
                    self.__ref = ::std::option::Option::Some(r);
                    self
                }

                pub fn build(self) -> #props_name {
                    #props_name {
                        #(#build_assignments,)*
                        __ref: self.__ref,
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
                // Tag string is namespaced by the cargo crate name to
                // avoid collisions between elements from independent
                // module packages. Two unrelated crates that both
                // declare `#[whisker::module_component("Video")]` end up
                // with distinct platform-side registrations
                // (`crate-a:Video` vs `crate-b:Video`). The platform
                // SwiftPM plugin / KSP processor prepends the same
                // namespace when emitting the matching
                // `LynxComponentRegistry.registerUI` / `addBehavior`
                // call, so the lookup matches end-to-end.
                let __handle = ::whisker::runtime::view::create_element_by_name(
                    concat!(env!("CARGO_PKG_NAME"), ":", #tag_name)
                );
                #(#apply_calls)*
                // Bind the user-supplied `ElementRef` (if any) to the
                // freshly-created handle so `ref.invoke("play", ...)`
                // calls route through the C bridge. The matching
                // `on_cleanup(...)` clears the binding on unmount so
                // post-unmount calls surface as `RefError::NotBound`
                // rather than dispatching against a recycled
                // `Element` ID.
                if let ::std::option::Option::Some(__r) = props.__ref {
                    __r.__bind(__handle);
                    ::whisker::on_cleanup(move || __r.__unbind());
                }
                __handle
            }
        }

        #alias_emission
    }
}

// ----- Prop classification ------------------------------------------------

struct Prop {
    ident: Ident,
    ty: Type,
    kind: PropKind,
}

enum PropKind {
    /// `style: Signal<String>` (or any `Signal<T>` / `T` whose name is
    /// `style`) — routed through `apply_styles`.
    Style { inner: Type },
    /// Plain attribute — `Signal<T>` or `T`, name not in the special
    /// list. Routed through `apply_attr` with the kebab-cased name.
    Attr { inner: Type },
    /// Children prop. Either `Children` directly (`Rc<dyn Fn() -> View>`)
    /// or any other type the user names `children`. The macro
    /// attaches the resulting View to the element after all attribute
    /// writes.
    Children,
    /// `on_<event>: ()` — no-payload event handler. The macro
    /// generates a `Box<dyn Fn() + 'static>` field and wires it via
    /// `event::bind_unit` (the value-carrying primitive, payload
    /// ignored).
    EventNoPayload { event: String },
    /// `on_<event>: E` — typed-payload event handler. `E` is any
    /// `serde::Deserialize` type (the typed event structs in
    /// `whisker::event`, or `WhiskerValue` for the raw body). The
    /// macro generates a `Box<dyn Fn(E) + 'static>` field and wires
    /// it via `event::bind_typed`, which deserializes the
    /// `WhiskerValue` event body into `E` before calling the handler.
    EventTyped { event: String, payload: Type },
}

fn classify(ident: &Ident, ty: &Type) -> syn::Result<PropKind> {
    let name = ident.to_string();

    // Children always wins, regardless of type.
    if name == "children" {
        return Ok(PropKind::Children);
    }

    // Event handler? The `on_<event>` naming convention picks these out.
    // Payload classification comes from the declared TYPE — `()` means
    // the payload is ignored; any other type is deserialized from the
    // event body via `bind_typed` (must be `serde::Deserialize`).
    if let Some(event) = name.strip_prefix("on_") {
        if event.is_empty() {
            return Err(syn::Error::new(
                ident.span(),
                "#[whisker::module_component]: event prop name `on_` is empty; \
                 use e.g. `on_tap: ()` or `on_input: TouchEvent`",
            ));
        }
        let event = event.to_string();
        if is_unit_type(ty) {
            return Ok(PropKind::EventNoPayload { event });
        }
        // Any other type → typed-payload handler. `E` must be
        // `serde::Deserialize` (enforced at the `bind_typed` call
        // site); the typed event structs in `whisker::event` and
        // `WhiskerValue` (raw body) all qualify.
        return Ok(PropKind::EventTyped {
            event,
            payload: ty.clone(),
        });
    }

    // Style → SetRawInlineStyles. Inner type extraction is the same
    // as for any attribute — strip `Signal<…>` if present.
    if name == "style" {
        return Ok(PropKind::Style {
            inner: signal_inner(ty).unwrap_or_else(|| ty.clone()),
        });
    }

    // Otherwise: an attribute prop. Strip the `Signal<…>` wrapper if
    // present so the apply_attr turbofish picks the right T (= the
    // value's ToString-able payload, not the wrapped Signal).
    Ok(PropKind::Attr {
        inner: signal_inner(ty).unwrap_or_else(|| ty.clone()),
    })
}

/// If `ty` matches `Signal<X>` (in `whisker::Signal<X>` form too),
/// return `X`. Otherwise `None`.
fn signal_inner(ty: &Type) -> Option<Type> {
    let Type::Path(TypePath { path, qself: None }) = ty else {
        return None;
    };
    let seg = path.segments.last()?;
    if seg.ident != "Signal" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(TypeTuple { elems, .. }) if elems.is_empty())
}

// ----- Codegen helpers ----------------------------------------------------

fn prop_struct_field(p: &Prop) -> TokenStream2 {
    let i = &p.ident;
    match &p.kind {
        PropKind::Style { .. } | PropKind::Attr { .. } => {
            let t = &p.ty;
            quote! { pub #i: #t }
        }
        PropKind::Children => {
            quote! { pub #i: ::whisker::runtime::view::Children }
        }
        PropKind::EventNoPayload { .. } => {
            quote! { pub #i: ::std::boxed::Box<dyn ::std::ops::Fn() + 'static> }
        }
        PropKind::EventTyped { payload, .. } => {
            quote! { pub #i: ::std::boxed::Box<dyn ::std::ops::Fn(#payload) + 'static> }
        }
    }
}

fn prop_builder_field(p: &Prop) -> TokenStream2 {
    let i = &p.ident;
    match &p.kind {
        PropKind::Style { .. } | PropKind::Attr { .. } => {
            let t = &p.ty;
            quote! { #i: ::std::option::Option<#t> }
        }
        PropKind::Children => {
            quote! { #i: ::std::option::Option<::whisker::runtime::view::Children> }
        }
        PropKind::EventNoPayload { .. } => {
            quote! { #i: ::std::option::Option<::std::boxed::Box<dyn ::std::ops::Fn() + 'static>> }
        }
        PropKind::EventTyped { payload, .. } => {
            quote! { #i: ::std::option::Option<::std::boxed::Box<dyn ::std::ops::Fn(#payload) + 'static>> }
        }
    }
}

fn prop_setter(p: &Prop) -> TokenStream2 {
    let i = &p.ident;
    match &p.kind {
        PropKind::Style { .. } | PropKind::Attr { .. } => {
            let t = &p.ty;
            quote! {
                #[allow(unused_mut)]
                pub fn #i(mut self, value: impl ::std::convert::Into<#t>) -> Self {
                    self.#i = ::std::option::Option::Some(value.into());
                    self
                }
            }
        }
        PropKind::Children => {
            // Match the render! macro's UserComponent emission shape:
            //   .children(::std::rc::Rc::new(move || { … }))
            // So the setter accepts a `Children` directly.
            quote! {
                #[allow(unused_mut)]
                pub fn #i(mut self, value: ::whisker::runtime::view::Children) -> Self {
                    self.#i = ::std::option::Option::Some(value);
                    self
                }
            }
        }
        PropKind::EventNoPayload { .. } => {
            quote! {
                #[allow(unused_mut)]
                pub fn #i<F: ::std::ops::Fn() + 'static>(mut self, f: F) -> Self {
                    self.#i = ::std::option::Option::Some(::std::boxed::Box::new(f));
                    self
                }
            }
        }
        PropKind::EventTyped { payload, .. } => {
            quote! {
                #[allow(unused_mut)]
                pub fn #i<F: ::std::ops::Fn(#payload) + 'static>(mut self, f: F) -> Self {
                    self.#i = ::std::option::Option::Some(::std::boxed::Box::new(f));
                    self
                }
            }
        }
    }
}

fn prop_build_assignment(p: &Prop, tag_name: &str) -> TokenStream2 {
    let i = &p.ident;
    let name = i.to_string();
    let err = format!("required prop `{name}` was not set on `{tag_name}`");
    match &p.kind {
        PropKind::Children => {
            // Default to an empty children list when omitted — mirrors
            // `#[component]`'s Children default.
            quote! {
                #i: self.#i.unwrap_or_else(|| {
                    ::std::rc::Rc::new(|| ::whisker::runtime::view::View::Empty)
                })
            }
        }
        // Style/Attr props are optional by default. `Signal<String>`
        // defaults to `Signal::Static("")` when omitted, matching
        // what Lynx would see if the attribute wasn't declared.
        // Event handler props stay required because their `dyn Fn`
        // types don't have a sensible default and a missing callback
        // is almost always an author bug.
        PropKind::Style { .. } | PropKind::Attr { .. } => {
            quote! { #i: self.#i.unwrap_or_default() }
        }
        PropKind::EventNoPayload { .. } | PropKind::EventTyped { .. } => {
            quote! { #i: self.#i.expect(#err) }
        }
    }
}

fn prop_apply_call(p: &Prop) -> TokenStream2 {
    let i = &p.ident;
    let name = i.to_string();
    match &p.kind {
        PropKind::Style { inner } => {
            quote! {
                ::whisker::runtime::view::apply_styles::<_, #inner>(__handle, props.#i);
            }
        }
        PropKind::Attr { inner } => {
            let attr_name = name.replace('_', "-");
            quote! {
                ::whisker::runtime::view::apply_attr::<_, #inner>(__handle, #attr_name, props.#i);
            }
        }
        PropKind::EventNoPayload { event } => {
            quote! {
                ::whisker::runtime::event::bind_unit(
                    __handle,
                    #event,
                    ::whisker::runtime::event::BindType::Bind,
                    props.#i,
                );
            }
        }
        PropKind::EventTyped { event, payload } => {
            quote! {
                ::whisker::runtime::event::bind_typed::<#payload, _>(
                    __handle,
                    #event,
                    ::whisker::runtime::event::BindType::Bind,
                    props.#i,
                );
            }
        }
        PropKind::Children => {
            quote! {
                let __children_view: ::whisker::runtime::view::View = (props.#i)();
                ::whisker::runtime::view::IntoView::into_view(__children_view)
                    .attach_to(__handle);
            }
        }
    }
}

/// `hello` / `my_input` → `Hello` / `MyInput`. ASCII-only — native
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
