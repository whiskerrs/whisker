//! View layer — element handles, type-erased renderer, `IntoView`.
//!
//! This is the surface the `render!` macro emits against. Two choices
//! shape it, both matching the reactive runtime's own pattern of a
//! thread-local arena behind opaque handles:
//!
//! 1. **Type-erased handle**. A single [`Element`] (a `Copy` newtype
//!    around a `u32` ID) regardless of which backend is mounted. The
//!    renderer maps these IDs to whatever concrete types it needs
//!    internally — `MockRenderer` keeps a `HashMap<u32, MockOp>`, the
//!    production Hosts map each to their retained native element.
//!
//! 2. **Thread-local active renderer**. The macro expansion calls free
//!    functions ([`create_element`], [`set_attribute`], etc.) that
//!    dispatch through a thread-local "currently mounted" renderer,
//!    rather than threading an `R` through every closure the macro
//!    generates.

pub mod apply;
pub mod control_flow;
pub mod handle;
pub mod into_view;
pub mod list;
pub mod renderer;
pub mod virtualizer;

#[cfg(test)]
mod tests;

pub use apply::{
    apply_accessibility, apply_attr, apply_attr_bool, apply_attr_f64, apply_attr_int,
    apply_attr_int_mapped, apply_attr_owned, apply_dataset, apply_element_id, apply_text_max_lines,
};
pub use handle::Element;
pub use into_view::{
    Children, ChildrenBuilder, EachFn, Fallback, IntoView, ItemFn, KeyFn, TextChildren, View,
    WhenFn, mount_children, mount_text_children, mount_view,
};
pub use list::{
    ListHandle, ListHandleError, ListRef, ListScrollTarget, ListSnapshot, ScrollAlignment,
    ScrollAxis, ScrollBehavior,
};
#[doc(hidden)]
pub use renderer::__reset_children_mirror_for_tests;
pub use virtualizer::{VirtualGridLayout, VirtualListLayout, VirtualListOptions, virtualize};

// Element-manipulation + lifecycle surface the `render!` macro expands
// against and that framework-extension authors (custom control flow,
// platform-component module crates) legitimately reach for.
pub use renderer::{
    BindType, append_child, child_index, children_of, create_element, create_element_by_name,
    create_element_by_schema, create_phantom_element, dispatch_event, flush, insert_child_at,
    is_phantom, observe_layout, observe_layout_batch_end, previous_sibling, release_element,
    remove_child, set_accessibility, set_attribute, set_attribute_bool, set_attribute_double,
    set_attribute_int, set_attribute_object, set_dataset, set_element_id, set_event_listener,
    set_root, set_specified_style, set_text_max_lines,
};

// Renderer-wiring internals. Public because Hosts and test renderers link
// against them across the crate boundary and macro
// expansions name them by path — but NOT part of the app- or
// module-author API, hence `#[doc(hidden)]`.
#[doc(hidden)]
pub use renderer::{
    DynRenderer, EventDispatchPlan, PHANTOM_BASE, current_renderer_id, install_renderer,
    specified_style, try_invoke_element_command, uninstall_renderer, with_installed_renderer,
};
