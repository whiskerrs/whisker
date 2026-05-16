//! `render!` macro — Phase 6.5a (A3) replacement for `rsx!`.
//!
//! Grammar matches `rsx!` (so users migrate by renaming the macro):
//!
//! ```text
//! render_root := node
//! node        := IDENT "{" attr_list child_list "}" | LIT_STR | "{" expr "}"
//! attr_list   := (IDENT ":" expr ",")*
//! child_list  := node*
//! ```
//!
//! What changed from `rsx!`:
//!
//! - **Emit**. `rsx!` produces an `Element` value tree via builder
//!   calls; `render!` produces an imperative block that calls
//!   [`whisker::view`] dispatch functions to construct elements
//!   directly through the installed `DynRenderer`, and returns an
//!   [`ElementHandle`].
//! - **`{expr}` interpolation is not yet supported.** Step 3 of A3
//!   will wrap it in an `effect` for reactivity; until then, using
//!   it is a compile error.
//! - **Event handler closures must be `Fn() + 'static`** (no event
//!   payload). The payload work is a separate stream tracked
//!   outside Phase 6.5a.
//!
//! [`whisker::view`]: ../../../whisker_runtime/view/index.html
//! [`ElementHandle`]: ../../../whisker_runtime/view/struct.ElementHandle.html

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    braced,
    parse::{Parse, ParseStream, Result},
    parse_macro_input, token, Expr, Ident, LitStr, Token,
};

pub fn expand(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as Root);
    root.to_tokens().into()
}

// ---- AST ------------------------------------------------------------------

struct Root {
    node: Node,
}

impl Parse for Root {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            node: input.parse()?,
        })
    }
}

enum Node {
    Element(ElementNode),
    Text(LitStr),
    /// Reserved for Step 3 — currently emits a `compile_error!`.
    Expr(Expr),
}

impl Parse for Node {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(LitStr) {
            return Ok(Node::Text(input.parse()?));
        }
        if input.peek(token::Brace) {
            let content;
            braced!(content in input);
            let expr: Expr = content.parse()?;
            return Ok(Node::Expr(expr));
        }
        Ok(Node::Element(input.parse()?))
    }
}

struct ElementNode {
    tag: Ident,
    attrs: Vec<AttrEntry>,
    children: Vec<Node>,
}

struct AttrEntry {
    name: Ident,
    value: Expr,
}

impl Parse for ElementNode {
    fn parse(input: ParseStream) -> Result<Self> {
        let tag: Ident = input.parse()?;
        let body;
        braced!(body in input);

        let mut attrs = Vec::new();
        let mut children = Vec::new();

        // Attributes: while we see `IDENT :`, parse an attribute. Once
        // we see something else, switch to children.
        while body.peek(Ident) && body.peek2(Token![:]) {
            let name: Ident = body.parse()?;
            body.parse::<Token![:]>()?;
            let value: Expr = body.parse()?;
            attrs.push(AttrEntry { name, value });
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        // Children until we hit the closing brace.
        while !body.is_empty() {
            children.push(body.parse()?);
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            tag,
            attrs,
            children,
        })
    }
}

// ---- Codegen --------------------------------------------------------------

impl Root {
    fn to_tokens(&self) -> TokenStream2 {
        self.node.to_tokens()
    }
}

