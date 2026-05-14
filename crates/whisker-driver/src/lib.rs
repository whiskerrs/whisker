//! Backend driver for Whisker.
//!
//! Today there is one backend ([`lynx`]) that talks to the C++ Lynx
//! bridge shipped under `whisker-driver-sys/bridge/`. Future backends
//! (web, wgpu, …) would
//! land as sibling modules behind cfg gates. Users never touch this
//! crate directly — `#[whisker::main]` re-exports the `run` / `tick`
//! helpers from here as `whisker::__main_runtime::{run,tick}`.

pub mod lynx;

pub use lynx::bootstrap;
pub use lynx::renderer::BridgeRenderer;
