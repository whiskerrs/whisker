//! `#[whisker::module_element]` proc-macro.
//!
//! Generates a builder-style API and, in the named form, the shared schema for
//! a custom element. The macro shape mirrors `#[component]` —
//! same private `<Name>Props` representation, same hand-rolled builder, same
//! public PascalCase marker — but the function body is **auto-generated**
//! rather than supplied by the user. Each declared parameter becomes
//! either a component-specific attribute, a structured-style write, a
//! component-specific event handler, or the children list on the
//! underlying element, depending on its name + type. The generated
//! builder also implements Whisker's common `ElementBuilder` API, so
//! style, accessibility, gesture, ref, and other universal authoring
//! features do not need to be repeated in the component schema.
//!
//! ## User syntax
//!
//! ```ignore
//! #[whisker::module_element(
//!     name = "example-forms:Input",
//!     measurement = None,
//! )]
//! pub fn input(
//!     value: Signal<String>,                // → SetAttribute("value", …) — Static / Dynamic dispatch
//!     placeholder: Signal<String>,          // → SetAttribute("placeholder", …)
//!     style: whisker::Style,                // → SetSpecifiedStyle(…)
//!     checked: Signal<bool>,                // → SetAttribute("checked", "true" / "false") via ToString
//!     on_focus: (),                         // → event::bind_unit("focus", Fn())
//!     on_input: TouchEvent,                 // → event::bind_typed::<TouchEvent>("input", Fn(TouchEvent))
//!     children: Children,                   // → child views attached to this element
//! ) {}
//! //  ^^
//! //  empty body — the macro replaces it.
//! ```
//!
//! Rust's grammar requires a body for top-level `fn` items, so the
//! placeholder `{}` is unavoidable. The macro discards whatever return
//! type and body the user supplies — the generated body always returns
//! `whisker::runtime::view::Element`.
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
//! | `style`      | `whisker::Style`      | Structured style (SetSpecifiedStyle) |
//! | other        | `Signal<T>`          | Attribute, dispatch on Static/Dynamic |
//! | other        | `T`                  | Attribute, static set-once         |
//!
//! For the value-prop rows, `T` must implement `ToString + Clone +
//! 'static` (every primitive plus `String`/`&str`).
//!
//! ## What the macro emits
//!
//! Conceptually, the generated public surface is:
//!
//! ```ignore
//! pub struct XInput;
//! impl XInput {
//!     pub fn builder() -> /* hidden typed builder */ { /* … */ }
//! }
//! XInput::builder()
//!     .value(text)
//!     .on_input(move |event| { /* … */ })
//!     .body(|body| body.push(child))
//!     .build(); // -> Element
//! ```
//!
//! Props storage and type-state helpers are generated implementation details.
//! Event handlers are required at compile time, while ordinary attribute,
//! style, and children setters may be omitted. The builder creates the Host
//! element, applies the declared schema through the same runtime operations as
//! built-ins, and returns the ordinary [`Element`](::whisker::Element).
//!
//! ## Call-site shape
//!
//! Same as user components and built-in tags. Inside `render!`:
//!
//! ```ignore
//! let text = signal(String::new());
//! render! {
//!     XInput(
//!         value: text,
//!         on_input: move |new_value| text.set(new_value),
//!     )
//! }
//! ```

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    Expr, ExprLit, FnArg, GenericArgument, Ident, ItemFn, Lit, LitBool, LitStr, Pat, PathArguments,
    Token, Type, TypePath, TypeTuple, Visibility, parse2,
};

enum ModuleElementArgs {
    Legacy(LitStr),
    Schema {
        name: LitStr,
        measurement: Ident,
        text_style: bool,
        commands: Vec<(LitStr, Ident)>,
    },
}

struct SchemaArgs {
    name: LitStr,
    measurement: Ident,
    text_style: bool,
    commands: Vec<(LitStr, Ident)>,
}

struct SchemaDefinition<'a> {
    name: &'a LitStr,
    measurement: &'a Ident,
    text_style: bool,
    commands: &'a [(LitStr, Ident)],
}

