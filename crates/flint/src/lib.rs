//! # Flint
//!
//! Cross-platform mobile UI framework for Rust, built on the Lynx C++ engine.
//!
//! This is the umbrella crate. Most users should `use flint::prelude::*;`.

pub use flint_app_config as app_config;
pub use flint_runtime as runtime;

pub use flint_macros::main;

/// Common imports for Flint app code.
pub mod prelude {
    pub use flint_macros::main;
}
