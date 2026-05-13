//! Development-only runtime extensions for Tuft.
//!
//! Compiled into Tuft apps only when the umbrella crate is built
//! with `--features hot-reload`. Release builds end up with an empty
//! crate (no tokio / no WebSocket / no subsecond).
//!
//! ## What lives here
//! - [`hot_reload`]: WebSocket *client* that connects to the
//!   `tuft run` dev server, deserialises incoming `subsecond::JumpTable`
//!   messages, and parks them on a single-slot mutex. The Lynx TASM
//!   thread drains the mutex at the top of every tick and invokes
//!   `subsecond::apply_patch`.

#[cfg(feature = "hot-reload")]
pub mod hot_reload;

#[cfg(feature = "hot-reload")]
pub use hot_reload::{start_receiver, take_pending_patch};