impl Node {
    fn to_tokens(&self) -> TokenStream2 {
        match self {
            Node::Element(el) => el.to_tokens(),
            Node::Text(lit) => quote! {
                {
                    let __h = ::whisker::runtime::view::create_element(
                        ::whisker::ElementTag::RawText,
                    );
                    ::whisker::runtime::view::set_attribute(__h, "text", #lit);
                    __h
                }
            },
            // `{expr}` inside a text node: render as a raw_text
            // element whose `text` attribute is driven by an effect.
            // The effect's closure reads whatever signals `#expr`
            // touches via `.get()` / `.with()`, so it re-runs (and
            // re-updates the element) on every dependency change.
            // Static (signal-less) expressions just run once at
            // registration and never re-trigger.
            Node::Expr(expr) => quote! {
                {
                    let __h = ::whisker::runtime::view::create_element(
                        ::whisker::ElementTag::RawText,
                    );
                    ::whisker::effect(move || {
                        let __text =
                            ::std::string::ToString::to_string(&(#expr));
                        ::whisker::runtime::view::set_attribute(
                            __h, "text", &__text,
                        );
                    });
                    __h
                }
            },
        }
    }
}

impl ElementNode {
    fn to_tokens(&self) -> TokenStream2 {
        let tag_path = match tag_to_element_tag(&self.tag) {
            Ok(t) => t,
            Err(err) => return err,
        };

        let mut stmts: Vec<TokenStream2> = Vec::new();

        for attr in &self.attrs {
            let value = &attr.value;
            let name_str = attr.name.to_string();

            if name_str == "style" {
                // Wrap the style setter in an effect. A signal-reading
                // expression (e.g. `format!("color: {}", color.get())`)
                // re-runs and re-applies the style on every dep change;
                // a static value just runs the effect once at
                // registration and never re-fires.
                stmts.push(quote! {
                    ::whisker::effect(move || {
                        ::whisker::runtime::view::set_inline_styles(
                            __h, &::std::string::ToString::to_string(&(#value)),
                        );
                    });
                });
            } else if let Some(event) = strip_on_prefix(&name_str) {
                // Event handlers register once and stay until the
                // owner is disposed. Re-registering on every signal
                // change would lose the previous registration — not
                // what users want.
                let event_lit = LitStr::new(&event, attr.name.span());
                stmts.push(quote! {
                    ::whisker::runtime::view::set_event_listener(
                        __h, #event_lit, ::std::boxed::Box::new(#value),
                    );
                });
            } else if name_str == "key" {
                // Keys are a `For` reconciliation hint; Step 4 wires
                // them through. For now silently accept but ignore so
                // existing rsx! callers can rename to render! without
                // touching their templates.
                let _ = value;
            } else {
                // All other attributes are routed through an effect
                // — same uniform-treatment rationale as `style:`
                // above.
                let attr_name = LitStr::new(&name_str, attr.name.span());
                stmts.push(quote! {
                    ::whisker::effect(move || {
                        ::whisker::runtime::view::set_attribute(
                            __h, #attr_name,
                            &::std::string::ToString::to_string(&(#value)),
                        );
                    });
                });
            }
        }

        for child in &self.children {
            let child_code = child.to_tokens();
            stmts.push(quote! {
                {
                    let __child = #child_code;
                    ::whisker::runtime::view::append_child(__h, __child);
                }
            });
        }

        quote! {
            {
                let __h = ::whisker::runtime::view::create_element(#tag_path);
                #(#stmts)*
                __h
            }
        }
    }
}

fn tag_to_element_tag(tag: &Ident) -> std::result::Result<TokenStream2, TokenStream2> {
    let name = tag.to_string();
    let path = match name.as_str() {
        "page" => quote! { ::whisker::ElementTag::Page },
        "view" => quote! { ::whisker::ElementTag::View },
        "text" => quote! { ::whisker::ElementTag::Text },
        "raw_text" => quote! { ::whisker::ElementTag::RawText },
        "image" => quote! { ::whisker::ElementTag::Image },
        "scroll_view" => quote! { ::whisker::ElementTag::ScrollView },
        _ => {
            // Reject everything else for now. List + frame coming
            // when the bridge / ElementTag enum gains the variants;
            // x-* and custom components come when the macro learns
            // the `Component { … }` invocation form (Step 4).
            let span = tag.span();
            let err = LitStr::new(&format!("unknown render! tag `{name}`"), span);
            return Err(quote! { ::std::compile_error!(#err) });
        }
    };
    Ok(path)
}

fn strip_on_prefix(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix("on_") {
        Some(rest.to_string())
    } else if let Some(rest) = name.strip_prefix("on") {
        if let Some(first) = rest.chars().next() {
            if first.is_uppercase() {
                let mut owned = first.to_lowercase().to_string();
                owned.push_str(&rest[first.len_utf8()..]);
                return Some(owned);
            }
        }
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::strip_on_prefix;

    #[test]
    fn strips_snake_case() {
        assert_eq!(strip_on_prefix("on_tap"), Some("tap".into()));
    }

    #[test]
    fn strips_camel_case() {
        assert_eq!(strip_on_prefix("onTap"), Some("tap".into()));
    }

    #[test]
    fn rejects_non_event_prefixes() {
        assert_eq!(strip_on_prefix("tap"), None);
        assert_eq!(strip_on_prefix("ontap"), None);
    }
}
