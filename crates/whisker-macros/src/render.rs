//! `render!` macro — compose-style, kwarg-only DSL.
//!
//! ```text
//! root      := node
//! node      := IDENT ( '(' kwargs ')' )? ( '{' children '}' )?
//! kwargs    := IDENT ':' expr ( ',' IDENT ':' expr )* ','?
//!            | IDENT                           # partial (mid-typing)
//! children  := node*
//! ```
//!
//! Each `node` is classified by its leading ident:
//!
//! - Built-in tag (`view`, `page`, `text`, `raw_text`, `image`,
//!   `scroll_view`) → lowered to a builder chain
//!   `::whisker::__tags::__<tag>_ctor().style(…).…__h()`.
//! - `Show` / `For` → lowered to the matching `whisker::show` /
//!   `whisker::for_each` helper.
//! - Anything else → user `#[component]` invocation; lowered to
//!   `name(<Name>Props::builder().k(v)…build())`.
//!
//! ## Children-block restriction
//!
//! Every item in a `{ … }` children block MUST be node-shaped
//! (`IDENT(kwargs?) { … }?`). Bare string literals and bare
//! `{expr}` blocks are rejected with a hard parser error.
//!
//! Why: RA experiments (kept as integration tests in
//! `tests/ra_completion.rs`) showed that
//! rust-analyzer's input fixup gives up on children blocks that
//! contain anything other than `IDENT(name: value, …)` shapes at
//! their top level. With a bare `"hi"` or `{count}` present, the
//! sibling element's kwarg-position completion stops working — no
//! emission-side workaround helped. The fix is to forbid those
//! shapes at the DSL level so the block stays on RA's happy path.
//!
//! For text content, use a kwarg-styled element:
//!
//! ```ignore
//! render! {
//!     view(style: "...") {
//!         text(value: "Hello")
//!         text(value: format!("count: {}", c.get()))
//!     }
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    braced,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream, Result},
    token, Expr, Ident, LitStr, Token,
};

