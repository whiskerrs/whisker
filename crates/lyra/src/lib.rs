//! # Lyra
//!
//! Cross-platform mobile UI framework for Rust, built on the Lynx C++ engine.
//!
//! This is the umbrella crate. Most users should `use lyra::prelude::*;`.

pub use lyra_app_config as app_config;
pub use lyra_runtime as runtime;

pub use lyra_macros::main;

/// Common imports for Lyra app code.
pub mod prelude {
    pub use lyra_macros::main;
}
