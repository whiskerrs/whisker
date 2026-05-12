//! # Lyra
//!
//! Cross-platform mobile UI framework for Rust, built on the Lynx C++ engine.
//!
//! Most users only need:
//!
//! ```ignore
//! use lyra::prelude::*;
//!
//! #[lyra::main]
//! fn app() -> Element {
//!     rsx! {
//!         page { style: "background: white;",
//!             text { "Hello, Lyra" }
//!         }
//!     }
//! }
//! ```

pub use lyra_app_config as app_config;
pub use lyra_runtime as runtime;

// Re-export commonly used types so users don't need to depend on the
// inner crates directly.
pub use lyra_runtime::build;
pub use lyra_runtime::element::{Element, ElementTag};
pub use lyra_runtime::renderer::Renderer;
pub use lyra_runtime::signal::{use_signal, Signal};

pub use lyra_macros::{main, rsx};

/// Internal runtime entry points used by code the `#[lyra::main]` macro
/// expands to. Not stable, not for direct use.
#[doc(hidden)]
pub mod __main_runtime {
    pub use lyra_mobile::bootstrap::{run, tick};
}

/// Common imports for Lyra app code.
pub mod prelude {
    pub use crate::build::{page, raw_text, text, text_with, view};
    pub use crate::{Element, ElementTag, Signal};
    pub use crate::{main, rsx, use_signal};
}
