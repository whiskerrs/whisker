//! View layer — element handles, type-erased renderer, `IntoView`.
//!
//! This is the surface the new (Phase 6.5a) `render!` macro emits
//! against. It deliberately mirrors the shape of [`crate::renderer`]
//! but with two key differences:
//!
//! 1. **Type-erased handle**. The new system uses a single
//!    [`Element`] (a `Copy` newtype around a `u32` ID) regardless
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

pub mod apply;
pub mod control_flow;
pub mod handle;
pub mod into_view;
pub mod list_provider;
pub mod renderer;
pub mod virtualizer;

#[cfg(test)]
mod tests;

pub use apply::{
    apply_attr, apply_attr_bool, apply_attr_f64, apply_attr_int, apply_attr_owned, apply_styles,
};
pub use handle::Element;
pub use into_view::{
    Children, EachFn, Fallback, IntoView, ItemFn, KeyFn, MetaFn, View, WhenFn, mount_children,
};
pub use list_provider::{INVALID_ITEM_INDEX, NativeItemProvider};
#[doc(hidden)]
pub use renderer::__reset_children_mirror_for_tests;
pub use renderer::ListItemAction;
pub use virtualizer::{ItemMeta, virtualize};

// Element-manipulation + lifecycle surface the `render!` macro expands
// against and that framework-extension authors (custom control flow,
// platform-component module crates) legitimately reach for.
pub use renderer::{
    BindType, append_child, child_index, children_of, create_element, create_element_by_name,
    create_phantom_element, dispatch_event, flush, insert_child_at,
    install_list_native_item_provider, is_phantom, previous_sibling, release_element, remove_child,
    set_attribute, set_attribute_int, set_attribute_object, set_event_listener, set_inline_styles,
    set_root, set_update_list_info,
};

// Renderer-wiring internals. Public because `whisker-driver` (and test
// renderers) link against them across the crate boundary and the macro
// expansions name them by path — but NOT part of the app- or
// module-author API. `#[doc(hidden)]` keeps them out of docs.rs and
// signals "do not depend on this" without breaking the existing
// cross-crate references.
#[doc(hidden)]
pub use renderer::{
    DynRenderer, EventDispatchPlan, PHANTOM_BASE, current_renderer_id, element_sign,
    install_renderer, module_component_ptr, uninstall_renderer, with_installed_renderer,
};
