//! Per-property builder methods on [`Css`](crate::Css).
//!
//! Each submodule adds one `impl crate::Css` block of methods,
//! one method per CSS longhand property. Every method takes a typed
//! value, serializes it through [`ToCss`](crate::ToCss), and pushes
//! it onto the style. Doc comments on each method link to the
//! corresponding `lynxjs.org/api/css/properties/<name>` page.

mod animation;
mod background;
mod border;
mod box_model;
mod display;
mod effects;
mod flex;
mod grid;
mod linear;
mod overflow;
mod position;
mod relative;
mod text;
mod transform;
mod transition;
mod typography;