impl Parse for SchemaArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut measurement = None;
        let mut text_style = None;
        let mut commands = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "name" => {
                    if name.is_some() {
                        return Err(syn::Error::new(key.span(), "duplicate `name` argument"));
                    }
                    let expression: Expr = input.parse()?;
                    let Expr::Lit(ExprLit {
                        lit: Lit::Str(value),
                        ..
                    }) = expression
                    else {
                        return Err(syn::Error::new(
                            key.span(),
                            "`name` must be a string literal",
                        ));
                    };
                    name = Some(value);
                }
                "measurement" => {
                    if measurement.is_some() {
                        return Err(syn::Error::new(
                            key.span(),
                            "duplicate `measurement` argument",
                        ));
                    }
                    measurement = Some(input.parse()?);
                }
                "text_style" => {
                    if text_style.is_some() {
                        return Err(syn::Error::new(
                            key.span(),
                            "duplicate `text_style` argument",
                        ));
                    }
                    text_style = Some(input.parse::<LitBool>()?.value);
                }
                "commands" => {
                    if commands.is_some() {
                        return Err(syn::Error::new(key.span(), "duplicate `commands` argument"));
                    }
                    let content;
                    syn::bracketed!(content in input);
                    let mut declarations = Vec::new();
                    while !content.is_empty() {
                        let tuple;
                        syn::parenthesized!(tuple in content);
                        let name: LitStr = tuple.parse()?;
                        tuple.parse::<Token![,]>()?;
                        let kind: Ident = tuple.parse()?;
                        if !tuple.is_empty() {
                            return Err(
                                tuple.error("command entry must be `(\"name\", ValueKind)`")
                            );
                        }
                        declarations.push((name, kind));
                        if content.is_empty() {
                            break;
                        }
                        content.parse::<Token![,]>()?;
                    }
                    commands = Some(declarations);
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "unsupported module_element argument; expected `name`, `measurement`, `text_style`, or `commands`",
                    ));
                }
            }

            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(Self {
            name: name.ok_or_else(|| syn::Error::new(input.span(), "missing `name` argument"))?,
            measurement: measurement
                .ok_or_else(|| syn::Error::new(input.span(), "missing `measurement` argument"))?,
            text_style: text_style.unwrap_or(false),
            commands: commands.unwrap_or_default(),
        })
    }
}

fn parse_args(attr: TokenStream2) -> syn::Result<ModuleElementArgs> {
    if let Ok(name) = parse2::<LitStr>(attr.clone()) {
        return Ok(ModuleElementArgs::Legacy(name));
    }
    let args = parse2::<SchemaArgs>(attr)?;
    Ok(ModuleElementArgs::Schema {
        name: args.name,
        measurement: args.measurement,
        text_style: args.text_style,
        commands: args.commands,
    })
}

