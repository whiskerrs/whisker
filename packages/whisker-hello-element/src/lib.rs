//! `whisker-hello-element` — reference Whisker module / element library.
//!
//! Registers a single Lynx native element with the tag `<x-hello>`
//! that renders as a system-pink `UIView`. Used by `examples/hello-world`
//! as the smoke test for the full
//! `#[whisker::native_element]` → bridge → Lynx → host registry chain,
//! and as the canonical template for the Whisker module system:
//! everything a third-party module needs (Cargo.toml, `lib.rs`,
//! `whisker.module.toml`, `src/native/*.mm`) lives in this crate at
//! roughly the smallest possible size.
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
//! Then `use whisker_hello_element::XHello;` and place
//! `XHello(style: "…")` inside any `render! { … }`. The
//! `whisker run --target ios` pipeline discovers this crate's
//! `whisker.module.toml` via cargo metadata, folds
//! `src/native/whisker_hello_element.mm` into the iOS framework's
//! cc::Build, and the `LYNX_REGISTER_UI("x-hello")` constructor
//! inside the `.mm` registers the class with Lynx's behaviour
//! registry at dylib load time.

use whisker::prelude::*;

/// `<x-hello>` Whisker native element. Empty `Signal<String>` prop
/// (`style`) routes through the macro's `apply_styles` so the host
/// can size / colour the rectangle.
#[whisker::native_element("x-hello")]
pub fn x_hello(style: Signal<String>) {}
