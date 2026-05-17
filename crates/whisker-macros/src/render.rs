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
    Component(ComponentNode),
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
        // IDENT { ... } — element or component depending on case of
        // the first character of the identifier.
        let body = TagBody::parse(input)?;
        if starts_uppercase(&body.tag) {
            Ok(Node::Component(ComponentNode {
                name: body.tag,
                kwargs: body.kwargs,
                children: body.children,
            }))
        } else {
            Ok(Node::Element(ElementNode {
                tag: body.tag,
                attrs: body.kwargs,
                children: body.children,
            }))
        }
    }
}

/// Shared parse result for both element and component nodes: an
/// identifier followed by a brace-delimited block of `name: expr`
/// kwargs and then nested `Node` children.
struct TagBody {
    tag: Ident,
    kwargs: Vec<AttrEntry>,
    children: Vec<Node>,
}

impl TagBody {
    fn parse(input: ParseStream) -> Result<Self> {
        let tag: Ident = input.parse()?;
        let body;
        braced!(body in input);

        let mut kwargs = Vec::new();
        let mut children = Vec::new();

        // kwargs: while we see `IDENT :`, parse name : expr.
        while body.peek(Ident) && body.peek2(Token![:]) {
            let name: Ident = body.parse()?;
            body.parse::<Token![:]>()?;
            let value: Expr = body.parse()?;
            kwargs.push(AttrEntry { name, value });
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        // Children: nested nodes until the closing brace.
        while !body.is_empty() {
            children.push(body.parse()?);
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            tag,
            kwargs,
            children,
        })
    }
}

