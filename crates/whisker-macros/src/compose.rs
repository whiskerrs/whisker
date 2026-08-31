//! Common lowering for builder-shaped composition syntax.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use whisker_macro_syntax::compose::{ComposeArgument, ComposeChild, ComposeInput, ComposeNode};

pub fn expand_root(input: TokenStream2) -> TokenStream2 {
    match whisker_macro_syntax::compose::parse_root(input) {
        Ok(root) => node_to_tokens(&root),
        Err(error) => error.to_compile_error(),
    }
}

#[allow(dead_code)]
pub fn expand_many(input: TokenStream2) -> TokenStream2 {
    match syn::parse2::<ComposeInput>(input) {
        Ok(input) => {
            let nodes = input.nodes.iter().map(node_to_tokens);
            quote! { [#(#nodes),*] }
        }
        Err(error) => error.to_compile_error(),
    }
}

pub fn node_to_tokens(node: &ComposeNode) -> TokenStream2 {
    let path = &node.path;
    let arguments = node.arguments.iter().map(argument_to_tokens);
    let body = (node.has_body && !node.body.is_empty()).then(|| body_to_tokens(&node.body));
    quote! {{
        use ::whisker::__element_builder::ElementBuilder as _;
        #path::builder()
            #(#arguments)*
            #body
            .build()
    }}
}

pub fn arguments_to_tokens(
    constructor: TokenStream2,
    arguments: &[ComposeArgument],
) -> TokenStream2 {
    let setters = arguments.iter().map(argument_to_tokens);
    quote! { #constructor #(#setters)* }
}

fn argument_to_tokens(argument: &ComposeArgument) -> TokenStream2 {
    let name = &argument.name;
    let value = &argument.value;
    quote_spanned! {name.span()=> .#name(#value) }
}

fn body_to_tokens(children: &[ComposeChild]) -> TokenStream2 {
    let statements = children.iter().map(|child| match child {
        ComposeChild::Node(node) => {
            let child = node_to_tokens(node);
            quote! { __whisker_body.push(#child); }
        }
        ComposeChild::Text(value) => quote_spanned! {value.span()=> {
            __whisker_body.push(::std::string::String::from(#value));
        }},
        ComposeChild::Expression(expression) => {
            quote! { __whisker_body.push(#expression); }
        }
        ComposeChild::Spread(expression) => {
            quote! { __whisker_body.extend(#expression); }
        }
    });
    quote! {
        .body(move |__whisker_body| { #(#statements)* })
    }
}
