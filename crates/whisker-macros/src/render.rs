//! `render!` macro codegen — compose-style, kwarg-only DSL.
//!
//! The PARSE side (AST + `syn::parse::Parse` impls + classification
//! helpers) lives in the `whisker-macro-syntax` crate so both this
//! codegen AND the `whisker-fmt` formatter can share it. This module
//! holds only the lowering.
//!
//! Because the AST types ([`Root`], [`Node`], …) are now defined in
//! another crate, the orphan rule forbids adding *inherent* methods to
//! them here. The lowering is therefore expressed as FREE FUNCTIONS
//! over `&Root` / `&Node` / `&ElementNode` / `&UserComponentNode`
//! (`root_to_tokens`, `node_to_tokens`, …). The emitted token streams
//! are byte-for-byte identical to the previous inherent-method form, so
//! all existing macro behavior and tests are preserved.
//!
//! See `whisker-macro-syntax/src/render.rs` for the grammar.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::LitStr;
use whisker_macro_syntax::render::{ElementNode, Kwarg, Node, Root, UserComponentNode};

pub fn expand(input: TokenStream) -> TokenStream {
    let tokens: TokenStream2 = input.into();
    match syn::parse2::<Root>(tokens) {
        Ok(root) => root_to_tokens(&root).into(),
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
    root_to_tokens(&root)
}

// ---- Codegen ------------------------------------------------------------

fn root_to_tokens(root: &Root) -> TokenStream2 {
    node_to_tokens_returning_handle(&root.node)
}

fn node_to_tokens_returning_handle(node: &Node) -> TokenStream2 {
    match node {
        Node::Element(el) => element_to_tokens(el),
        Node::UserComponent(u) => user_component_to_tokens(u),
        // `children()` resolves the surrounding `#[component]`'s
        // `children: Children` prop by name. The body of a
        // `#[component]` fn always destructures `children` into
        // scope when the prop is declared, so this is a plain
        // local-ident reference — no closure capture, no `move`,
        // and (crucially) no `Rc::clone` either: `mount_children`
        // takes `&Children` so the outer `FnMut` body can re-run
        // (e.g. hot-reload remount) without `cannot move out`.
        Node::ChildrenSlot { span } => quote_spanned! {*span=>
            ::whisker::runtime::view::mount_children(&children)
        },
    }
}

/// Emit a `View`-shaped expression. Used by `Show` and `For`'s
/// children-callback case; each child needs to be wrapped via
/// `IntoView::into_view(…)` for the helper's signature.
fn node_to_tokens_as_view(node: &Node) -> TokenStream2 {
    let h = node_to_tokens_returning_handle(node);
    quote! {
        ::whisker::runtime::view::IntoView::into_view(#h)
    }
}

/// Lower a built-in element to a builder chain on
/// `::whisker::__tags::<tag>`. The inline-chain form
/// (`__tags::__view_ctor().style(…).…__h()` with no intermediate
/// `let __h = …; __h` binding) is load-bearing: a let-binding breaks
/// RA's receiver-type threading and kills kwarg completion. See
/// `tests/ra_completion.rs`.
fn element_to_tokens(el: &ElementNode) -> TokenStream2 {
    let tag_ident = &el.tag;
    let tag_name = tag_ident.to_string();
    let tag_span = tag_ident.span();
    let ctor_ident = format_ident!("__{}_ctor", tag_ident, span = tag_span);
    // Inline the full `::whisker::__tags::__<tag>_ctor()` path
    // into the outer `quote!`s below. Storing it into an
    // intermediate TokenStream and interpolating captures span /
    // grouping info differently and breaks RA's kwarg completion.

    // One `.kwarg(value)` token group per attr, span-anchored
    // at the user's kwarg-name source position so RA's
    // method-name completion lands on the right token.
    let setter_calls: Vec<TokenStream2> = el
        .kwargs
        .iter()
        .filter_map(|kw| element_kwarg_to_setter(el, kw))
        .collect();

    // Every partial kwarg routes through the setter chain as a
    // method call — see the long comment in `element_kwarg_to_setter`.
    let ident_refs: Vec<TokenStream2> = Vec::new();
    let _ = tag_name;

    // Children: each child becomes a `.child({ inner_chain })`
    // method call on the builder.
    let child_calls: Vec<TokenStream2> = el
        .children
        .iter()
        .map(|c| {
            let inner = node_to_tokens_returning_handle(c);
            quote! { .child(#inner) }
        })
        .collect();

    // No children AND no ident-refs → bare expression form.
    // Keeps the chain on RA's happy path for partial-kwarg
    // completion.
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

/// Lower one kwarg to a `.method(value)` token group, or `None` if this
/// kwarg is partial-with-no-method-match (the emitter handles those via
/// ident-refs instead).
fn element_kwarg_to_setter(el: &ElementNode, kw: &Kwarg) -> Option<TokenStream2> {
    let name = &kw.name;
    let value = &kw.value;
    let name_str = name.to_string();
    let span = name.span();
    let tag_name = el.tag.to_string();

    if name_str == "key" && tag_name != "list" {
        // `key:` on a direct element is a no-op (reconciliation
        // hint with no semantic effect outside keyed lists). The
        // `list` builder has its own typed `key` setter for the
        // keyed-list key extractor closure, so let it through.
        return None;
    }

    if kw.partial {
        // ALWAYS emit a method call for partial kwargs. RA
        // injects a sentinel suffix at the cursor during its
        // expansion-for-completion pass, so any prefix-match
        // heuristic (e.g. "only emit `.sty(())` if some builder
        // method starts with `sty`") sees the suffixed name and
        // returns false, breaking method-name completion.
        return Some(quote_spanned! {span=> .#name(()) });
    }

    let call = if is_known_attr_method(&tag_name, &name_str) {
        // Named builder attribute method (`style`, `class`, the
        // universal trait attrs, and per-tag ones like
        // `scroll_view::bounces`, `text::text_maxline`). The method
        // takes `impl Into<Signal<T>>` for its semantic `T` and
        // handles Static / Dynamic dispatch internally; the value
        // flows as-is and type inference picks the right `From`
        // (`From<T>` static / `From<ReadSignal<T>>` reactive) — so
        // `bounces: true` / `text_maxline: 3` /
        // `mode: ImageMode::AspectFit` all just work.
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

/// Kwargs that map to a **named** builder attribute method
/// (`.#name(value)`) rather than the catch-all `.attr("kebab", value)`.
/// Each method takes `impl Into<Signal<T>>` for its semantic `T`
/// (bool / number / String), so the value flows through unchanged and
/// type inference picks the right `From` — crucial for bool/number
/// attrs, which `.attr` (String-only) couldn't accept. Mirrors the
/// methods on `ElementBuilder` + the per-tag inherent impls in
/// `whisker::__tags`; tag-specific names only match their tag, so using
/// one on the wrong element is a clear "no method" error.
fn is_known_attr_method(tag: &str, attr: &str) -> bool {
    // Universal attributes (the `ElementBuilder` trait — any tag).
    let common = matches!(
        attr,
        "style"
            | "class"
            | "id"
            | "name"
            | "event_through"
            | "exposure_id"
            | "exposure_scene"
            | "exposure_area"
            | "accessibility_label"
            | "accessibility_trait"
            | "accessibility_element"
            | "accessibility_elements"
            | "accessibility_elements_hidden"
            | "accessibility_exclusive_focus"
            | "a11y_id"
            | "user_interaction_enabled"
            | "native_interaction_enabled"
            | "block_native_event"
            | "consume_slide_event"
            | "pan_intercept_direction"
            | "pan_intercept_scope"
            | "hit_slop"
            | "flatten"
    );
    if common {
        return true;
    }
    // Tag-specific inherent attributes.
    matches!(
        (tag, attr),
        ("raw_text", "text")
            | ("text", "value")
            | ("text", "text_maxline")
            | ("text", "text_selection")
            | ("text", "include_font_padding")
            | ("text", "tail_color_convert")
            | ("text", "text_single_line_vertical_align")
            | ("text", "custom_context_menu")
            | ("text", "custom_text_selection")
            | ("scroll_view", "scroll_orientation")
            | ("scroll_view", "bounces")
            | ("scroll_view", "enable_scroll")
            | ("scroll_view", "scroll_bar_enable")
            | ("scroll_view", "initial_scroll_offset")
            | ("scroll_view", "initial_scroll_to_index")
            | ("scroll_view", "upper_threshold")
            | ("scroll_view", "lower_threshold")
            | ("list", "list_type")
            | ("list", "column_count")
            | ("list", "span_count")
            | ("list", "vertical_orientation")
            // Render-props setters on `list` — type-stated, take
            // closure literals via `Into<EachFn<T>>` etc. The
            // typed-setter route is what makes the closure flow
            // through the right `Into` impl (the generic `attr`
            // fallback would try `Into<Signal<String>>`).
            | ("list", "each")
            | ("list", "key")
            | ("list", "children") // (`list_item` is no longer a user-writable tag; the
                                   // list builder owns the wrap. `item_key` is set by the
                                   // list's effect, not by author code.)
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
            // Component-specific events (CustomEvent → bind only). These
            // are inherent methods on a single tag's builder (scroll_view
            // / text), so the macro emits `.on_scroll(f)` etc.;
            // using one on the wrong tag is a clear "no method" error.
            | "on_scroll"
            | "on_scrolltoupper"
            | "on_scrolltolower"
            | "on_scrollend"
            | "on_contentsizechanged"
            | "on_layout"
            | "on_selectionchange"
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

// ---- User-component codegen ---------------------------------------------

fn user_component_to_tokens(uc: &UserComponentNode) -> TokenStream2 {
    let fn_ident = &uc.alias_ident;

    let setter_calls: Vec<TokenStream2> = uc
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
                // `ref:` is the canonical call-site name for the
                // implicit ElementRef prop. `ref` is a Rust
                // keyword so the setter is exposed as
                // `.with_ref(...)`; re-route here.
                let value = &kw.value;
                quote_spanned! {span=> .with_ref(#value) }
            } else {
                let value = &kw.value;
                quote_spanned! {span=> .#name(#value) }
            }
        })
        .collect();

    // `key` flows as a regular Props field — control-flow
    // components like `ForEach` are themselves `#[component]`s
    // and read it directly off `XxxProps`.

    let children_call = if uc.children.is_empty() {
        quote! {}
    } else {
        let child_views: Vec<TokenStream2> =
            uc.children.iter().map(node_to_tokens_as_view).collect();
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

    // `#fn_ident::builder()` (not `#props_ident::builder()`): the
    // component name doubles as a TYPE alias to its Props struct (see
    // `#[component]`), so a single `use crate::Icon` is enough — no
    // separate `IconProps` import. The outer `#fn_ident(…)` resolves
    // to the callable (value namespace), the inner `#fn_ident::` to
    // the Props type (type namespace).
    quote! {
        #fn_ident(
            #fn_ident::builder()
                #(#setter_calls)*
                #children_call
                .build()
        )
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
    use super::strip_on_prefix;
    use proc_macro2::TokenStream as TokenStream2;
    use whisker_macro_syntax::render::{is_builtin_tag, snake_to_pascal, Root};

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
        for t in ["view", "text", "raw_text", "scroll_view"] {
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
        // No-children case must stay a bare chain expression — a
        // `let __h = …; __h` wrapper breaks RA's kwarg completion.
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
        // All partial kwargs route through `.name(())` — even
        // prefixes that don't match any builder method. RA's
        // sentinel-suffix injection during completion makes any
        // "does this prefix match a method" heuristic unreliable.
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
        let result = syn::parse2::<Root>(input);
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
        let result = syn::parse2::<Root>(input);
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
        let result = syn::parse2::<Root>(input);
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
        // macro emits — both the value-namespace fn AND the
        // type-namespace `type MyCard = MyCardProps` alias share that
        // name, so `MyCard::builder()` resolves without a separate
        // `MyCardProps` import (issue #1). The `…Props` name must NOT
        // appear in the emission.
        assert!(
            output.contains("MyCard (MyCard :: builder ()") && !output.contains("MyCardProps"),
            "user component must lower to `MyCard(MyCard::builder()…)` \
             — the PascalCase alias is the public call surface and the \
             `…Props` name should not leak into the call site; \
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
            output.contains("MyCard (MyCard :: builder ()") && !output.contains("MyCardProps"),
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

    // ---- `children()` slot ---------------------------------------------

    #[test]
    fn children_slot_lowers_to_mount_children() {
        // `children()` inside a `render!` body should resolve to
        // `view::mount_children(&children)`, where `children` is the
        // surrounding `#[component]`'s `children: Children` prop in
        // local scope.
        let input: TokenStream2 = quote::quote! {
            view(style: "x") {
                children()
            }
        };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains("mount_children"),
            "children() must lower to mount_children(&children); \
             output was: {output}"
        );
        assert!(
            output.contains("& children"),
            "children() must borrow (not move) the `children` ident; \
             output was: {output}"
        );
    }

    #[test]
    fn children_slot_can_appear_multiple_times() {
        // Multi-projection: `children()` appearing N times produces
        // N independent `mount_children(&children)` calls. The Rc
        // is borrowed, never moved, so all N calls succeed.
        let input: TokenStream2 = quote::quote! {
            view(style: "x") {
                children()
                view(class: "sep")
                children()
            }
        };
        let output = super::expand_test(input).to_string();
        let mounts = output.matches("mount_children").count();
        assert_eq!(
            mounts, 2,
            "expected 2 mount_children calls, got {mounts}; \
             output was: {output}"
        );
    }

    #[test]
    fn children_slot_sits_inside_child_method_call() {
        // The slot lowers to a `.child(mount_children(&children))`
        // call on the parent builder — i.e. it goes through the
        // exact same shape as any other child node, so the parent's
        // builder chain stays a single expression.
        let input: TokenStream2 = quote::quote! {
            view(style: "x") {
                children()
            }
        };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains(". child") && output.contains("mount_children"),
            "children() must be wrapped in `.child(mount_children(&children))`; \
             output was: {output}"
        );
        assert!(
            !output.contains("let __h"),
            "children-slot-bearing emission should stay inline-chain; \
             output was: {output}"
        );
    }

    #[test]
    fn children_with_args_is_not_a_slot() {
        // `children(arg: x)` is NOT the slot — falls through to the
        // regular user-component path so a user fn named `children`
        // with props still resolves correctly. (Esoteric, but the
        // grammar must not steal the identifier.)
        let input: TokenStream2 = quote::quote! {
            children(title: "x")
        };
        let output = super::expand_test(input).to_string();
        assert!(
            output.contains("Children (Children :: builder ()"),
            "children(arg: x) should route through the user-component \
             path → Children::builder(); output was: {output}"
        );
        assert!(
            !output.contains("mount_children"),
            "children(arg: …) must NOT lower to mount_children; \
             output was: {output}"
        );
    }

    #[test]
    fn children_with_block_is_not_a_slot() {
        // `children() { … }` is also NOT the slot — that's a tag
        // invocation with empty kwargs and a children block. The
        // user is using a component literally named `children`; let
        // the regular path handle it.
        let input: TokenStream2 = quote::quote! {
            children() {
                text(value: "y")
            }
        };
        let output = super::expand_test(input).to_string();
        assert!(
            !output.contains("mount_children"),
            "children() with a `{{ … }}` block must not be a slot; \
             output was: {output}"
        );
    }
}
