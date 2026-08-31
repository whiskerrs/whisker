//! Shared parse AST for Whisker's `render!`, `css!` and `routes!` macros.
//!
//! This crate holds ONLY the parse side (the AST structs/enums plus
//! their [`syn::parse::Parse`] impls). It is deliberately NOT a
//! proc-macro crate, so it can be linked into ordinary binaries — in
//! particular `whisker-fmt`, which re-parses macro bodies in order to
//! reformat them.
//!
//! The codegen side lives in `whisker-macros`. `render!` lowers the shared
//! tree directly to public builder calls, `css!` uses the shared named-
//! argument list, and `routes!` applies router-specific validation to the
//! same parsed nodes.
//!
//! Spans are retained throughout the AST (every ident / expr carries
//! its `proc_macro2::Span`) so the formatter can recover source slices
//! and comment trivia.

pub mod compose;

pub use compose::{ComposeArgument, ComposeArguments, ComposeChild, ComposeInput, ComposeNode};
