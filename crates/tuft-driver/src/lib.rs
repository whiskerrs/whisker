//! Backend driver for Tuft.
//!
//! Today there is one backend ([`lynx`]) that talks to the C++ Lynx
//! bridge in `native/bridge/`. Future backends (web, wgpu, …) would
//! land as sibling modules behind cfg gates. Users never touch this
//! crate directly — `#[tuft::main]` re-exports the `run` / `tick`
//! helpers from here as `tuft::__main_runtime::{run,tick}`.

pub mod lynx;

pub use lynx::bootstrap;
pub use lynx::renderer::BridgeRenderer;
