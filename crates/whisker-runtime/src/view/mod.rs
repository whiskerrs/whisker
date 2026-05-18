//! View layer — element handles, type-erased renderer, `IntoView`.
//!
//! This is the surface the new (Phase 6.5a) `render!` macro emits
//! against. It deliberately mirrors the shape of [`crate::renderer`]
//! but with two key differences:
//!
//! 1. **Type-erased handle**. The new system uses a single
//!    [`ElementHandle`] (a `Copy` newtype around a `u32` ID) regardless
//!    of which backend is mounted. The renderer maps these IDs to
//!    whatever concrete types it needs internally — `MockRenderer`
//!    keeps a `HashMap<u32, MockOp>`, the production bridge maps each
//!    to a `Rc<LynxElement>`.
//!
//! 2. **Thread-local active renderer**. The macro expansion calls
//!    free functions ([`create_element`], [`set_attribute`], etc.)
//!    that dispatch through a thread-local "currently mounted"
//!    renderer. Avoids threading `R` through every closure the macro
//!    generates.
//!
//! Both choices match the reactive runtime's pattern (thread-local
//! arena, opaque handles). The old [`crate::renderer::Renderer`]
//! trait + [`crate::render`] module stay until A3 Step 5 deletes
//! them; both APIs co-exist during the transition.

pub mod control_flow;
pub mod handle;
pub mod into_view;
pub mod renderer;

#[cfg(test)]
mod tests;

pub use control_flow::{for_each, show};
pub use handle::ElementHandle;
pub use into_view::{IntoView, View};
pub use renderer::{
    append_child, child_index, children_of, create_element, current_renderer_id, flush,
    insert_child_at, install_renderer, previous_sibling, release_element, remove_child,
    set_attribute, set_event_listener, set_inline_styles, set_root, uninstall_renderer,
    with_installed_renderer, DynRenderer,
};
#[doc(hidden)]
pub use renderer::__reset_children_mirror_for_tests;