pub fn expand(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    let args = match parse_args(attr) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error(),
    };
    let element_name = match &args {
        ModuleElementArgs::Legacy(name) => name,
        ModuleElementArgs::Schema { name, .. } => name,
    };
    let element_name_str = element_name.value();

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
            "#[whisker::module_element] does not support generic parameters \
             — platform components are tag-name driven, not type-parameterised. \
             Each tag is one registered Host element schema.",
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
                    "#[whisker::module_element] does not support method receivers",
                )
                .to_compile_error();
            }
        };
        let ident = match &*pat_type.pat {
            Pat::Ident(pi) => pi.ident.clone(),
            other => {
                return syn::Error::new(
                    other.span(),
                    "#[whisker::module_element] parameters must be plain identifiers",
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
    let required_props: Vec<&Prop> = props
        .iter()
        .filter(|prop| {
            matches!(
                prop.kind,
                PropKind::EventNoPayload { .. } | PropKind::EventTyped { .. }
            )
        })
        .collect();
    let required_states: Vec<Ident> = required_props
        .iter()
        .map(|prop| format_ident!("__{}_SET", prop.ident.to_string().to_ascii_uppercase()))
        .collect();
    let builder_declaration = if required_states.is_empty() {
        quote! { #builder_name }
    } else {
        quote! { #builder_name < #(const #required_states: bool),* > }
    };
    let builder_type = |states: &[TokenStream2]| {
        if states.is_empty() {
            quote! { #builder_name }
        } else {
            quote! { #builder_name < #(#states),* > }
        }
    };
    let initial_states = required_states
        .iter()
        .map(|_| quote! { false })
        .collect::<Vec<_>>();
    let ready_states = required_states
        .iter()
        .map(|_| quote! { true })
        .collect::<Vec<_>>();
    let generic_states = required_states
        .iter()
        .map(|state| quote! { #state })
        .collect::<Vec<_>>();
    let initial_builder_type = builder_type(&initial_states);
    let ready_builder_type = builder_type(&ready_states);
    let generic_builder_type = builder_type(&generic_states);
    let builder_impl_generics = if required_states.is_empty() {
        quote! {}
    } else {
        quote! { <#(const #required_states: bool),*> }
    };

    let props_fields: Vec<TokenStream2> = props.iter().map(prop_struct_field).collect();

    let builder_fields: Vec<TokenStream2> = props.iter().map(prop_builder_field).collect();

    let builder_init: Vec<TokenStream2> = props
        .iter()
        .map(|p| {
            let i = &p.ident;
            quote! { #i: ::std::option::Option::None }
        })
        .collect();

    let builder_field_idents: Vec<Ident> = props.iter().map(|prop| prop.ident.clone()).collect();
    let setters: Vec<TokenStream2> = props
        .iter()
        .map(|prop| {
            let Some(required_index) = required_props
                .iter()
                .position(|required| required.ident == prop.ident)
            else {
                return prop_setter(prop);
            };
            let target_states = required_states
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
            let bindings = builder_field_idents
                .iter()
                .map(|field| format_ident!("__field_{field}"));
            let bindings = bindings.collect::<Vec<_>>();
            let assignments = builder_field_idents.iter().zip(bindings.iter()).map(
                |(field, binding)| {
                    if field == ident {
                        quote! { #field: ::std::option::Option::Some(::std::boxed::Box::new(f)) }
                    } else {
                        quote! { #field: #binding }
                    }
                },
            );
            let signature = match &prop.kind {
                PropKind::EventNoPayload { .. } => quote! { ::std::ops::Fn() + 'static },
                PropKind::EventTyped { payload, .. } => {
                    quote! { ::std::ops::Fn(#payload) + 'static }
                }
                _ => unreachable!(),
            };
            quote! {
                pub fn #ident<F: #signature>(self, f: F) -> #target_type {
                    let Self {
                        #(#builder_field_idents: #bindings,)*
                        __element,
                        __ref,
                        __required: _,
                    } = self;
                    #builder_name {
                        #(#assignments,)*
                        __element,
                        __ref,
                        __required: ::std::marker::PhantomData,
                    }
                }
            }
        })
        .collect();

    let build_assignments: Vec<TokenStream2> = props
        .iter()
        .map(|p| prop_build_assignment(p, &element_name_str))
        .collect();

    let apply_calls: Vec<TokenStream2> = props.iter().map(prop_apply_call).collect();

    let drop_unused = if props.is_empty() {
        quote! { let _ = props; }
    } else {
        quote! {}
    };

    let inner_mod = format_ident!("__{}_inner", fn_name);
    let schema_mod = format_ident!("{}_schema", fn_name);

    let pascal_alias_ident = format_ident!("{}", to_pascal_case(&fn_name.to_string()));
    let fn_name_str = fn_name.to_string();
    let marker_ident = if pascal_alias_ident == fn_name_str.as_str() {
        fn_name.clone()
    } else {
        pascal_alias_ident
    };
    let alias_emission = quote! {
        #[allow(missing_docs)]
        #vis struct #marker_ident;

        #[allow(missing_docs)]
        impl #marker_ident {
            pub fn builder() -> #internal_mod::#initial_builder_type {
                #props_name::builder()
            }
        }
    };

    let body_method = props.iter().find_map(|prop| match &prop.kind {
        PropKind::Children | PropKind::TextChildren => {
            let ident = &prop.ident;
            let stored = if matches!(&prop.kind, PropKind::TextChildren) {
                quote! { ::whisker::runtime::view::TextChildren::new(children) }
            } else {
                quote! { children }
            };
            Some(quote! {
                pub fn body<F>(mut self, compose: F) -> Self
                where
                    F: ::std::ops::Fn(&mut ::whisker::ChildrenBuilder) + 'static,
                {
                    let children: ::whisker::Children = ::std::rc::Rc::new(move || {
                        let mut body = ::whisker::ChildrenBuilder::new();
                        compose(&mut body);
                        body.finish()
                    });
                    self.#ident = ::std::option::Option::Some(#stored);
                    self
                }
            })
        }
        _ => None,
    });

    let (element_creation, schema_emission) = match &args {
        ModuleElementArgs::Legacy(tag_name) => (
            quote! {
                ::whisker::runtime::view::create_element_by_name(
                    concat!(env!("CARGO_PKG_NAME"), ":", #tag_name)
                )
            },
            quote! {},
        ),
        ModuleElementArgs::Schema {
            name,
            measurement,
            text_style,
            commands,
        } => {
            let definition = SchemaDefinition {
                name,
                measurement,
                text_style: *text_style,
                commands,
            };
            let schema = match schema_tokens(&schema_mod, vis, &definition, &props, true) {
                Ok(schema) => schema,
                Err(error) => return error.to_compile_error(),
            };
            (
                quote! {
                    ::whisker::runtime::view::create_element_by_schema(
                        &#schema_mod::schema()
                    )
                },
                schema,
            )
        }
    };

    // Every platform component implicitly carries a `__ref:
    // Option<ElementRef>` Props field. `render!` routes a call-site
    // `element_ref: <expr>` to the `.with_ref(expr)` setter emitted below; the
    // generated body then binds it to the freshly-created handle.

    quote! {
        #[doc(hidden)]
        mod #internal_mod {
            use super::*;

            pub struct #props_name {
                #(#props_fields,)*
                #[doc(hidden)]
                pub __element: ::whisker::runtime::view::Element,
                /// Implicit `element_ref:` prop. Bound to the freshly-created
                /// element inside the macro-emitted body so user code
                /// can invoke element methods after mount.
                pub __ref: ::std::option::Option<::whisker::ElementRef>,
            }

            #[doc(hidden)]
            pub struct #builder_declaration {
                #(#builder_fields,)*
                pub __element: ::whisker::runtime::view::Element,
                pub __ref: ::std::option::Option<::whisker::ElementRef>,
                __required: ::std::marker::PhantomData<(#(RequiredState<#required_states>,)*)>,
            }

            #[doc(hidden)]
            struct RequiredState<const SET: bool>;

            impl #props_name {
                pub fn builder() -> #initial_builder_type {
                    #builder_name {
                        #(#builder_init,)*
                        __element: #element_creation,
                        __ref: ::std::option::Option::None,
                        __required: ::std::marker::PhantomData,
                    }
                }
            }

            impl #builder_impl_generics #generic_builder_type {
                #(#setters)*
                #body_method

                /// Bind an `ElementRef` to this element on mount.
                /// `render!` routes the `element_ref:` kwarg here. Takes
                /// the ref by value (a `Copy` slotmap handle) so
                /// callers can keep theirs for later `invoke` calls.
                pub fn element_ref(
                    mut self,
                    r: ::whisker::ElementRef,
                ) -> Self {
                    self.__ref = ::std::option::Option::Some(r);
                    self
                }

            }

            impl #ready_builder_type {
                pub fn build(self) -> ::whisker::Element {
                    super::#inner_mod::#fn_name(#props_name {
                        #(#build_assignments,)*
                        __element: self.__element,
                        __ref: self.__ref,
                    })
                }
            }

            impl #builder_impl_generics ::whisker::__element_builder::ElementBuilder for #generic_builder_type {
                fn __element(&self) -> ::whisker::runtime::view::Element {
                    self.__element
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
                // The builder creates the element early so common
                // ElementBuilder methods and generated module props operate on
                // the same handle.
                let __handle = props.__element;
                #(#apply_calls)*
                // The matching `on_cleanup` clears the binding on
                // unmount so post-unmount calls surface as
                // `RefError::NotBound` rather than dispatching against
                // a recycled `Element` ID.
                if let ::std::option::Option::Some(__r) = props.__ref {
                    __r.__bind(__handle);
                    ::whisker::on_cleanup(move || __r.__unbind());
                }
                __handle
            }
        }

        #alias_emission
        #schema_emission
    }
}

/// Expands a built-in schema declaration without generating a second
/// authoring builder. Built-ins retain their hand-tuned `ElementTag` builder;
/// only their Host-independent schema uses the shared declaration compiler.
pub fn expand_builtin(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    let args = match parse_args(attr) {
        Ok(ModuleElementArgs::Schema {
            name,
            measurement,
            text_style,
            commands,
        }) => (name, measurement, text_style, commands),
        Ok(ModuleElementArgs::Legacy(name)) => {
            return syn::Error::new(
                name.span(),
                "#[builtin_element] requires `name = ...` and `measurement = ...`",
            )
            .to_compile_error();
        }
        Err(error) => return error.to_compile_error(),
    };
    let input: ItemFn = match parse2(item) {
        Ok(function) => function,
        Err(error) => return error.to_compile_error(),
    };
    let visibility = &input.vis;
    let signature = &input.sig;
    if !signature.generics.params.is_empty() {
        return syn::Error::new(
            signature.generics.span(),
            "#[builtin_element] does not support generic parameters",
        )
        .to_compile_error();
    }

    let mut props = Vec::new();
    for argument in &signature.inputs {
        let typed = match argument {
            FnArg::Typed(typed) => typed,
            FnArg::Receiver(receiver) => {
                return syn::Error::new(
                    receiver.span(),
                    "#[builtin_element] does not support method receivers",
                )
                .to_compile_error();
            }
        };
        let ident = match &*typed.pat {
            Pat::Ident(ident) => ident.ident.clone(),
            pattern => {
                return syn::Error::new(
                    pattern.span(),
                    "#[builtin_element] parameters must be plain identifiers",
                )
                .to_compile_error();
            }
        };
        let ty = (*typed.ty).clone();
        let kind = match classify(&ident, &ty) {
            Ok(kind) => kind,
            Err(error) => return error.to_compile_error(),
        };
        props.push(Prop { ident, ty, kind });
    }

    let schema_mod = format_ident!("{}_schema", signature.ident);
    let definition = SchemaDefinition {
        name: &args.0,
        measurement: &args.1,
        text_style: args.2,
        commands: &args.3,
    };
    match schema_tokens(&schema_mod, visibility, &definition, &props, false) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}

fn schema_tokens(
    schema_mod: &Ident,
    visibility: &Visibility,
    definition: &SchemaDefinition<'_>,
    props: &[Prop],
    named_provider: bool,
) -> syn::Result<TokenStream2> {
    let SchemaDefinition {
        name,
        measurement,
        text_style,
        commands: command_declarations,
    } = definition;
    let measurement_name = measurement.to_string();
    if !matches!(
        measurement_name.as_str(),
        "None" | "Text" | "ReplacedContent" | "Custom"
    ) {
        return Err(syn::Error::new(
            measurement.span(),
            "measurement must be one of `None`, `Text`, `ReplacedContent`, or `Custom`",
        ));
    }

    let child_policy = if props
        .iter()
        .any(|prop| matches!(prop.kind, PropKind::TextChildren))
    {
        quote! { ::whisker::ChildPolicy::PlainText }
    } else if props
        .iter()
        .any(|prop| matches!(prop.kind, PropKind::Children))
    {
        quote! { ::whisker::ChildPolicy::Elements }
    } else {
        quote! { ::whisker::ChildPolicy::None }
    };
    let mut property_index = 0_u32;
    let mut event_index = 0_u32;
    let mut constants = Vec::new();
    let mut properties = Vec::new();
    let mut events = Vec::new();
    let mut commands = Vec::new();

    for prop in props {
        match &prop.kind {
            PropKind::Attr { inner } => {
                property_index += 1;
                let constant =
                    format_ident!("{}_PROPERTY", prop.ident.to_string().to_ascii_uppercase());
                let property_name = prop.ident.to_string().replace('_', "-");
                let value_kind = element_value_kind(inner)?;
                let id = property_index;
                constants.push(quote! {
                    pub const #constant: ::whisker::PropertyId =
                        ::whisker::PropertyId::new(#id).unwrap();
                });
                properties.push(quote! {
                    ::whisker::ElementPropertySchema {
                        property: #constant,
                        name: #property_name.into(),
                        value: ::whisker::ElementValueKind::#value_kind,
                    }
                });
            }
            PropKind::EventNoPayload { event } | PropKind::EventTyped { event, .. } => {
                event_index += 1;
                let constant = format_ident!("{}_EVENT", event.to_ascii_uppercase());
                let id = event_index;
                constants.push(quote! {
                    pub const #constant: ::whisker::EventId =
                        ::whisker::EventId::new(#id).unwrap();
                });
                events.push(quote! {
                    ::whisker::ElementEventSchema {
                        event: #constant,
                        name: #event.into(),
                        detail: ::std::option::Option::None,
                    }
                });
            }
            PropKind::Style | PropKind::Children | PropKind::TextChildren => {}
        }
    }

    let mut command_names = std::collections::HashSet::new();
    for (index, (command_name, value_kind)) in command_declarations.iter().enumerate() {
        if command_name.value().trim().is_empty() {
            return Err(syn::Error::new(
                command_name.span(),
                "command name must not be empty",
            ));
        }
        if !command_names.insert(command_name.value()) {
            return Err(syn::Error::new(
                command_name.span(),
                "duplicate command name",
            ));
        }
        let kind = value_kind.to_string();
        if !matches!(
            kind.as_str(),
            "Null" | "Bool" | "Int" | "Float" | "String" | "Bytes" | "Array" | "Map"
        ) {
            return Err(syn::Error::new(
                value_kind.span(),
                "command ValueKind must be Null, Bool, Int, Float, String, Bytes, Array, or Map",
            ));
        }
        let id = u32::try_from(index + 1).expect("command list fits u32");
        let constant = format_ident!(
            "{}_COMMAND",
            command_name
                .value()
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    '_'
                })
                .collect::<String>()
        );
        constants.push(quote! {
            pub const #constant: ::whisker::CommandId =
                ::whisker::CommandId::new(#id).unwrap();
        });
        commands.push(quote! {
            ::whisker::ElementCommandSchema {
                command: #constant,
                name: #command_name.into(),
                arguments: ::whisker::ElementValueKind::#value_kind,
            }
        });
    }

    let provider = named_provider.then(|| {
        quote! {
            /// Builds the custom element metadata registered during bootstrap.
            pub fn element_provider() -> ::whisker::ElementProviderMetadata {
                ::whisker::ElementProviderMetadata::named(schema())
            }

            // Native application binaries collect every linked schema before
            // mounting the first tree. Web composition roots already receive
            // the same definitions explicitly from whisker-cng.
            #[cfg(not(target_arch = "wasm32"))]
            #[::whisker::runtime::__linked_elements::distributed_slice(
                ::whisker::runtime::__linked_elements::LINKED_ELEMENT_PROVIDERS
            )]
            #[linkme(crate = ::whisker::runtime::__linked_elements)]
            static __WHISKER_LINKED_ELEMENT_PROVIDER:
                fn() -> ::whisker::ElementProviderMetadata = element_provider;
        }
    });

    Ok(quote! {
        /// Generated Host-independent schema and binding symbols.
        #visibility mod #schema_mod {
            #(#constants)*

            /// Stable element name shared by Rust authoring and every Host.
            pub const NAME: &str = #name;

            /// Builds the Host-independent element contract.
            pub fn schema() -> ::whisker::ElementSchema {
                ::whisker::ElementSchema {
                    name: NAME.into(),
                    child_policy: #child_policy,
                    measurement: ::whisker::ElementMeasurement::#measurement,
                    text_style: #text_style,
                    properties: ::std::vec![#(#properties),*],
                    events: ::std::vec![#(#events),*],
                    commands: ::std::vec![#(#commands),*],
                }
            }

            #provider
        }
    })
}

fn element_value_kind(ty: &Type) -> syn::Result<Ident> {
    let kind = if type_is(ty, "bool") {
        "Bool"
    } else if [
        "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize",
    ]
    .iter()
    .any(|name| type_is(ty, name))
    {
        "Int"
    } else if type_is(ty, "f32") || type_is(ty, "f64") {
        "Float"
    } else if type_is(ty, "String") || type_is(ty, "str") {
        "String"
    } else {
        return Err(syn::Error::new(
            ty.span(),
            "cannot infer the WhiskerValue shape for this property type; use bool, an integer, a float, or String",
        ));
    };
    Ok(Ident::new(kind, ty.span()))
}

struct Prop {
    ident: Ident,
    ty: Type,
    kind: PropKind,
}

enum PropKind {
    /// `style: …` — the style prop. Always lowered to a
    /// `::whisker::Style` field (ignoring the authored type) and
    /// routed through `::whisker::apply_style`, exactly like a
    /// built-in element's `style:`.
    Style,
    /// Plain attribute — `Signal<T>` or `T`, name not in the special
    /// list. Routed through `apply_attr` with the kebab-cased name.
    Attr { inner: Type },
    /// Children prop. Either `Children` directly (`Rc<dyn Fn() -> View>`)
    /// or any other type the user names `children`. The macro
    /// attaches the resulting View to the element after all attribute
    /// writes.
    Children,
    /// Plain-text children. The authoring value is still React-like `View`
    /// data, but core rejects element children and lowers text fragments to
    /// one `SetText` operation.
    TextChildren,
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

/// Event names owned by the common Rust input and motion pipelines. Element
/// modules must use distinct names for provider-specific events.
const RESERVED_EVENT_NAMES: &[&str] = &[
    "tap",
    "click",
    "touchstart",
    "touch_start",
    "touchmove",
    "touch_move",
    "touchend",
    "touch_end",
    "touchcancel",
    "touch_cancel",
    "animationstart",
    "animation_start",
    "animationend",
    "animation_end",
    "animationcancel",
    "animation_cancel",
    "animationiteration",
    "animation_iteration",
    "transitionstart",
    "transition_start",
    "transitionend",
    "transition_end",
    "transitioncancel",
    "transition_cancel",
];

/// Props supplied by `ElementBuilder` for every element. They are Rust-side
/// authoring features rather than component-specific Host schema entries.
///
/// `style` and `children` are intentionally absent: structured `style` is
/// always excluded from the element schema, while the declared children type
/// determines its child policy.
const COMMON_ELEMENT_PROP_NAMES: &[&str] = &[
    "id",
    "dataset",
    "accessibility",
    "on_tap",
    "on_tap_catch",
    "on_capture_tap",
    "on_capture_tap_catch",
    "on_longpress",
    "on_longpress_catch",
    "on_capture_longpress",
    "on_capture_longpress_catch",
    "on_click",
    "on_click_catch",
    "on_capture_click",
    "on_capture_click_catch",
    "on_touchstart",
    "on_touchstart_catch",
    "on_capture_touchstart",
    "on_capture_touchstart_catch",
    "on_touchmove",
    "on_touchmove_catch",
    "on_capture_touchmove",
    "on_capture_touchmove_catch",
    "on_touchend",
    "on_touchend_catch",
    "on_capture_touchend",
    "on_capture_touchend_catch",
    "on_touchcancel",
    "on_touchcancel_catch",
    "on_capture_touchcancel",
    "on_capture_touchcancel_catch",
    "on_animationstart",
    "on_animationend",
    "on_animationcancel",
    "on_animationiteration",
    "on_transitionstart",
    "on_transitionend",
    "on_transitioncancel",
    "child",
    "bind_ref",
];

fn classify(ident: &Ident, ty: &Type) -> syn::Result<PropKind> {
    let name = ident.to_string();

    if name == "children" {
        return Ok(if type_is(ty, "TextChildren") {
            PropKind::TextChildren
        } else {
            PropKind::Children
        });
    }

    if COMMON_ELEMENT_PROP_NAMES.contains(&name.as_str()) {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "#[whisker::module_element]: `{name}` is supplied by the common ElementBuilder API and must not be declared as a component-specific property or event"
            ),
        ));
    }

    // The `on_<event>` naming convention picks out handlers; the
    // declared TYPE then decides the payload — `()` ignores it, any
    // other type is deserialized from the event body via `bind_typed`.
    if let Some(event) = name.strip_prefix("on_") {
        if event.is_empty() {
            return Err(syn::Error::new(
                ident.span(),
                "#[whisker::module_element]: event prop name `on_` is empty; \
                 use e.g. `on_press: ()` or `on_input: TouchEvent`",
            ));
        }
        let event = event.to_string();
        if RESERVED_EVENT_NAMES.contains(&event.as_str()) {
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "#[whisker::module_element]: event name `{event}` collides with a \
                     common Whisker input or motion event. Rename it to a \
                     non-reserved, module-specific name — e.g. `on_{event}_gesture`, \
                     `on_press`, `on_page_changed`, or `on_activate`.",
                ),
            ));
        }
        if is_unit_type(ty) {
            return Ok(PropKind::EventNoPayload { event });
        }
        // `E` must be `serde::Deserialize`, enforced at the
        // `bind_typed` call site.
        return Ok(PropKind::EventTyped {
            event,
            payload: ty.clone(),
        });
    }

    // The authored type of a `style` prop is ignored — always a
    // `::whisker::Style` field plus an `apply_style` call.
    if name == "style" {
        return Ok(PropKind::Style);
    }

    // Strip any `Signal<…>` wrapper so the `apply_attr` turbofish picks
    // the ToString-able payload rather than the wrapped Signal.
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

