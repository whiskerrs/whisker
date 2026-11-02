//! 3D game engine for Whisker.
//!
//! Application code mounts [`Viewport`]; each Host only supplies a render target
//! and frame timing to the shared wgpu [`SceneRenderer`].

mod renderer;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod ios;

pub use renderer::{Renderer, SceneRenderer};
#[doc(hidden)]
pub use wgpu;

/// A surface that Kiba draws into.
#[whisker::module_element(name = "kiba:Viewport", measurement = None)]
pub fn viewport(style: whisker::Style) {}

#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [viewport_schema::element_provider()],
    )
}
