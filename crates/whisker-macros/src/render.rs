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
use quote::{format_ident, quote, quote_spanned};
use syn::{
    braced,
    parse::{Parse, ParseStream, Result},
    token, Expr, Ident, LitStr, Token,
};

pub fn expand(input: TokenStream) -> TokenStream {
    let tokens: TokenStream2 = input.into();
    match syn::parse2::<Root>(tokens) {
        Ok(root) => root.to_tokens().into(),
        Err(err) => {
            // Don't just emit `compile_error!(…)` — that expands to
            // nothing of `ElementHandle` type, so the surrounding
            // code (`let h: ElementHandle = render! { … };`) gets a
            // cascading "expected ElementHandle, found ()" error
            // for every line that touches the variable. Pair the
            // error message with a placeholder `create_element` so
            // the macro's return type stays `ElementHandle` and
            // diagnostics stay confined to the actual syntax error
            // — same approach Leptos uses for its `view!` macro.
            //
            // The placeholder never runs: `compile_error!` aborts
            // `cargo build`, and the placeholder exists only to
            // keep rust-analyzer's type-checker happy.
            let err_tokens = err.to_compile_error();
            quote! {
                {
                    #err_tokens
                    ::whisker::runtime::view::create_element(
                        ::whisker::ElementTag::View,
                    )
                }
            }
            .into()
        }
    }
}

/// Test-only hook: same parse + lowering as `expand` but works on
/// `proc_macro2::TokenStream` so unit tests can drive it without
/// going through `proc_macro::TokenStream` (which needs the real
/// compiler context).
#[cfg(test)]
fn expand_test(input: TokenStream2) -> TokenStream2 {
    let root: Root = syn::parse2(input).expect("test input must parse");
    root.to_tokens()
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
    /// Built-in element (`view`, `text`, `image`, …). Lowered to
    /// `view::create_element` + `set_attribute` + `append_child`.
    Element(ElementNode),
    /// Built-in control-flow component (`Show`, `For`). PascalCase
    /// idents that aren't `Show`/`For` are a compile error.
    ControlFlow(ControlFlowNode),
    /// User-defined `#[component]` invocation: lowercase ident NOT in
    /// the built-in whitelist. Lowered to
    /// `fn_name(FnNameProps::builder().kwarg(value)…build())`.
    UserComponent(UserComponentNode),
    Text(LitStr),
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
        // JSX-style element: `<tag attrs… />` or `<tag attrs…>children</tag>`.
        if input.peek(Token![<]) {
            return parse_jsx_element(input);
        }
        Err(input.error(
            "expected `<tag … />`, `<tag …>…</tag>`, a string literal, or `{expr}`",
        ))
    }
}

/// Parse one JSX-style element. Handles both self-closing
/// (`<tag attrs… />`) and open-close (`<tag attrs…>children</tag>`)
/// forms. Attributes are `name="string"` or `name={expr}` (with
/// `name` alone tolerated for in-flight typing — emitted as
/// partial). The closing tag's name must match the opening tag.
fn parse_jsx_element(input: ParseStream) -> Result<Node> {
    input.parse::<Token![<]>()?;
    let tag: Ident = input.parse()?;
    let mut attrs: Vec<AttrEntry> = Vec::new();

    // Attribute loop. Stop when we hit `>` (open-tag end) or `/>`
    // (self-close). Everything else is a kwarg.
    while !input.peek(Token![>]) && !(input.peek(Token![/]) && input.peek2(Token![>])) {
        if input.is_empty() {
            return Err(input.error("unexpected end of input inside `<tag …`; \
                                    expected `>` or `/>` to close the open tag"));
        }
        attrs.push(parse_jsx_attr(input)?);
    }

    // Self-closing path.
    if input.peek(Token![/]) {
        input.parse::<Token![/]>()?;
        input.parse::<Token![>]>()?;
        return Ok(make_node(tag, attrs, Vec::new()));
    }

    // Open-tag end, then children, then `</tag>`.
    input.parse::<Token![>]>()?;
    let mut children: Vec<Node> = Vec::new();
    while !is_close_tag_start(input) {
        if input.is_empty() {
            return Err(input.error(format!(
                "unexpected end of input inside `<{tag}>…`; \
                 expected closing `</{tag}>`",
            )));
        }
        children.push(input.parse()?);
    }
    // Consume the closing `</tag>`.
    input.parse::<Token![<]>()?;
    input.parse::<Token![/]>()?;
    let close_tag: Ident = input.parse()?;
    if close_tag != tag {
        return Err(syn::Error::new(
            close_tag.span(),
            format!(
                "mismatched closing tag: expected `</{tag}>`, found `</{close_tag}>`",
            ),
        ));
    }
    input.parse::<Token![>]>()?;

    Ok(make_node(tag, attrs, children))
}