fn prop_struct_field(p: &Prop) -> TokenStream2 {
    let i = &p.ident;
    match &p.kind {
        PropKind::Style => {
            quote! { pub #i: ::whisker::Style }
        }
        PropKind::Attr { .. } => {
            let t = &p.ty;
            quote! { pub #i: #t }
        }
        PropKind::Children => {
            quote! { pub #i: ::whisker::runtime::view::Children }
        }
        PropKind::TextChildren => {
            quote! { pub #i: ::whisker::runtime::view::TextChildren }
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
        PropKind::Style => {
            quote! { #i: ::std::option::Option<::whisker::Style> }
        }
        PropKind::Attr { .. } => {
            let t = &p.ty;
            quote! { #i: ::std::option::Option<#t> }
        }
        PropKind::Children => {
            quote! { #i: ::std::option::Option<::whisker::runtime::view::Children> }
        }
        PropKind::TextChildren => {
            quote! { #i: ::std::option::Option<::whisker::runtime::view::TextChildren> }
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
        PropKind::Style => {
            quote! {
                #[allow(unused_mut)]
                pub fn #i(mut self, value: impl ::std::convert::Into<::whisker::Style>) -> Self {
                    self.#i = ::std::option::Option::Some(value.into());
                    self
                }
            }
        }
        PropKind::Attr { .. } => {
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
            // So the setter accepts a `Children` directly.
            quote! {
                #[allow(unused_mut)]
                pub fn #i(mut self, value: ::whisker::runtime::view::Children) -> Self {
                    self.#i = ::std::option::Option::Some(value);
                    self
                }
            }
        }
        PropKind::TextChildren => {
            quote! {
                #[allow(unused_mut)]
                pub fn #i(mut self, value: ::whisker::runtime::view::Children) -> Self {
                    self.#i = ::std::option::Option::Some(
                        ::whisker::runtime::view::TextChildren::new(value)
                    );
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
        PropKind::TextChildren => {
            quote! {
                #i: self.#i.unwrap_or_else(|| {
                    ::whisker::runtime::view::TextChildren::new(
                        ::std::rc::Rc::new(|| ::whisker::runtime::view::View::Empty)
                    )
                })
            }
        }
        // Style/Attr props are optional by default. `Signal<String>`
        // defaults to `Signal::Stored("")` when omitted, matching an
        // undeclared Host attribute.
        // Event handler props stay required because their `dyn Fn`
        // types don't have a sensible default and a missing callback
        // is almost always an author bug.
        PropKind::Style | PropKind::Attr { .. } => {
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
        PropKind::Style => {
            // The style prop is a `::whisker::Style` value; route it
            // through the same `apply_style` sink built-in elements
            // use (Static → set-once, Dynamic → effect-wrapped).
            quote! {
                ::whisker::apply_style(__handle, props.#i);
            }
        }
        PropKind::Attr { inner } => {
            let attr_name = name.replace('_', "-");
            if type_is(inner, "bool") {
                quote! {
                    ::whisker::runtime::view::apply_attr_bool(__handle, #attr_name, props.#i);
                }
            } else if type_is(inner, "i32") {
                quote! {
                    ::whisker::runtime::view::apply_attr_int(__handle, #attr_name, props.#i);
                }
            } else if type_is(inner, "f64") {
                quote! {
                    ::whisker::runtime::view::apply_attr_f64(__handle, #attr_name, props.#i);
                }
            } else {
                quote! {
                    ::whisker::runtime::view::apply_attr::<_, #inner>(__handle, #attr_name, props.#i);
                }
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
        PropKind::TextChildren => {
            quote! {
                ::whisker::runtime::view::mount_text_children(&props.#i, __handle);
            }
        }
    }
}

fn type_is(ty: &Type, expected: &str) -> bool {
    matches!(
        ty,
        Type::Path(path)
            if path.qself.is_none()
                && path.path.segments.last().is_some_and(|segment| {
                    segment.ident == expected
                        && matches!(segment.arguments, PathArguments::None)
                })
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_builder_props_cannot_enter_a_component_schema() {
        for name in [
            "id",
            "dataset",
            "accessibility",
            "on_tap",
            "on_longpress",
            "on_longpress_catch",
            "on_capture_longpress",
            "on_capture_longpress_catch",
            "on_animationend",
        ] {
            let ident: Ident = syn::parse_str(name).unwrap();
            let ty: Type = syn::parse_str("String").unwrap();
            let error = classify(&ident, &ty).err().expect("must be rejected");
            assert!(error.to_string().contains("common ElementBuilder API"));
        }
    }

    #[test]
    fn schema_special_fields_remain_supported() {
        let style: Ident = syn::parse_str("style").unwrap();
        let style_ty: Type = syn::parse_str("whisker::Style").unwrap();
        assert!(matches!(classify(&style, &style_ty), Ok(PropKind::Style)));

        let children: Ident = syn::parse_str("children").unwrap();
        let children_ty: Type = syn::parse_str("whisker::Children").unwrap();
        assert!(matches!(
            classify(&children, &children_ty),
            Ok(PropKind::Children)
        ));

        let text_children_ty: Type = syn::parse_str("whisker::TextChildren").unwrap();
        assert!(matches!(
            classify(&children, &text_children_ty),
            Ok(PropKind::TextChildren)
        ));
    }
}
