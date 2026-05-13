//! # Tuft
//!
//! Cross-platform mobile UI framework for Rust, built on the Lynx C++ engine.
//!
//! Most users only need:
//!
//! ```ignore
//! use tuft::prelude::*;
//!
//! #[tuft::main]
//! fn app() -> Element {
//!     rsx! {
//!         page { style: "background: white;",
//!             text { "Hello, Tuft" }
//!         }
//!     }
//! }
//! ```

pub use tuft_app_config as app_config;
pub use tuft_runtime as runtime;

// Re-export commonly used types so users don't need to depend on the
// inner crates directly.
pub use tuft_runtime::build;
pub use tuft_runtime::element::{Element, ElementTag};
pub use tuft_runtime::renderer::Renderer;
pub use tuft_runtime::signal::{use_signal, Signal};

pub use tuft_macros::{main, rsx};

/// Internal runtime entry points used by code the `#[tuft::main]` macro
/// expands to. Not stable, not for direct use.
#[doc(hidden)]
pub mod __main_runtime {
    pub use tuft_driver::bootstrap::{run, tick};
}

/// Common imports for Tuft app code.
pub mod prelude {
    pub use crate::build::{image, page, raw_text, scroll_view, text, text_with, view};
    pub use crate::{Element, ElementTag, Signal};
    pub use crate::{main, rsx, use_signal};
}