fn make_node(tag: Ident, attrs: Vec<AttrEntry>, children: Vec<Node>) -> Node {
    let name = tag.to_string();
    if is_builtin_tag(&name) {
        Node::Element(ElementNode {
            tag,
            attrs,
            children,
        })
    } else if name == "Show" || name == "For" {
        Node::ControlFlow(ControlFlowNode {
            name: tag,
            kwargs: attrs,
            children,
        })
    } else {
        // snake_case or anything else → user component.
        // PascalCase user components are accepted here too; if the
        // referenced identifier doesn't resolve, rustc/RA reports
        // the missing-fn error at the call site, which is more
        // localised than a macro-side compile_error.
        Node::UserComponent(UserComponentNode {
            fn_name: tag,
            kwargs: attrs,
            children,
        })
    }
}

/// Parse a single JSX attribute. Three accepted shapes:
///
/// - `name="literal"` — string literal value
/// - `name={expr}` — Rust expression value (closures, format!, …)
/// - `name` alone — in-flight typing; emitted as a **partial**
///   AttrEntry with a `()` placeholder so the codegen can still
///   surface the name at its source span for RA completion
fn parse_jsx_attr(input: ParseStream) -> Result<AttrEntry> {
    let name: Ident = input.parse()?;
    if !input.peek(Token![=]) {
        // Partial — no `=` yet. Cursor likely sits at the end of
        // the attribute name.
        let value: Expr = syn::parse_quote_spanned!(name.span()=> ());
        return Ok(AttrEntry {
            name,
            value,
            partial: true,
        });
    }
    input.parse::<Token![=]>()?;
    let value = if input.peek(token::Brace) {
        let content;
        braced!(content in input);
        content.parse::<Expr>()?
    } else if input.peek(LitStr) {
        let lit: LitStr = input.parse()?;
        syn::parse_quote!(#lit)
    } else {
        return Err(input.error(
            "attribute value must be a string literal (`name=\"…\"`) \
             or a braced expression (`name={…}`)",
        ));
    };
    Ok(AttrEntry {
        name,
        value,
        partial: false,
    })
}

/// Peek two tokens to detect the start of a closing tag (`</`).
fn is_close_tag_start(input: ParseStream) -> bool {
    input.peek(Token![<]) && input.peek2(Token![/])
}

/// Lowercase identifiers that lower to `view::create_element` calls
/// rather than user component invocations. Matches the `ElementTag`
/// enum + the C bridge's `whisker_bridge_create_element` switch.
fn is_builtin_tag(name: &str) -> bool {
    matches!(
        name,
        "page" | "view" | "text" | "raw_text" | "image" | "scroll_view"
    )
}

/// Shared parse result for both element and component nodes:
/// `IDENT (kwargs)? {children}?`. Kotlin-Compose-shaped syntax:
///
/// - `view(style: "x", on_tap: || {}) { text { "hi" } }` — both
/// - `view(style: "x")` — props only (no children-block needed)
/// - `view { text { "hi" } }` — children only
/// - `view()` / `view {}` / `view` — neither (the bare-`view` form
///   exists for in-flight RA tag-name completion)
///
/// The split into `()` for props and `{}` for children removes the
/// ambiguity the old brace-only syntax had — RA can tell whether
/// the user is typing a kwarg name (method completion on the
/// builder) or a child name (identifier completion in scope)
/// purely from the syntactic delimiter, no heuristics.
// `TagBody` and the Compose-syntax parser path are gone; JSX
// parsing happens in `parse_jsx_element` above.

struct ElementNode {
    tag: Ident,
    attrs: Vec<AttrEntry>,
    children: Vec<Node>,
}

/// Built-in control-flow (`Show`, `For`). Lowered to the matching
/// `show()` / `for_each()` helper call.
struct ControlFlowNode {
    name: Ident,
    kwargs: Vec<AttrEntry>,
    children: Vec<Node>,
}

/// User-defined `#[component]` invocation. Lowered to
/// `fn_name(FnNameProps::builder().k(v)…build())`.
struct UserComponentNode {
    fn_name: Ident,
    kwargs: Vec<AttrEntry>,
    children: Vec<Node>,
}

struct AttrEntry {
    name: Ident,
    value: Expr,
    /// `true` when the parser synthesized `value` because the user
    /// hadn't typed `:` + an expression yet (cursor mid-typing). The
    /// builder-chain emitter uses this to always route partial input
    /// through `.#name(…)` (so rust-analyzer's method completion
    /// fires) instead of the usual `.attr(kebab, …)` fallback.
    partial: bool,
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
            Node::ControlFlow(c) => c.to_tokens(),
            Node::UserComponent(u) => u.to_tokens(),
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
        // All built-in tags lower to a `__tags::<tag>().…__h()`
        // builder method chain. This is the path rust-analyzer's
        // method-completion engine knows how to drive: the user
        // typing `view { sty|` (or `image { sr|`, etc.) ends up on a
        // method-name slot in the expansion, RA infers the receiver
        // type, and offers the builder's methods as completion
        // candidates. The legacy imperative codegen below is now
        // dead for built-in tags; it stays as a defensive fallback
        // in case a future tag forgets to register a builder.
        if is_builtin_tag(&self.tag.to_string()) {
            return self.to_tokens_builder_chain();
        }

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

    /// Lower a built-in element (`view`, `page`, `text`, `image`,
    /// `scroll_view`, `raw_text`) to a builder method chain on the
    /// corresponding `::whisker::__tags::<tag>` type. Each prop
    /// kwarg becomes a method call (`.style(…)`, `.on_tap(…)`, …)
    /// with the method-name token at the user's source span —
    /// that's what lets rust-analyzer offer method completion when
    /// the user types `image { sr|` etc.
    ///
    /// Children stay on the legacy imperative path (append_child /
    /// effect-based interpolation) after the chain finalises with
    /// `.__h()` because the builder API doesn't yet model `{expr}`
    /// remount logic in a chainable shape.
    fn to_tokens_builder_chain(&self) -> TokenStream2 {
        // Build one `.method(value)` token group per attr. The whole
        // group sits at the kwarg name's source span so the user's
        // cursor on `style|` maps to the expansion's `.style(…)`
        // method call.
        let setter_calls: Vec<TokenStream2> = self
            .attrs
            .iter()
            .filter_map(|attr| {
                let name = &attr.name;
                let value = &attr.value;
                let name_str = name.to_string();
                let span = name.span();

                if name_str == "key" {
                    // `key:` is a `For` reconciliation hint only —
                    // silently swallow on direct elements to match
                    // legacy behaviour.
                    return None;
                }

                // Known attribute-shaped methods on `__tags::view`
                // are `style` and `class` — both take a closure
                // returning `impl ToString` so signal-reading
                // expressions stay reactive across effect re-runs.
                // The event method `on_tap` takes the user's handler
                // directly (it's already a closure).
                //
                // Other event-shaped kwargs (`onTap`, `on_swipe`, …)
                // route through the generic `.on(event, handler)`.
                // Anything else falls through to `.attr(kebab, move
                // || value)`.
                // Partial-input case. Two emission shapes available:
                //
                //   - `.#name(())` → method call. RA does
                //     METHOD completion against the builder type.
                //     Surfaces `style`, `on_tap`, etc.
                //   - (handled below as a child fall-through)
                //     `#name()` → function call. RA does
                //     IDENTIFIER completion against scope. Surfaces
                //     `view`, `page`, user components, etc.
                //
                // Decide based on whether the partial prefix could
                // plausibly be a kwarg name on this tag. We look at
                // the static known-method list per built-in tag
                // (`style`, `class`, `on_tap`, `on`, `attr`,
                // `child`, `__h` + tag-specific ones) and treat any
                // prefix that matches a method as a kwarg, falling
                // through to identifier completion for the rest.
                // That gets `view { v|` to suggest `view` (child)
                // while keeping `view { sty|` suggesting `style`
                // (kwarg).
                let tag_name_str = self.tag.to_string();
                if attr.partial {
                    if builder_method_prefix_matches(&tag_name_str, &name_str) {
                        return Some(quote_spanned! {span=>
                            .#name(())
                        });
                    }
                    // Fall through to the child-emission path
                    // below — represented as `None` here so the
                    // setter chain doesn't include this entry.
                    return None;
                }

                let tag_name = self.tag.to_string();
                let call = if is_string_attr_method(&tag_name, &name_str) {
                    // String-shaped attr method (`style`, `class`,
                    // plus tag-specific ones like `image::src`,
                    // `scroll_view::scroll_orientation`,
                    // `raw_text::text`). Wrap the value in a closure
                    // that borrows the captured binding via
                    // `ToString::to_string(&value)` — matches the
                    // legacy imperative emission's borrow-only
                    // re-read pattern, so effect re-runs see the
                    // current binding without moving non-`Copy`
                    // values twice.
                    quote_spanned! {span=>
                        .#name(move || ::std::string::ToString::to_string(&(#value)))
                    }
                } else if name_str == "on_tap" {
                    // Handler is already a closure — pass through.
                    quote_spanned! {span=> .#name(#value) }
                } else if let Some(event) = strip_on_prefix(&name_str) {
                    let event_lit = LitStr::new(&event, span);
                    quote_spanned! {span=> .on(#event_lit, #value) }
                } else {
                    let kebab = name_str.replace('_', "-");
                    let kebab_lit = LitStr::new(&kebab, span);
                    quote_spanned! {span=>
                        .attr(#kebab_lit, move || ::std::string::ToString::to_string(&(#value)))
                    }
                };
                Some(call)
            })
            .collect();

        // Children: reuse the same lowering the legacy path uses, so
        // text children, nested elements, user components, and
        // `{expr}` interpolation all behave identically.
        let mut child_stmts: Vec<TokenStream2> = Vec::new();
        for child in &self.children {
            match child {
                Node::Expr(expr) => {
                    child_stmts.push(quote! {
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
                    child_stmts.push(quote! {
                        {
                            let __child = #child_code;
                            ::whisker::runtime::view::append_child(__h, __child);
                        }
                    });
                }
            }
        }

        // Builder constructor. We use a `__<tag>_ctor` name instead
        // of `<tag>()` so the tag's bare name doesn't appear as a
        // function in scope. Otherwise rust-analyzer parses the
        // user's source `view(s)` as a Rust function call (because
        // `view` is a callable value via the prelude), pegs the
        // cursor at argument-expression position, and offers
        // local-variable completion instead of falling through to
        // macro-expansion completion. By emitting `__view_ctor()`,
        // the `view` identifier in the user's source has no
        // resolved value form, the function-call interpretation
        // fails, and RA uses the builder chain we expand into for
        // method completion.
        let tag_ident = &self.tag;
        let tag_span = self.tag.span();
        let ctor_ident = format_ident!("__{}_ctor", tag_ident, span = tag_span);
        let ctor = quote_spanned! {tag_span=> ::whisker::__tags::#ctor_ident() };
        // Explicit type annotation on the builder binding. The
        // type `__tags::<tag>` is the struct returned by the
        // constructor; RA uses this to know the receiver's methods
        // (`style`, `class`, …) without having to chase the chain
        // through any maybe-invalid `.kwarg(())` call.
        let builder_ty = quote_spanned! {tag_span=> ::whisker::__tags::#tag_ident };

        // Partial-kwarg identifiers that didn't match any builder
        // method get re-emitted as bare identifier references so
        // rust-analyzer's IDENTIFIER completion can fire — picks up
        // built-in tag names (`view`, `text`, …) and user components
        // from the surrounding scope. Without this, e.g.
        // `view { v|` had no expansion site at `v|` and RA gave no
        // suggestions; now `v` appears in a value-expression
        // position and RA offers tag/component candidates starting
        // with the typed prefix.
        let tag_name_str = self.tag.to_string();
        let ident_refs: Vec<TokenStream2> = self
            .attrs
            .iter()
            .filter_map(|attr| {
                if !attr.partial {
                    return None;
                }
                if builder_method_prefix_matches(&tag_name_str, &attr.name.to_string()) {
                    // Handled by the setter chain — no extra
                    // ident-ref needed.
                    return None;
                }
                let name = &attr.name;
                let span = name.span();
                Some(quote_spanned! {span=>
                    let _ = #name;
                })
            })
            .collect();

        let ident_refs_block = if ident_refs.is_empty() {
            quote! {}
        } else {
            quote! {
                #[allow(dead_code, unused_variables, path_statements)]
                {
                    #(#ident_refs)*
                }
            }
        };

        // Inline chain (no intermediate `let __b`). This mirrors
        // the shape user-component emission uses
        // (`chip(ChipProps::builder().lab(()).build())`) — RA's
        // method-completion engine seems to follow inline chains
        // more reliably than chains through an explicitly-annotated
        // local binding for our built-in tag types.
        let _ = builder_ty; // currently unused; kept in case we
                            // need the explicit annotation back.
        quote! {
            {
                let __h = #ctor #(#setter_calls)* .__h();
                #ident_refs_block
                #(#child_stmts)*
                __h
            }
        }
    }
}

/// Static list of builder method names for a given built-in tag.
/// The macro uses this for the partial-input heuristic: a kwarg
/// prefix that matches one of these is treated as a kwarg (method
/// completion); otherwise it's re-emitted as a bare identifier so
/// rust-analyzer offers tag/component completion instead.
///
/// Keep in sync with the impl blocks in
/// `crates/whisker/src/lib.rs::__tags`.
fn builder_methods_for_tag(tag: &str) -> &'static [&'static str] {
    match tag {
        "view" | "page" | "text" => &[
            "style", "class", "on_tap", "on", "attr", "child", "__h",
        ],
        "image" => &[
            "style", "class", "on_tap", "on", "attr", "child", "__h", "src",
        ],
        "scroll_view" => &[
            "style", "class", "on_tap", "on", "attr", "child", "__h",
            "scroll_orientation",
        ],
        "raw_text" => &[
            "style", "class", "on_tap", "on", "attr", "child", "__h", "text",
        ],
        _ => &[],
    }
}

/// Does any builder method on `tag` start with `prefix`? Used by the
/// partial-input emitter to decide whether to emit a method call
/// (for method completion) or a bare identifier reference (for
/// scope completion).
fn builder_method_prefix_matches(tag: &str, prefix: &str) -> bool {
    builder_methods_for_tag(tag)
        .iter()
        .any(|m| m.starts_with(prefix))
}

/// String-attribute-shaped methods on the builder for a given tag.
/// These are the methods that take a closure returning
/// `impl ToString` (`style`, `class`, plus tag-specific ones). The
/// macro routes a matching kwarg through `.#name(move || …)`;
/// non-matches go through `.attr(kebab, …)` (catch-all) or
/// `.on(…)` (events).
fn is_string_attr_method(tag: &str, attr: &str) -> bool {
    // Common to every built-in builder.
    if matches!(attr, "style" | "class") {
        return true;
    }
    // Tag-specific methods that live on individual builders.
    matches!(
        (tag, attr),
        ("image", "src")
            | ("scroll_view", "scroll_orientation")
            | ("raw_text", "text")
    )
}

// ---- Control-flow codegen (Show / For) -----------------------------------

impl ControlFlowNode {
    fn to_tokens(&self) -> TokenStream2 {
        match self.name.to_string().as_str() {
            "Show" => self.emit_show(),
            "For" => self.emit_for(),
            // Parse path ensures we only get Show / For here.
            other => unreachable!("ControlFlowNode constructed with name `{other}`"),
        }
    }

    fn kwarg(&self, name: &str) -> Option<&Expr> {
        self.kwargs
            .iter()
            .find(|k| k.name == name)
            .map(|k| &k.value)
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
                    &format!("unknown kwarg `{n}` on Show; allowed: when, fallback"),
                    k.name.span(),
                );
                return quote! { ::std::compile_error!(#err) };
            }
        }

        let children_views: Vec<TokenStream2> = self
            .children
            .iter()
            .map(|c| c.to_tokens_as_view())
            .collect();

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
                    &format!("unknown kwarg `{n}` on For; allowed: each, key, children"),
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

// ---- User-component codegen (snake_case `#[component]` invocation) -------

impl UserComponentNode {
    fn to_tokens(&self) -> TokenStream2 {
        // Map fn name → Props struct name (`my_component` →
        // `MyComponentProps`). Matches what `#[component]` emits.
        let props_ident = snake_to_props_ident(&self.fn_name);
        let fn_ident = &self.fn_name;

        // One `.kwarg(value)` per attribute. typed-builder's
        // `setter(into)` (which `#[component]` adds by default)
        // handles `&str` → `String` / `i32` → `f64` coercion at the
        // call site, so users write naive values.
        //
        // The `kwarg` name is the parameter name from the user's
        // `fn foo(<kwarg>: T)` — `#[component]` lowers it into a
        // `XxxProps::<kwarg>` field with a setter of the same name.
        let setter_calls: Vec<TokenStream2> = self
            .kwargs
            .iter()
            .map(|attr| {
                let name = &attr.name;
                let value = &attr.value;
                // Set the *whole* `.kwarg(value)` token group to the
                // user's kwarg-name span so rust-analyzer can map
                // the user's `kwarg|` cursor position to this
                // method-call in the expansion, triggering method
                // completion against the auto-generated
                // `XxxPropsBuilder`. Matches how leptos's view!
                // macro preserves prop-name spans on its builder
                // chain — that's what drives RA completion there.
                let span = name.span();
                quote_spanned! {span=> .#name(#value) }
            })
            .collect();

        // Reject `key:` here — it's only meaningful inside `For`'s
        // `children:` callback and would otherwise collide with a
        // user's actual `key` prop if they ever defined one. The
        // For-level filter is in `emit_for`; this one catches
        // accidental top-level usage.
        for kw in &self.kwargs {
            if kw.name == "key" {
                let err = LitStr::new(
                    "`key` is only valid on direct children of `For`, \
                     not on user components",
                    kw.name.span(),
                );
                return quote! { ::std::compile_error!(#err) };
            }
        }

        // Children handling: if any non-kwarg children are present,
        // build them into a `move || View::Fragment(...)` closure and
        // pass as `.children(...)`. typed-builder's default kicks in
        // when no children are given (the `#[component]` macro emits
        // an "empty closure" default for `Children` props), so
        // components that don't declare a `children` prop still work
        // as long as the user doesn't try to nest any children.
        //
        // When the user nests children but the component has no
        // `children` prop, typed-builder produces a compile error
        // ("method `children` not found on the builder") at the
        // call site — clearer than any custom diagnostic we could
        // emit here.
        let children_call = if self.children.is_empty() {
            quote! {}
        } else {
            // Each child is materialised to a `View` via
            // `to_tokens_as_view` (same path `Show` uses). Single-
            // child case skips the Fragment wrapper to keep the
            // expansion lean.
            let child_views: Vec<TokenStream2> =
                self.children.iter().map(|c| c.to_tokens_as_view()).collect();
            let body = if child_views.len() == 1 {
                let only = &child_views[0];
                quote! { #only }
            } else {
                quote! {
                    ::whisker::runtime::view::View::Fragment(
                        ::std::vec![#(#child_views),*]
                    )
                }
            };
            quote! {
                .children(::std::rc::Rc::new(move || { #body }))
            }
        };

        quote! {
            #fn_ident(
                #props_ident::builder()
                    #(#setter_calls)*
                    #children_call
                    .build()
            )
        }
    }
}

/// `my_component` → `MyComponentProps`. Mirror of the same helper in
/// `component.rs`; kept duplicated to avoid a cross-module dep within
/// the proc-macro crate (the modules see entirely different syn
/// types and this conversion is the only thing they share).
fn snake_to_props_ident(fn_name: &Ident) -> Ident {
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
    use super::{is_builtin_tag, snake_to_props_ident, strip_on_prefix};
    use proc_macro2::{Span, TokenStream as TokenStream2};
    use syn::Ident;

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

    // ---- Tag classification ------------------------------------

    #[test]
    fn builtin_tags_recognised() {
        for t in ["page", "view", "text", "raw_text", "image", "scroll_view"] {
            assert!(is_builtin_tag(t), "{t} should be a builtin");
        }
    }

    #[test]
    fn non_builtin_lowercase_is_not_builtin() {
        // These should be classified as user components, not elements.
        for t in [
            "card",
            "my_component",
            "tab_item",
            "now_playing",
            "header",
        ] {
            assert!(!is_builtin_tag(t), "{t} should NOT be a builtin");
        }
    }

    #[test]
    fn user_component_with_underscore_name_is_not_builtin() {
        // Regression guard: it's tempting to special-case the
        // built-in list as "anything with an underscore is a
        // component", but `raw_text` and `scroll_view` are
        // built-ins with underscores. Make sure we don't slip.
        assert!(is_builtin_tag("raw_text"));
        assert!(is_builtin_tag("scroll_view"));
        assert!(!is_builtin_tag("scroll_view_x"));
        assert!(!is_builtin_tag("my_view"));
    }

    // ---- snake→Props ident -------------------------------------

    #[test]
    fn snake_to_props_ident_basic() {
        let id = Ident::new("my_component", Span::call_site());
        assert_eq!(snake_to_props_ident(&id).to_string(), "MyComponentProps");

        let id = Ident::new("card", Span::call_site());
        assert_eq!(snake_to_props_ident(&id).to_string(), "CardProps");

        let id = Ident::new("tab_item", Span::call_site());
        assert_eq!(snake_to_props_ident(&id).to_string(), "TabItemProps");
    }

    // ---- RA hint emission ----------------------------------------

    #[test]
    fn view_emission_uses_builder_chain() {
        // `render! { view { style: "x" } }` must lower to a builder
        // chain `::whisker::__tags::view().style("x").__h()`. The
        // method-call shape (not struct-init) is what drives
        // rust-analyzer's prop-name completion.
        let input: TokenStream2 = quote::quote! { <view style="x" /> };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains("__tags :: __view_ctor ()") || output.contains("__tags::__view_ctor()"),
            "view emission must call `::whisker::__tags::__view_ctor()`; \
             output was: {output}"
        );
        assert!(
            output.contains(". style"),
            "view emission must call `.style(value)`; \
             output was: {output}"
        );
        assert!(
            output.contains(". __h ()"),
            "builder chain must finalise with `.__h()`; \
             output was: {output}"
        );
    }

    #[test]
    fn view_emission_falls_through_unknown_attrs_to_attr_method() {
        // Attributes the builder doesn't have a method for go
        // through `.attr("kebab-name", value)`.
        let input: TokenStream2 = quote::quote! {
            <view scroll_orientation="horizontal" />
        };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". attr ("),
            "unknown attrs should fall through to `.attr(…)`; \
             output was: {output}"
        );
        assert!(
            output.contains("\"scroll-orientation\""),
            "snake_case must be kebab-cased for `.attr(…)`; \
             output was: {output}"
        );
    }

    #[test]
    fn all_builtin_tags_use_builder_chain() {
        // Every built-in lowers to `__tags::<tag>().…__h()`. Spot-
        // check a few representative ones.
        for tag in &["page", "view", "text", "image", "scroll_view"] {
            let input: TokenStream2 = match *tag {
                "image" => quote::quote! { <image src="x" /> },
                "scroll_view" => quote::quote! { <scroll_view scroll_orientation="vertical" /> },
                _ => {
                    let ident = syn::Ident::new(tag, proc_macro2::Span::call_site());
                    quote::quote! { <#ident style="x" /> }
                }
            };
            let output = super::expand_test(input).to_string();
            assert!(
                output.contains("__tags") && output.contains(". __h ()"),
                "tag `{tag}` should use the builder chain; output was: {output}"
            );
        }
    }

    #[test]
    fn image_src_lowers_to_dedicated_method() {
        let input: TokenStream2 = quote::quote! { <image src="https://x" /> };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". src"),
            "image.src should lower to the `.src` builder method, not `.attr`; \
             output was: {output}"
        );
    }

    #[test]
    fn scroll_view_orientation_lowers_to_dedicated_method() {
        let input: TokenStream2 =
            quote::quote! { <scroll_view scroll_orientation="horizontal" /> };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". scroll_orientation"),
            "scroll_view.scroll_orientation should lower to the dedicated method; \
             output was: {output}"
        );
    }

    #[test]
    fn partial_kwarg_matching_method_emits_method_call() {
        // `view { sty|` mid-typing: parser produces partial kwarg
        // `sty`, codegen should still emit `.sty(())` so RA can do
        // method completion against `view`'s builder.
        let input: TokenStream2 = quote::quote! { <view sty /> };
        let output = super::expand_test(input).to_string();
        eprintln!("DUMP partial-sty: {output}");
        assert!(
            output.contains(". sty"),
            "partial kwarg matching method prefix must emit `.sty(())`; \
             output was: {output}"
        );
    }

    #[test]
    fn single_char_partial_kwarg_emits_method_call() {
        let input: TokenStream2 = quote::quote! { <view s /> };
        let output = super::expand_test(input).to_string();
        eprintln!("DUMP partial-s: {output}");
        assert!(
            output.contains(". s ("),
            "view {{ s }} should emit `.s(())`; output was: {output}"
        );
    }

    #[test]
    fn partial_kwarg_with_children_block_still_emits_method_call() {
        // Compose syntax: props in `()`, children separately in
        // `{}`. Partial kwarg in `()` is unambiguous now.
        let input: TokenStream2 = quote::quote! {
            <view s>
                <text>"hi"</text>
            </view>
        };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". s ("),
            "partial `s` in props block should still emit `.s(())` \
             alongside the children-block; output was: {output}"
        );
    }

    #[test]
    fn partial_kwarg_non_matching_emits_ident_ref() {
        // `view { v|` mid-typing: `v` doesn't match any of view's
        // builder methods (`style`, `class`, `on_tap`, …), so the
        // emitter falls through to a bare `let _ = v;` reference
        // for identifier completion.
        let input: TokenStream2 = quote::quote! { <view v /> };
        let output = super::expand_test(input).to_string();
        eprintln!("DUMP partial-v: {output}");
        assert!(
            output.contains("let _ = v"),
            "non-method-matching partial should emit ident-ref; \
             output was: {output}"
        );
    }

    #[test]
    fn user_component_does_not_use_builtin_tags_module() {
        let input: TokenStream2 = quote::quote! { <my_card title="x" /> };
        let output = super::expand_test(input).to_string();
        assert!(
            !output.contains("__tags"),
            "user components must not touch the built-in tags module; \
             output was: {output}"
        );
    }
}
