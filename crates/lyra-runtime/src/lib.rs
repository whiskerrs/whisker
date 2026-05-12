//! Core runtime for Lyra.
//!
//! Responsibilities:
//! - Reactive primitives (signals, effects, memos)
//! - Virtual element tree management
//! - Diffing and patching
//! - Driving Lynx Element PAPI via the FFI bridge
//! - Event dispatch from native to Rust

pub mod prelude {}