pub fn expand(input: TokenStream) -> TokenStream {
    let tokens: TokenStream2 = input.into();
    match syn::parse2::<Root>(tokens) {
        Ok(root) => root.to_tokens().into(),
        Err(err) => {
            // Pair the compile_error with a same-typed placeholder
            // so the surrounding code (`let h: Element = render!
            // { … };`) keeps type-checking and diagnostics stay
            // confined to the actual syntax error. Same approach
            // leptos uses for its `view!` macro.
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

/// Test-only hook for the unit tests at the bottom of this file.
#[cfg(test)]
fn expand_test(input: TokenStream2) -> TokenStream2 {
    let root: Root = syn::parse2(input).expect("test input must parse");
    root.to_tokens()
}

// ---- AST ----------------------------------------------------------------

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
    ControlFlow(ControlFlowNode),
    UserComponent(UserComponentNode),
}

struct ElementNode {
    tag: Ident,
    kwargs: Vec<Kwarg>,
    children: Vec<Node>,
}

struct ControlFlowNode {
    name: Ident,
    kwargs: Vec<Kwarg>,
    children: Vec<Node>,
}

struct UserComponentNode {
    /// PascalCase ident the call site resolves to. `#[component]`
    /// emits a `pub use __<name>_inner::<fn> as <PascalCase>;`
    /// alias under this name; the snake_case fn itself lives
    /// inside the inner module and isn't reachable from outer
    /// scope, so the emission MUST go through this alias.
    alias_ident: Ident,
    /// PascalCase Props struct ident (`<name>Props`). Same span
    /// as the render! token so RA's go-to-definition lands here.
    props_ident: Ident,
    kwargs: Vec<Kwarg>,
    children: Vec<Node>,
}

struct Kwarg {
    name: Ident,
    value: Expr,
    /// `true` when the user hasn't typed `:` + an expression yet
    /// (cursor sits at the end of the kwarg name). The builder-
    /// chain emitter routes these through `.#name(())` so RA's
    /// method completion can fire on the partial prefix.
    partial: bool,
}

impl Parse for Node {
    fn parse(input: ParseStream) -> Result<Self> {
        // Every node starts with an ident. Reject anything else at
        // this position with a targeted error message — that's the
        // only way to give a useful hint when the user writes bare
        // `"hi"` or `{ count }` as a child.
        if input.peek(LitStr) {
            let lit: LitStr = input.parse()?;
            return Err(syn::Error::new(
                lit.span(),
                "bare string literals are not allowed in render!; \
                 use `text(value: \"…\")` to render text content",
            ));
        }
        if input.peek(token::Brace) {
            // Take the span from the brace itself for a clean
            // arrow in the diagnostic.
            let body;
            let _ = braced!(body in input);
            let _ = body; // discard contents — we're erroring out
            return Err(input.error(
                "bare `{expr}` blocks are not allowed in render!; \
                 use `text(value: <expr>)` to render dynamic text",
            ));
        }

        let tag: Ident = input.parse()?;
        let mut kwargs = Vec::new();
        if input.peek(token::Paren) {
            let body;
            parenthesized!(body in input);
            while !body.is_empty() {
                // `Ident::peek_any` / `Ident::parse_any` admits
                // raw-style identifiers AND Rust keywords. The
                // latter matters for Phase 7-Φ.H.2.4's `ref:` kwarg
                // (the most natural call-site name; `ref` itself
                // is a Rust keyword). Plain `body.peek(Ident)`
                // would reject `ref` outright; `peek_any` lets it
                // through as an `Ident` whose text is `"ref"` and
                // the later `name_str == "ref"` branch routes it
                // to the `.with_ref(...)` setter.
                if !body.peek(syn::Ident::peek_any) {
                    return Err(body.error(
                        "kwargs must be `name: expr` — positional arguments \
                         not allowed",
                    ));
                }
                let name: Ident = body.call(syn::Ident::parse_any)?;
                let (value, partial) = if body.peek(Token![:]) {
                    body.parse::<Token![:]>()?;
                    (body.parse::<Expr>()?, false)
                } else {
                    // Partial — synthesize `()` as a placeholder so
                    // the emitter can still place the method-name
                    // token at the user's source span.
                    let placeholder: Expr = syn::parse_quote_spanned!(name.span()=> ());
                    (placeholder, true)
                };
                kwargs.push(Kwarg {
                    name,
                    value,
                    partial,
                });
                if body.peek(Token![,]) {
                    body.parse::<Token![,]>()?;
                }
            }
        }

        let mut children = Vec::new();
        if input.peek(token::Brace) {
            let body;
            braced!(body in input);
            while !body.is_empty() {
                children.push(body.parse::<Node>()?);
            }
        }

        let name = tag.to_string();
        // Classification by casing + whitelist:
        //
        //   snake_case + in built-in whitelist  → Element (Lynx tag)
        //   PascalCase, name ∈ {"Show", "For"}  → ControlFlow
        //   PascalCase (other)                   → UserComponent
        //                                          (preferred: PascalCase
        //                                          alias from #[component])
        //   snake_case + not in whitelist        → UserComponent
        //                                          (back-compat path:
        //                                          fn name as-is, Props
        //                                          derived from snake_case)
        //
        // The PascalCase form is the canonical convention now (matches
        // React / Leptos / Solid). Snake_case stays parseable so:
        //   (1) mid-typing partials (`vie|`) don't blow up the macro
        //       — RA needs the expansion to succeed for completion,
        //   (2) older code calling snake_case names keeps compiling.
        if is_builtin_tag(&name) {
            Ok(Node::Element(ElementNode {
                tag,
                kwargs,
                children,
            }))
        } else if (name == "Show" || name == "For") && is_pascal_case(&name) {
            Ok(Node::ControlFlow(ControlFlowNode {
                name: tag,
                kwargs,
                children,
            }))
        } else {
            // User component. Derive `alias_ident` (PascalCase —
            // the public re-exported name) and `props_ident`
            // (PascalCase + "Props") from whichever form the user
            // wrote. The snake_case fn itself stays inside the
            // inner module and we never reference it directly
            // from the lowering.
            let span = tag.span();
            let (alias_str, props_str) = if is_pascal_case(&name) {
                (name.clone(), format!("{name}Props"))
            } else {
                let pascal = snake_to_pascal(&name);
                let props = format!("{pascal}Props");
                (pascal, props)
            };
            let alias_ident = Ident::new(&alias_str, span);
            let props_ident = Ident::new(&props_str, span);
            Ok(Node::UserComponent(UserComponentNode {
                alias_ident,
                props_ident,
                kwargs,
                children,
            }))
        }
    }
}

/// `my_card` → `MyCard`. Snake-to-PascalCase for the back-compat
/// snake_case path of user components in `render!`.
fn snake_to_pascal(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = true;
    for c in name.chars() {
        if c == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for u in c.to_uppercase() {
                out.push(u);
            }
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
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

/// `true` if `name`'s first character is ASCII uppercase. Used to
/// route PascalCase idents (user components / control flow) away
/// from the snake_case-only Element path.
fn is_pascal_case(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

// ---- Codegen ------------------------------------------------------------

impl Root {
    fn to_tokens(&self) -> TokenStream2 {
        self.node.to_tokens_returning_handle()
    }
}

impl Node {
    fn to_tokens_returning_handle(&self) -> TokenStream2 {
        match self {
            Node::Element(el) => el.to_tokens(),
            Node::ControlFlow(c) => c.to_tokens(),
            Node::UserComponent(u) => u.to_tokens(),
        }
    }

    /// Emit a `View`-shaped expression. Used by `Show` and
    /// `For`'s children-callback case; each child needs to be
    /// wrapped via `IntoView::into_view(…)` for the helper's
    /// signature.
    fn to_tokens_as_view(&self) -> TokenStream2 {
        let h = self.to_tokens_returning_handle();
        quote! {
            ::whisker::runtime::view::IntoView::into_view(#h)
        }
    }
}

impl ElementNode {
    /// Lower a built-in element to a builder chain on
    /// `::whisker::__tags::<tag>`. Inline-chain form matches the
    /// earlier-experiment-verified `__tags::__view_ctor().style(…).…__h()`
    /// shape — no intermediate `let __h = …; __h` binding when the
    /// element has no children (the let-binding form broke RA's
    /// receiver-type threading; see `tests/ra_completion.rs`).
    fn to_tokens(&self) -> TokenStream2 {
        let tag_ident = &self.tag;
        let tag_name = tag_ident.to_string();
        let tag_span = tag_ident.span();
        let ctor_ident = format_ident!("__{}_ctor", tag_ident, span = tag_span);
        // Inline the entire `::whisker::__tags::__<tag>_ctor()` path
        // directly into the outer `quote!`s below — same layout as
        // earlier-experiment compose_a. Storing it into an intermediate
        // TokenStream and interpolating may capture span/grouping
        // info differently, which we suspect is why kwarg completion
        // worked for compose_a but not for render! (same shape on
        // the surface, different behaviour in practice).

        // One `.kwarg(value)` token group per attr, span-anchored
        // at the user's kwarg-name source position so RA's
        // method-name completion lands on the right token.
        let setter_calls: Vec<TokenStream2> = self
            .kwargs
            .iter()
            .filter_map(|kw| self.kwarg_to_setter(kw))
            .collect();

        // No more ident-ref side block. Every partial kwarg now
        // routes through the setter chain as a method call — see
        // the long comment in `kwarg_to_setter` for the
        // ra-fixup-vs-prefix-match rationale.
        let ident_refs: Vec<TokenStream2> = Vec::new();
        let _ = tag_name;

        // Children: each child becomes a `.child({ inner_chain })`
        // method call on the builder. Same shape verified by the
        // earlier RA experiments.
        let child_calls: Vec<TokenStream2> = self
            .children
            .iter()
            .map(|c| {
                let inner = c.to_tokens_returning_handle();
                quote! { .child(#inner) }
            })
            .collect();

        // No children AND no ident-refs to emit → bare expression
        // form. Matches the earlier-experiment-verified completion shape
        // for the partial-kwarg case.
        if child_calls.is_empty() && ident_refs.is_empty() {
            return quote! {
                {
                    use ::whisker::__tags::ElementBuilder as _;
                    ::whisker::__tags::#ctor_ident() #(#setter_calls)* .__h()
                }
            };
        }

        // Has children or ident-refs → still keep chain inline (no
        // `let __h = … ; __h` binding around it), but add the
        // ident-refs in a side block. The chain itself stays
        // a single expression so RA can thread its receiver type.
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

        if ident_refs.is_empty() {
            quote! {
                {
                    use ::whisker::__tags::ElementBuilder as _;
                    ::whisker::__tags::#ctor_ident() #(#setter_calls)* #(#child_calls)* .__h()
                }
            }
        } else {
            quote! {
                {
                    use ::whisker::__tags::ElementBuilder as _;
                    #ident_refs_block
                    ::whisker::__tags::#ctor_ident() #(#setter_calls)* #(#child_calls)* .__h()
                }
            }
        }
    }

    /// Lower one kwarg to a `.method(value)` token group, or
    /// `None` if this kwarg is partial-with-no-method-match (the
    /// emitter handles those via ident-refs instead).
    fn kwarg_to_setter(&self, kw: &Kwarg) -> Option<TokenStream2> {
        let name = &kw.name;
        let value = &kw.value;
        let name_str = name.to_string();
        let span = name.span();
        let tag_name = self.tag.to_string();

        if name_str == "key" {
            // `key:` is a For-reconciliation hint. Silently ignore
            // on direct elements — matches legacy behaviour.
            return None;
        }

        if kw.partial {
            // ALWAYS emit a method call for partial kwargs. We
            // used to gate this behind a prefix-match heuristic
            // (only emit `.sty(())` if some method name on the
            // builder started with `sty`), falling through to a
            // `let _ = name;` ident-ref otherwise so RA could do
            // identifier completion in the surrounding scope.
            //
            // The heuristic broke RA's macro completion when the
            // partial prefix wasn't a real prefix on its own.
            // Concretely, RA injects a sentinel suffix at the
            // cursor (`sty` becomes `stysomething` during the
            // expansion-for-completion pass) — the prefix check
            // then resolves to `false` and we fall through to
            // the ident-ref path, robbing RA of the method-call
            // shape it needed for kwarg completion. The earlier experiment
            // proves this: `compose_a!` always emits the method
            // call and completes correctly; `render!` used the
            // heuristic and didn't.
            return Some(quote_spanned! {span=> .#name(()) });
        }

        let call = if is_string_attr_method(&tag_name, &name_str) {
            // String-shaped attr (`style`, `class`, plus tag-
            // specific ones like `image::src`,
            // `scroll_view::scroll_orientation`,
            // `raw_text::text`, `text::value`). Phase 7-Φ.B
            // pivot: the builder method now takes
            // `impl Into<Signal<T>>` and handles Static / Dynamic
            // dispatch internally. The macro no longer wraps in
            // `move || …to_string()` — the value flows as-is and
            // type inference picks the correct `From` impl
            // (`From<T>` for static / `From<ReadSignal<T>>` etc.
            // for reactive).
            quote_spanned! {span=> .#name(#value) }
        } else if name_str == "ref" {
            // `ref: <ElementRef>` on a built-in element → bind the ref
            // to this element so its UI methods are invokable after
            // mount. (`ref` is a keyword, so the builder method is
            // `bind_ref`.)
            quote_spanned! {span=> .bind_ref(#value) }
        } else if is_known_event_method(&name_str) {
            // Typed event helper on `ElementBuilder` — `.on_tap(f)`,
            // `.on_longpress(f)`, … — where `f` receives a typed
            // `TouchEvent` / `CustomEvent` / `AnimationEvent`. The
            // closure flows through unchanged; the builder method
            // fixes the event type.
            quote_spanned! {span=> .#name(#value) }
        } else if let Some(event) = strip_on_prefix(&name_str) {
            // Unknown event name → raw `WhiskerValue` escape hatch
            // (`.on("name", |e: WhiskerValue| …)`).
            let event_lit = LitStr::new(&event, span);
            quote_spanned! {span=> .on(#event_lit, #value) }
        } else {
            // Catch-all → `.attr("kebab-name", value)`. The builder's
            // `.attr` accepts `impl Into<Signal<T>>` like the named
            // attr methods; no closure wrapping here either.
            let kebab = name_str.replace('_', "-");
            let kebab_lit = LitStr::new(&kebab, span);
            quote_spanned! {span=>
                .attr(#kebab_lit, #value)
            }
        };
        Some(call)
    }
}

/// String-attribute methods on the builder (take `Fn() -> impl
/// ToString`). The catch-all `.attr(name, …)` path uses the same
/// shape for unknown attrs.
fn is_string_attr_method(tag: &str, attr: &str) -> bool {
    if matches!(attr, "style" | "class") {
        return true;
    }
    matches!(
        (tag, attr),
        ("image", "src")
            | ("scroll_view", "scroll_orientation")
            | ("raw_text", "text")
            | ("text", "value")
    )
}

/// Event kwargs that map to a **typed** `ElementBuilder::on_<event>`
/// method (the closure receives a typed `TouchEvent` /
/// `CustomEvent` / `AnimationEvent`). Any other `on_*` kwarg falls
/// through to the raw `.on("name", |e: WhiskerValue| …)` escape
/// hatch. Mirrors the `on_*` methods on the trait in
/// `whisker::__tags`.
fn is_known_event_method(name: &str) -> bool {
    // Bubble-phase bind variant for every typed event …
    let bind_variant = matches!(
        name,
        "on_tap"
            | "on_longpress"
            | "on_click"
            | "on_touchstart"
            | "on_touchmove"
            | "on_touchend"
            | "on_touchcancel"
            | "on_layoutchange"
            | "on_uiappear"
            | "on_uidisappear"
            | "on_animationstart"
            | "on_animationend"
            | "on_animationcancel"
            | "on_animationiteration"
            | "on_transitionstart"
            | "on_transitionend"
            | "on_transitioncancel"
    );
    // … plus the catch / capture propagation variants for the touch
    // family (`on_tap_catch`, `on_capture_tap`, `on_capture_tap_catch`,
    // …). These map 1:1 to Lynx's `catchtap` / `capture-bindtap` /
    // `capture-catchtap` handler kinds. Mirrors the `on_*` methods on
    // `ElementBuilder` in `whisker::__tags`.
    let propagation_variant = matches!(
        name,
        "on_tap_catch"
            | "on_capture_tap"
            | "on_capture_tap_catch"
            | "on_longpress_catch"
            | "on_capture_longpress"
            | "on_capture_longpress_catch"
            | "on_click_catch"
            | "on_capture_click"
            | "on_capture_click_catch"
            | "on_touchstart_catch"
            | "on_capture_touchstart"
            | "on_capture_touchstart_catch"
            | "on_touchmove_catch"
            | "on_capture_touchmove"
            | "on_capture_touchmove_catch"
            | "on_touchend_catch"
            | "on_capture_touchend"
            | "on_capture_touchend_catch"
            | "on_touchcancel_catch"
            | "on_capture_touchcancel"
            | "on_capture_touchcancel_catch"
    );
    bind_variant || propagation_variant
}

// ---- Control-flow (Show / For) ------------------------------------------

impl ControlFlowNode {
    fn to_tokens(&self) -> TokenStream2 {
        match self.name.to_string().as_str() {
            "Show" => self.emit_show(),
            "For" => self.emit_for(),
            other => unreachable!("ControlFlowNode constructed with name `{other}`"),
        }
    }

    fn kwarg(&self, name: &str) -> Option<&Expr> {
        self.kwargs
            .iter()
            .find(|k| !k.partial && k.name == name)
            .map(|k| &k.value)
    }

    fn emit_show(&self) -> TokenStream2 {
        let Some(when_expr) = self.kwarg("when") else {
            let err = LitStr::new("Show requires `when:` kwarg", self.name.span());
            return quote! { ::std::compile_error!(#err) };
        };

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

// ---- User-component codegen ---------------------------------------------

impl UserComponentNode {
    fn to_tokens(&self) -> TokenStream2 {
        let fn_ident = &self.alias_ident;
        let props_ident = &self.props_ident;

        let setter_calls: Vec<TokenStream2> = self
            .kwargs
            .iter()
            .map(|kw| {
                let name = &kw.name;
                let name_str = name.to_string();
                let span = name.span();
                if kw.partial {
                    // Partial kwarg on a user component → emit
                    // `.name(())` so typed-builder's per-field
                    // setter shows up under RA's method completion.
                    quote_spanned! {span=> .#name(()) }
                } else if name_str == "ref" {
                    // Phase 7-Φ.H.2.4 — `ref:` is the canonical
                    // call-site name for the implicit ElementRef
                    // prop. `ref` is a Rust keyword though, so
                    // typed-builder can't expose `.ref(...)` as a
                    // setter — the module_component macro emits
                    // `.with_ref(...)` instead and we re-route
                    // here.
                    let value = &kw.value;
                    quote_spanned! {span=> .with_ref(#value) }
                } else {
                    let value = &kw.value;
                    quote_spanned! {span=> .#name(#value) }
                }
            })
            .collect();

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

        let children_call = if self.children.is_empty() {
            quote! {}
        } else {
            let child_views: Vec<TokenStream2> = self
                .children
                .iter()
                .map(|c| c.to_tokens_as_view())
                .collect();
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
    use super::{is_builtin_tag, snake_to_pascal, strip_on_prefix};
    use proc_macro2::TokenStream as TokenStream2;

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

    #[test]
    fn builtin_tags_recognised() {
        for t in ["page", "view", "text", "raw_text", "image", "scroll_view"] {
            assert!(is_builtin_tag(t));
        }
    }

    #[test]
    fn non_builtin_lowercase_is_not_builtin() {
        for t in ["card", "my_component", "tab_item", "header"] {
            assert!(!is_builtin_tag(t));
        }
    }

    #[test]
    fn snake_to_pascal_basic() {
        assert_eq!(snake_to_pascal("my_card"), "MyCard");
        assert_eq!(snake_to_pascal("card"), "Card");
        assert_eq!(snake_to_pascal("tab_item"), "TabItem");
    }

    // ---- Compose-syntax parser & emission --------------------------------

    #[test]
    fn view_emission_uses_builder_chain() {
        let input: TokenStream2 = quote::quote! { view(style: "x") };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains("__view_ctor"),
            "view emission must call `__view_ctor()`; output was: {output}"
        );
        assert!(
            output.contains(". style"),
            "view emission must call `.style(value)`; output was: {output}"
        );
        assert!(
            output.contains(". __h ()"),
            "builder chain must finalise with `.__h()`; output was: {output}"
        );
    }

    #[test]
    fn ref_kwarg_on_builtin_routes_to_bind_ref() {
        // `ref:` on a built-in element binds an ElementRef to the
        // element (vs the catch-all `.attr("ref", …)`), so its UI
        // methods (bounding_client_rect, …) are invokable after mount.
        let input: TokenStream2 = quote::quote! { view(ref: my_ref) };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". bind_ref"),
            "ref: on a built-in must emit `.bind_ref(value)`; output was: {output}"
        );
        assert!(
            !output.contains("\"ref\""),
            "ref: must NOT fall through to `.attr(\"ref\", …)`; output was: {output}"
        );
    }

    #[test]
    fn no_children_emits_bare_chain_expression() {
        // Verifies the earlier-experiment-verified "no `let __h =` binding"
        // shape for the no-children case.
        let input: TokenStream2 = quote::quote! { view(style: "x") };
        let output = super::expand_test(input).to_string();
        assert!(
            !output.contains("let __h"),
            "no-children emission must NOT use `let __h = …; __h` binding; \
             output was: {output}"
        );
    }

    #[test]
    fn partial_kwarg_emits_method_call_for_method_prefix() {
        let input: TokenStream2 = quote::quote! { view(sty) };
        let output = super::expand_test(input).to_string();
        eprintln!("EMISSION: {output}");
        assert!(
            output.contains(". sty"),
            "partial kwarg matching method prefix must emit `.sty(())`; \
             output was: {output}"
        );
    }

    #[test]
    fn every_partial_kwarg_emits_method_call() {
        // All partial kwargs route through `.name(())` now —
        // even prefixes that don't match any builder method.
        // (RA injects a sentinel suffix during completion, which
        // makes a "does this prefix match a method" heuristic
        // unreliable; always emitting the method-call shape is
        // what the earlier-experiment compose_a does.)
        let input: TokenStream2 = quote::quote! { view(v) };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". v ("),
            "non-method-matching partial should still emit `.v(())`; \
             output was: {output}"
        );
        assert!(
            !output.contains("let _ = v"),
            "ident-ref side block was dropped — no `let _ = v;` expected; \
             output was: {output}"
        );
    }

    #[test]
    fn bare_string_literal_child_is_rejected() {
        let input: TokenStream2 = quote::quote! { view { "hi" } };
        let result = syn::parse2::<super::Root>(input);
        match result {
            Err(e) => assert!(
                e.to_string().contains("string literals are not allowed"),
                "expected hint about `text(value: \"…\")`; got: {e}"
            ),
            Ok(_) => panic!("bare LitStr child should be a parse error"),
        }
    }

    #[test]
    fn bare_brace_expr_child_is_rejected() {
        let input: TokenStream2 = quote::quote! { view { { count } } };
        let result = syn::parse2::<super::Root>(input);
        match result {
            Err(e) => assert!(
                e.to_string().contains("`{expr}` blocks are not allowed")
                    || e.to_string().contains("text(value:"),
                "expected hint about `text(value: <expr>)`; got: {e}"
            ),
            Ok(_) => panic!("bare `{{expr}}` child should be a parse error"),
        }
    }

    #[test]
    fn positional_arg_is_rejected() {
        let input: TokenStream2 = quote::quote! { text("hi") };
        let result = syn::parse2::<super::Root>(input);
        assert!(result.is_err(), "positional arg should be a parse error");
    }

    #[test]
    fn text_value_kwarg_lowers_to_value_method() {
        let input: TokenStream2 = quote::quote! { text(value: "Hello") };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains("__text_ctor"),
            "text must use the text builder; output was: {output}"
        );
        assert!(
            output.contains(". value"),
            "text(value: …) must lower to `.value(…)`; output was: {output}"
        );
    }

    #[test]
    fn user_component_does_not_use_builtin_tags_module() {
        let input: TokenStream2 = quote::quote! { MyCard(title: "x") };
        let output = super::expand_test(input).to_string();
        assert!(
            !output.contains("__tags"),
            "user components must not touch the built-in tags module; \
             output was: {output}"
        );
        // Emission goes through the PascalCase alias the `#[component]`
        // macro emits — that's how we keep snake_case `my_card` hidden
        // in the inner module from user-call-site completion.
        assert!(
            output.contains("MyCard") && output.contains("MyCardProps"),
            "user component must lower to `MyCard(MyCardProps::builder()…)` \
             — the PascalCase alias is the public call surface; \
             output was: {output}",
        );
    }

    #[test]
    fn snake_case_non_builtin_is_back_compat_user_component() {
        // Snake-case still parses (so mid-typing partials like `my_c|`
        // don't blow up the macro), but the emission still goes
        // through the PascalCase alias derived from the input.
        let input: TokenStream2 = quote::quote! { my_card(title: "x") };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains("MyCard") && output.contains("MyCardProps"),
            "snake_case input should lower to the PascalCase alias call site; \
             output was: {output}",
        );
    }

    #[test]
    fn children_block_emits_child_method() {
        let input: TokenStream2 = quote::quote! {
            view(style: "x") {
                view(class: "y")
            }
        };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". child"),
            "children must lower to `.child({{…}})`; output was: {output}"
        );
    }

    #[test]
    fn nested_children_use_inline_chain() {
        // Verify the chain stays a single expression even with
        // children — no `let __h = …; __h` wrapper.
        let input: TokenStream2 = quote::quote! {
            view(style: "outer") {
                view(class: "inner")
            }
        };
        let output = super::expand_test(input).to_string();
        assert!(
            !output.contains("let __h"),
            "children-bearing emission should stay inline-chain; \
             output was: {output}"
        );
    }
}