fn starts_uppercase(ident: &Ident) -> bool {
    ident
        .to_string()
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

struct ElementNode {
    tag: Ident,
    attrs: Vec<AttrEntry>,
    children: Vec<Node>,
}

struct ComponentNode {
    name: Ident,
    kwargs: Vec<AttrEntry>,
    children: Vec<Node>,
}

struct AttrEntry {
    name: Ident,
    value: Expr,
}

// ---- Codegen --------------------------------------------------------------

impl Root {
    fn to_tokens(&self) -> TokenStream2 {
        match &self.node {
            // A bare `{expr}` at the root just evaluates to that
            // expression's value — render! returns whatever it does
            // (typically an `ElementHandle` from a helper). The
            // surrounding scope, not the macro, is responsible for
            // anything else.
            Node::Expr(expr) => quote! { #expr },
            other => other.to_tokens_returning_handle(),
        }
    }
}

impl Node {
    /// Variant of `to_tokens` for the cases that produce a value the
    /// surrounding code can `append_child` directly: elements,
    /// components, and text-literal children. `Node::Expr` does NOT
    /// support this entry point — it's handled specially by
    /// `ElementNode::to_tokens` (where the parent `__h` is in
    /// scope) and by `Root::to_tokens` (where the expression's
    /// value becomes the macro output).
    fn to_tokens_returning_handle(&self) -> TokenStream2 {
        match self {
            Node::Component(c) => c.to_tokens(),
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
            Node::Expr(_) => unreachable!(
                "Node::Expr is handled at ElementNode / Root layer; \
                 should never reach to_tokens_returning_handle"
            ),
        }
    }

    /// Emit a `View`-shaped expression. Used by `Show` (and any
    /// future control-flow component that wraps its body in an
    /// `IntoView`-bearing closure). For element / component / text
    /// children we wrap their handle in `IntoView::into_view(...)`;
    /// for `{expr}` children we trust the user's expression to
    /// implement `IntoView` (which all the supported primitive +
    /// element types do).
    fn to_tokens_as_view(&self) -> TokenStream2 {
        match self {
            Node::Expr(expr) => quote! {
                ::whisker::runtime::view::IntoView::into_view(#expr)
            },
            other => {
                let h = other.to_tokens_returning_handle();
                quote! {
                    ::whisker::runtime::view::IntoView::into_view(#h)
                }
            }
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
                //
                // Rust identifiers can't contain `-`, but Lynx (and
                // HTML / Web Components in general) uses
                // hyphen-separated attribute names like
                // `scroll-orientation`, `enable-scroll`,
                // `safe-area-insets`. Translate `_` → `-` here so
                // users write `scroll_orientation: "horizontal"` and
                // Lynx sees `scroll-orientation`. JSX / Solid /
                // Leptos do equivalent rewrites; the alternative
                // (string-key syntax) is uglier and the choice of
                // separator is locked by the underlying engine.
                let lynx_name: String = name_str.replace('_', "-");
                let attr_name = LitStr::new(&lynx_name, attr.name.span());
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
            match child {
                // `{expr}` children attach themselves through their
                // own effect (which references the enclosing `__h`);
                // we inline the statement and skip the
                // wrap-in-append-child path that other child kinds
                // use.
                Node::Expr(expr) => {
                    stmts.push(quote! {
                        {
                            let __interp_parent = __h;
                            let __interp_last:
                                ::std::rc::Rc<::std::cell::RefCell<
                                    ::std::vec::Vec<
                                        ::whisker::runtime::view::ElementHandle,
                                    >,
                                >> = ::std::rc::Rc::new(
                                    ::std::cell::RefCell::new(
                                        ::std::vec::Vec::new(),
                                    ),
                                );
                            ::whisker::effect(move || {
                                for __h_prev in __interp_last.borrow_mut().drain(..) {
                                    ::whisker::runtime::view::remove_child(
                                        __interp_parent, __h_prev,
                                    );
                                }
                                let __view = ::whisker::runtime::view::IntoView::into_view(#expr);
                                let __new = __view.attach_to(__interp_parent);
                                *__interp_last.borrow_mut() = __new;
                            });
                        }
                    });
                }
                _ => {
                    let child_code = child.to_tokens_returning_handle();
                    stmts.push(quote! {
                        {
                            let __child = #child_code;
                            ::whisker::runtime::view::append_child(__h, __child);
                        }
                    });
                }
            }
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

// ---- Component codegen ----------------------------------------------------

impl ComponentNode {
    fn to_tokens(&self) -> TokenStream2 {
        match self.name.to_string().as_str() {
            "Show" => self.emit_show(),
            "For" => self.emit_for(),
            other => {
                let span = self.name.span();
                let err = LitStr::new(
                    &format!(
                        "unknown component `{other}` in render!. Phase 6.5a v1 \
                         supports `Show` and `For`; user-defined components \
                         are invoked as plain function calls outside the macro."
                    ),
                    span,
                );
                quote! { ::std::compile_error!(#err) }
            }
        }
    }

    fn kwarg(&self, name: &str) -> Option<&Expr> {
        self.kwargs.iter().find(|k| k.name == name).map(|k| &k.value)
    }

    fn emit_show(&self) -> TokenStream2 {
        let Some(when_expr) = self.kwarg("when") else {
            let err = LitStr::new("Show requires `when:` kwarg", self.name.span());
            return quote! { ::std::compile_error!(#err) };
        };

        // Validate the kwarg set so typos surface at compile time.
        for k in &self.kwargs {
            let n = k.name.to_string();
            if n != "when" && n != "fallback" {
                let err = LitStr::new(
                    &format!(
                        "unknown kwarg `{n}` on Show; allowed: when, fallback"
                    ),
                    k.name.span(),
                );
                return quote! { ::std::compile_error!(#err) };
            }
        }

        let children_views: Vec<TokenStream2> =
            self.children.iter().map(|c| c.to_tokens_as_view()).collect();

        // Single-child shortcut: avoid wrapping in a Fragment Vec
        // when the user has exactly one child element.
        let children_body = if children_views.len() == 1 {
            let only = &children_views[0];
            quote! { #only }
        } else {
            quote! {
                ::whisker::runtime::view::View::Fragment(
                    ::std::vec![#(#children_views),*]
                )
            }
        };

        let fallback_arg = match self.kwarg("fallback") {
            Some(expr) => quote! {
                ::std::option::Option::Some(::std::boxed::Box::new({
                    // Hold the user's closure in a local so the wrapper
                    // below captures it by move into a Fn() — the user's
                    // closure is already Fn() (re-callable each branch
                    // flip), we just adapt its return type to `View`.
                    let __whisker_user_fallback = #expr;
                    move || ::whisker::runtime::view::IntoView::into_view(
                        __whisker_user_fallback()
                    )
                }))
            },
            None => quote! { ::std::option::Option::<
                ::std::boxed::Box<dyn ::std::ops::Fn() -> ::whisker::runtime::view::View>,
            >::None },
        };

        quote! {
            ::whisker::show(
                #when_expr,
                move || #children_body,
                #fallback_arg,
            )
        }
    }

    fn emit_for(&self) -> TokenStream2 {
        let Some(each_expr) = self.kwarg("each") else {
            let err = LitStr::new("For requires `each:` kwarg", self.name.span());
            return quote! { ::std::compile_error!(#err) };
        };
        let Some(key_expr) = self.kwarg("key") else {
            let err = LitStr::new("For requires `key:` kwarg", self.name.span());
            return quote! { ::std::compile_error!(#err) };
        };
        let Some(children_expr) = self.kwarg("children") else {
            let err = LitStr::new("For requires `children:` kwarg", self.name.span());
            return quote! { ::std::compile_error!(#err) };
        };

        for k in &self.kwargs {
            let n = k.name.to_string();
            if n != "each" && n != "key" && n != "children" {
                let err = LitStr::new(
                    &format!(
                        "unknown kwarg `{n}` on For; allowed: each, key, children"
                    ),
                    k.name.span(),
                );
                return quote! { ::std::compile_error!(#err) };
            }
        }

        if !self.children.is_empty() {
            let err = LitStr::new(
                "For takes no positional children; pass them via `children:`",
                self.name.span(),
            );
            return quote! { ::std::compile_error!(#err) };
        }

        quote! {
            ::whisker::for_each(
                #each_expr,
                #key_expr,
                {
                    let __whisker_user_children = #children_expr;
                    move |__item| ::whisker::runtime::view::IntoView::into_view(
                        __whisker_user_children(__item)
                    )
                },
            )
        }
    }
}

// ---- Element codegen helpers ---------------------------------------------

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
