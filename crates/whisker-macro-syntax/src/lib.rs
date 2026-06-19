//! Shared parse AST for Whisker's `render!` and `css!` macros.
//!
//! This crate holds ONLY the parse side (the AST structs/enums plus
//! their [`syn::parse::Parse`] impls). It is deliberately NOT a
//! proc-macro crate, so it can be linked into ordinary binaries — in
//! particular `whisker-fmt`, which re-parses macro bodies in order to
//! reformat them.
//!
//! The codegen side (the `to_tokens` lowering) lives in
//! `whisker-macros`. Because these types are now defined here, the
//! orphan rule forbids `whisker-macros` from adding *inherent* methods
//! to them; the lowering is expressed there as free functions over
//! `&Root` / `&Node` / … (see `whisker-macros/src/render.rs`).
//!
//! Spans are retained throughout the AST (every ident / expr carries
//! its `proc_macro2::Span`) so the formatter can recover source slices
//! and comment trivia.

pub mod css;
pub mod render;

pub use css::{CssInput, CssKwarg};
pub use render::{
    ElementNode, Kwarg, Node, Root, UserComponentNode, is_builtin_tag, is_pascal_case,
    snake_to_pascal,
};
