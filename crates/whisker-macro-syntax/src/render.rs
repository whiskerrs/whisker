//! Parse AST for the `render!` macro — compose-style, kwarg-only DSL.
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
//! - Built-in tag (see [`is_builtin_tag`]) → [`Node::Element`].
//! - `children()` (lowercase ident + empty parens, no block) →
//!   [`Node::ChildrenSlot`].
//! - Anything else → user `#[component]` invocation →
//!   [`Node::UserComponent`].
//!
//! ## Children-block restriction
//!
//! Every item in a `{ … }` children block MUST be node-shaped
//! (`IDENT(kwargs?) { … }?`). Bare string literals and bare `{expr}`
//! blocks are rejected with a hard parser error, which keeps the block
//! on rust-analyzer's completion happy-path.

use proc_macro2::TokenStream as TokenStream2;
use syn::{
    Expr, Ident, LitStr, Token, braced,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream, Result},
    token,
};

// ---- AST ----------------------------------------------------------------

/// Root of a `render!` body — exactly one top-level [`Node`].
pub struct Root {
    pub node: Node,
}

impl Parse for Root {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            node: input.parse()?,
        })
    }
}

/// A single node in the `render!` tree.
pub enum Node {
    Element(ElementNode),
    UserComponent(UserComponentNode),
    /// `children()` — the lone special-cased ident in the children
    /// grammar. Lowers (in `whisker-macros`) to
    /// `::whisker::runtime::view::mount_children(&children)`.
    ChildrenSlot {
        span: proc_macro2::Span,
    },
}

/// A built-in element (`view`, `text`, …).
pub struct ElementNode {
    pub tag: Ident,
    pub kwargs: Vec<Kwarg>,
    pub children: Vec<Node>,
}

/// A user `#[component]` invocation.
pub struct UserComponentNode {
    /// PascalCase ident the call site resolves to (the public
    /// re-exported alias the `#[component]` macro emits). Derived from
    /// whichever casing the author wrote.
    pub alias_ident: Ident,
    pub kwargs: Vec<Kwarg>,
    pub children: Vec<Node>,
}

/// One `name: value` (or partial `name`) pair.
pub struct Kwarg {
    pub name: Ident,
    pub value: Expr,
    /// `true` when the user hasn't typed `:` + an expression yet
    /// (cursor sits at the end of the kwarg name). `value` is then a
    /// synthesized `()` placeholder.
    pub partial: bool,
}

impl Parse for Node {
    fn parse(input: ParseStream) -> Result<Self> {
        // Every node starts with an ident. A targeted error at this
        // position is the only way to give a useful hint when the user
        // writes bare `"hi"` or `{ count }` as a child.
        if input.peek(LitStr) {
            let lit: LitStr = input.parse()?;
            return Err(syn::Error::new(
                lit.span(),
                "bare string literals are not allowed in render!; \
                 use `text(value: \"…\")` to render text content",
            ));
        }
        if input.peek(token::Brace) {
            // The brace's own span gives a clean diagnostic arrow.
            let body;
            let _ = braced!(body in input);
            let _ = body; // discard contents — we're erroring out
            return Err(input.error(
                "bare `{expr}` blocks are not allowed in render!; \
                 use `text(value: <expr>)` to render dynamic text",
            ));
        }

        let tag: Ident = input.parse()?;

        // The `children()` slot requires ident `children`, an empty
        // `()` immediately after, and no `{ … }` block. Anything else
        // (`children(arg)`, `children { … }`, bare `children`) falls
        // through to the standard tag path.
        if tag == "children" && input.peek(token::Paren) {
            // Speculative: commit to the slot interpretation only if
            // the body is empty AND no `{}` block follows.
            let fork = input.fork();
            let body;
            parenthesized!(body in fork);
            if body.is_empty() && !fork.peek(token::Brace) {
                let consume;
                parenthesized!(consume in input);
                debug_assert!(consume.is_empty());
                return Ok(Node::ChildrenSlot { span: tag.span() });
            }
        }

        let mut kwargs = Vec::new();
        if input.peek(token::Paren) {
            let body;
            parenthesized!(body in input);
            while !body.is_empty() {
                // `parse_any` admits Rust keywords, needed for `ref:`.
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
                    // A `()` placeholder lets the emitter still put the
                    // method-name token at the user's source span.
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
        //   PascalCase (anything)                → UserComponent
        //   snake_case + not in whitelist        → UserComponent
        //                                          (back-compat path)
        if is_builtin_tag(&name) {
            Ok(Node::Element(ElementNode {
                tag,
                kwargs,
                children,
            }))
        } else {
            let span = tag.span();
            let alias_str = if is_pascal_case(&name) {
                name.clone()
            } else {
                snake_to_pascal(&name)
            };
            let alias_ident = Ident::new(&alias_str, span);
            Ok(Node::UserComponent(UserComponentNode {
                alias_ident,
                kwargs,
                children,
            }))
        }
    }
}

// ---- Classification helpers (parse-time) --------------------------------

/// `my_card` → `MyCard`. Snake-to-PascalCase for the back-compat
/// snake_case path of user components in `render!`.
pub fn snake_to_pascal(name: &str) -> String {
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
pub fn is_builtin_tag(name: &str) -> bool {
    matches!(
        name,
        "view" | "text" | "raw_text" | "scroll_view" | "list" | "fragment"
    )
}

/// `true` if `name`'s first character is ASCII uppercase. Used to
/// route PascalCase idents (user components / control flow) away
/// from the snake_case-only Element path.
pub fn is_pascal_case(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Convenience: parse a `render!` body from a token stream.
pub fn parse_root(tokens: TokenStream2) -> Result<Root> {
    syn::parse2(tokens)
}
