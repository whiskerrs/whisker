//! Native item provider — the Rust-facing surface of Lynx's
//! `componentAtIndex` / `enqueueComponent` callback contract.
//!
//! `<list>` is *data-source-driven*: the C++ element asks an
//! external provider for each visible item by index, on demand. This
//! is what gives the list its virtualisation (only the visible items
//! exist in memory). ReactLynx registers lepus closures for this;
//! Whisker has no JS runtime, so we register Rust closures instead.
//!
//! This module defines the **value type** ([`NativeItemProvider`])
//! that view code constructs and hands off via
//! [`install_list_native_item_provider`]. The C-ABI trampoline +
//! `Box<dyn FnMut>` ↔ `*mut c_void` lifetime management lives in
//! `whisker-driver::lynx::list_provider`; the runtime crate stays
//! FFI-free so test renderers can supply a no-op default.

/// Value returned by [`NativeItemProvider::component_at_index`] to
/// signal "no element produced for this index" — the list will skip
/// the slot. Matches Lynx's `lynx::tasm::list::kInvalidIndex` (and
/// `LYNX_LIST_INVALID_INDEX`); 0 is a real FiberElement `impl_id` so
/// would be silently consumed by the C++ list as a missing-node
/// lookup, not skipped.
pub const INVALID_ITEM_INDEX: i32 = -1;

/// Callbacks Lynx's `<list>` invokes on demand:
///
/// - `component_at_index(index, op_id, reuse_notification)` — return
///   the [`Element::id`](super::Element) (sign) of the FiberElement to use for
///   `index`, or [`INVALID_ITEM_INDEX`] on failure. Called whenever a
///   slot enters the viewport.
/// - `enqueue_component(sign)` — invoked when the slot at `sign`
///   leaves the viewport so the provider can pool or release the
///   element. Optional; if `None`, recycling notifications are
///   silently dropped.
///
/// Both closures are `FnMut + 'static` because they live for as long
/// as the list element and mutate internal pool / slot state across
/// calls.
pub struct NativeItemProvider {
    pub component_at_index: Box<dyn FnMut(u32, i64, bool) -> i32 + 'static>,
    pub enqueue_component: Option<Box<dyn FnMut(i32) + 'static>>,
}
