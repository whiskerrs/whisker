//! `whisker-hello-element` — reference Whisker module / element library.
//!
//! Registers a single Lynx platform component (locally named `Hello`)
//! that renders as a system-pink `UIView`. Used by
//! `examples/hello-world` as the smoke test for the full
//! `#[whisker::platform_component]` → bridge → Lynx → host registry
//! chain, and as the canonical template for the Whisker module
//! system.
//!
//! ## Building
//!
//! Consumers depend on this crate the usual way:
//!
//! ```toml
//! [dependencies]
//! whisker-hello-element = { path = "../../packages/whisker-hello-element" }
//! ```
//!
//! Then `use whisker_hello_element::Hello;` and place
//! `Hello(style: "…")` inside any `render! { … }`. The Lynx tag
//! string the bridge actually registers against is
//! `whisker-hello-element:Hello` — the cargo crate name (kebab-
//! case) is auto-prepended by `#[whisker::platform_component]` so two
//! unrelated module packages can both declare an element named
//! `Hello` without colliding in Lynx's behaviour registry. The
//! platform-side `@WhiskerElement` macro / KSP processor emits the
//! matching `<crate-name>:<local-name>` registration on its end.
//! (Phase 7-Φ.H.2.)

use whisker::prelude::*;

/// Whisker platform component with local tag name `Hello`. The Lynx
/// registration string is `whisker-hello-element:Hello`. Empty
/// `Signal<String>` prop (`style`) routes through the macro's
/// `apply_styles` so the host can size / colour the rectangle.
#[whisker::platform_component("Hello")]
pub fn hello(style: Signal<String>) {}
