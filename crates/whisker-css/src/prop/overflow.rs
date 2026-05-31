//! `overflow` is implemented in [`crate::prop::effects`] (it sits
//! conceptually with the visual-effect group). This module exists so
//! that adding overflow-related properties later does not require
//! restructuring `effects.rs`.
//!
//! Lynx supports only `overflow-x`, `overflow-y`, and the shorthand
//! `overflow`. CSS's `scroll`/`auto` values are not supported; use a
//! `<scroll-view>` element to scroll.
