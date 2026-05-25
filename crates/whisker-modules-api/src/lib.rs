//! # whisker-modules-api
//!
//! Minimal API surface for third-party **Whisker modules** (the
//! cargo-distributed packages that ship platform-side Swift / Kotlin
//! code alongside a Rust shim). Module authors depend on *just this
//! crate*; they never need the umbrella `whisker` crate (that's the
//! surface for *app* authors — `#[whisker::main]`, `render!`,
//! reactive primitives, …).
//!
//! ## Why a separate crate?
//!
//! Before Phase J every module crate depended on the umbrella
//! `whisker` crate, which transitively pulls in the full app-author
//! surface — `#[whisker::main]`, `render!`, `#[component]`, the
//! reactive operator zoo, the built-in tag builders. None of that is
//! reachable from a module crate's body, so the dep was effectively
//! a "give me everything just in case" import that muddied the
//! contract. This crate isolates the *module-author* part:
//!
//! - [`WhiskerValue`](platform_module::WhiskerValue) / [`WhiskerModuleError`](platform_module::WhiskerModuleError) /
//!   [`invoke`](platform_module::invoke) / [`invoke_async`](platform_module::invoke_async)
//!   for module-side dispatch.
//! - [`Signal<T>`] / [`ElementRef`] / [`element_ref`] for view-bearing
//!   modules whose Rust shim renders into a `render!` tree.
//! - The three module-author proc macros:
//!   [`platform_component`] (declare a view-bearing platform
//!   component), [`platform_module`] (declare a typed Rust proxy to
//!   a platform-side Swift / Kotlin class), and [`element_methods`]
//!   (declare typed methods on `ElementRef<T>` that route through
//!   the C bridge to the platform component).
//!
//! ## Cargo-rename convention
//!
//! Module crates must rename this dep to `whisker` so the proc macros'
//! emit paths (`::whisker::ElementRef`,
//! `::whisker::runtime::view::Element`,
//! `::whisker::platform_module::WhiskerValue`, …) resolve. Concretely:
//!
//! ```toml
//! [dependencies]
//! whisker = { package = "whisker-modules-api", workspace = true }
//! ```
//!
//! With that in place, `use whisker::platform_module::WhiskerValue;`
//! works identically in module crates and app crates — the rename
//! makes the module crate see this crate's narrower surface under
//! the `whisker` name. **App crates keep depending on the umbrella
//! directly** (`whisker = { workspace = true }`) and reach the
//! same paths because the umbrella re-exports them too.

// Top-level re-exports — these mirror the subset of the umbrella
// `whisker` crate that module-author code can reach (under the
// cargo rename in their `Cargo.toml`).

pub use whisker_runtime as runtime;

pub use whisker_runtime::element::ElementTag;

pub use whisker_macros::{element_methods, platform_component, platform_module};

pub use whisker_driver::{element_ref, ElementRef};

pub use whisker_runtime::reactive::Signal;

// `WhiskerValue` / `invoke` / `invoke_async` live under
// `whisker::platform_module::*` in the umbrella, so we mirror the
// same path layout here.
pub mod platform_module {
    pub use whisker_driver::module::{
        from_raw, invoke, invoke_async, WhiskerModuleError, WhiskerValue,
    };
}
