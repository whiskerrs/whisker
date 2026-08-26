//! Projection of semantic paint operations into browser CSSOM values.

pub(crate) mod background_layers;
#[path = "box.rs"]
pub(crate) mod box_paint;
pub(crate) mod clip;
pub(crate) mod color;
pub(crate) mod compositing;
pub(crate) mod text;
pub(crate) mod transform;
