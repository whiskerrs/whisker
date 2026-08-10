//! Where control flow lives.
//!
//! Built-in `for_each` / `show` are `#[component]` functions in the
//! `whisker` crate (the user-facing layer), so they share the exact
//! same author surface as user-defined control flow. They build on the
//! primitives this crate exposes:
//! [`create_phantom_element`](super::create_phantom_element),
//! [`effect`](crate::reactive::effect), and the per-control-flow
//! function types in [`super::into_view`].
//!
//! Custom control flow follows the same outline:
//!
//! ```ignore
//! #[whisker::component]
//! pub fn my_animated_show(when: ReadSignal<bool>, children: Children) -> Element {
//!     let frag = whisker::runtime::view::create_phantom_element();
//!     // ... install effect that mounts/unmounts under `frag` ...
//!     frag
//! }
//! ```
//!
//! See `docs/wrapper-less-control-flow-design.md` for the rationale.
