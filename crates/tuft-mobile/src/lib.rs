//! Mobile-side Rust runtime for Tuft.
//!
//! - [`bootstrap`]: per-host runtime helpers (`run` + `tick`) the
//!   `#[tuft::main]` macro calls into. User crates don't import this
//!   directly.
//! - [`bridge_renderer::BridgeRenderer`]: implementation of
//!   [`tuft_runtime::renderer::Renderer`] backed by the C++ bridge.
//! - [`bridge_ffi`]: raw FFI declarations matching `native/bridge`'s
//!   C ABI.
//!
//! No FFI symbols are exported from this crate. The cdylib that ships
//! to iOS/Android is the *user's* crate, which `#[tuft::main]` annotates
//! to generate the necessary `tuft_mobile_app_main` /
//! `tuft_mobile_tick` exports calling into [`bootstrap`].

pub mod bootstrap;
mod bridge_ffi;
mod bridge_renderer;

pub use bridge_renderer::BridgeRenderer;

