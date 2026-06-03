//! `whisker-svg-core` — Rust producer + replay infrastructure for
//! the binary display-list format defined in
//! `packages/whisker-svg/SPEC.md`.
//!
//! ## Modules
//!
//! - [`format`] — opcode constants. The single Rust-side source
//!   of truth for the SPEC's "Opcode space" table.
//! - [`builder`] — [`DisplayListBuilder`] writes a v1 stream.
//! - [`replay`] — generic [`Visitor`] + [`replay`] function. Used
//!   by [`replay::TraceVisitor`] for golden-file tests and by
//!   future Rust-side renderers (none ship yet; the per-platform
//!   replayers in `packages/whisker-svg/{ios,android}/` are
//!   independent reimplementations against the same SPEC).
//! - [`compile`] — SVG XML → bytes via [`DisplayListBuilder`].
//! - [`path_parse`] — SVG `<path d>` attribute parser.
//!
//! The crate is intentionally `no_std`-compatible in spirit
//! (no platform deps, no async, no FFI) so it can later be lifted
//! into a `cdylib` for `whisker-driver-sys` integration without
//! restructuring.

pub mod builder;
pub mod compile;
pub mod format;
pub mod path_parse;
pub mod replay;

pub use builder::{Color, DisplayListBuilder, Transform};
pub use compile::{compile, CompileError, Compiled};
pub use replay::{replay, ReplayError, TraceVisitor, Visitor};
