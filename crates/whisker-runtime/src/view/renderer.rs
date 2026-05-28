//! Type-erased renderer + thread-local current-renderer plumbing.
//!
//! The `render!` macro emits calls to the free functions in this
//! module ([`create_element`], [`set_attribute`], …). Each looks up
//! the currently-installed [`DynRenderer`] from a `thread_local!`
//! slot and forwards. This keeps the macro output renderer-agnostic
//! while still letting tests swap in a `MockRenderer`.
//!
//! Lifecycle:
//!
//! ```ignore
//! let renderer = Box::new(MyRenderer::new());
//! let prev = install_renderer(renderer);
//! // … all `view::create_element` etc. calls now go to MyRenderer
//! uninstall_renderer(prev);                 // restore previous (None)
//! ```
//!
//! In production the bridge driver installs the Lynx-backed renderer
//! once at startup and keeps it for the life of the process.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::handle::Element;
use crate::element::ElementTag;
use crate::value::WhiskerValue;

/// Event-handler propagation type — a faithful 1:1 mapping to Lynx's
/// four handler kinds (`bind` / `catch` / `capture-bind` /
/// `capture-catch`). The variant chosen when registering a listener is
/// what drives Lynx's native event chain:
///
///   - **phase**: capture handlers fire on the way *down* (root →
///     target); bind/catch (bubble) handlers fire on the way *up*
///     (target → root).
///   - **stop**: a `catch` handler stops propagation after it fires;
///     a `bind` handler lets the event continue along the chain.
///
/// The discriminants match `lynx_event_bind_type_e` in the C bridge,
/// so the value crosses the FFI as a plain `i32`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BindType {
    /// `bind` — bubble phase, does not stop propagation. The default
    /// (what plain `on_<event>` registers).
    #[default]
    Bind = 0,
    /// `catch` — bubble phase, stops propagation at this element.
    Catch = 1,
    /// `capture-bind` — capture phase, does not stop propagation.
    CaptureBind = 2,
    /// `capture-catch` — capture phase, stops propagation.
    CaptureCatch = 3,
}

/// One planned listener firing: the listener plus the event value it
/// should receive (its `currentTarget` already rewritten to that
/// listener's element).
pub type EventFiring = (Rc<dyn Fn(WhiskerValue) + 'static>, WhiskerValue);

/// The ordered firing plan produced by
/// [`DynRenderer::plan_event_dispatch`]. Separates *planning* (done
/// under the renderer borrow) from *firing* (done after the borrow is
/// released, since a handler may re-enter the renderer).
#[derive(Default)]
pub struct EventDispatchPlan {
    /// Whether any listener matched — relayed to the platform reporter
    /// so Lynx can skip its own native chain for this event.
    pub consumed: bool,
    /// Listeners to invoke, in propagation order.
    pub firings: Vec<EventFiring>,
}

/// Object-safe renderer trait. The renderer owns whatever per-element
/// state it needs and answers in `Element` IDs.
///
/// Mirrors the shape of [`crate::renderer::Renderer`] but is
/// type-erased — the handle type is always [`Element`]. Existing
/// `R: Renderer` implementations bridge into here via a small adapter
/// that maintains its own `Element → R::Element` map.
pub trait DynRenderer {
    fn create_element(&mut self, tag: ElementTag) -> Element;
    /// Phase 7: tag-by-name dispatch for custom / xelement-style
    /// tags ("x-input", etc.) not in the built-in [`ElementTag`]
    /// enum. Returns [`Element::INVALID`] when the tag is unknown
    /// to Lynx's behaviour registry.
    fn create_element_by_name(&mut self, tag_name: &str) -> Element;
    fn release_element(&mut self, handle: Element);

    fn set_attribute(&mut self, handle: Element, key: &str, value: &str);
    fn set_inline_styles(&mut self, handle: Element, css: &str);

    /// Hand a `<list>` element its item count so the bridge can build
    /// the `update-list-info` map (positional item-keys `w_<i>`) that
    /// Lynx's decoupled native list reads its items from. The `list`
    /// builder calls this once at `__h()` finalize. Default no-op for
    /// test renderers that don't model list virtualisation.
    fn set_update_list_info(&mut self, _handle: Element, _count: i32) {}

    fn append_child(&mut self, parent: Element, child: Element);
    fn remove_child(&mut self, parent: Element, child: Element);

    /// Register `callback` for `event_name` on `handle`.
    ///
    /// The callback receives the event body Lynx hands the handler
    /// as a [`WhiskerValue`] tree (the same wire as module
    /// args/returns). A built-in builder's `on_<event>` method or a
    /// `#[whisker::module_component]` `on_<event>` prop wraps a
    /// typed-event / unit / raw-value closure into this single
    /// shape, deserializing the payload as needed. An event with no
    /// body fires the callback with [`WhiskerValue::Null`].
    fn set_event_listener(
        &mut self,
        handle: Element,
        event_name: &str,
        bind_type: BindType,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    );

    /// Plan how a reported event (`event_name` at `target_sign`,
    /// carrying `body`) propagates through Whisker's reconstructed
    /// chain — capture phase (root → target) then bubble phase
    /// (target → root), honoring each registered listener's
    /// [`BindType`] (catch stops bubbling; capture-catch stops
    /// everything).
    ///
    /// Returns the listeners to fire **in order**, each paired with the
    /// event value it should receive (its `currentTarget` set to that
    /// listener's element), plus whether the event was consumed.
    ///
    /// Crucially this only *plans* — it does not fire the listeners,
    /// because firing happens after the renderer borrow is released
    /// (a handler may mutate signals → effects → re-enter the
    /// renderer). [`dispatch_event`] does the firing. The default impl
    /// plans nothing (renderers without a native event source); the
    /// Lynx bridge renderer overrides it.
    fn plan_event_dispatch(
        &self,
        _target_sign: i32,
        _event_name: &str,
        _body: &WhiskerValue,
    ) -> EventDispatchPlan {
        EventDispatchPlan::default()
    }

    fn set_root(&mut self, page: Element);
    fn flush(&mut self);

    /// Opaque platform pointer the C bridge associates with this
    /// `Element` handle (cast from `*mut WhiskerElement` for the
    /// Lynx bridge renderer; `0` for renderers without a native
    /// backing).
    ///
    /// Used by `whisker-driver`'s `ElementRef::invoke` to call
    /// `whisker_bridge_invoke_element_method` without the runtime
    /// crate having to know about the bridge's C types. Renderers
    /// that don't have a native pointer return `0`, which the
    /// driver surfaces as `WhiskerValue::Error` to the caller.
    ///
    /// Phase 7-Φ.H.2.3.
    fn module_component_ptr(&self, _handle: Element) -> usize {
        0
    }
}

thread_local! {
    /// The active renderer for this thread. `None` outside any mount.
    ///
    /// Wrapped in `RefCell<Option<Box<dyn>>>` rather than holding the
    /// renderer directly so [`install_renderer`] can swap one out for
    /// another atomically and tests can run with no renderer installed
    /// (where dispatch functions silently no-op + warn).
    static CURRENT_RENDERER: RefCell<Option<Box<dyn DynRenderer>>> = const { RefCell::new(None) };

    /// Whisker-side mirror of every parent → ordered-children
    /// relationship the runtime has emitted. Maintained by
    /// [`append_child`] / [`remove_child`].
    ///
    /// Used by `mount_component_remountable` (#17 wrapper-removal
    /// follow-up) to compute the "previous sibling at mount time"
    /// anchor without asking Lynx — Lynx's C API doesn't expose a
    /// child-position query, and we'd rather not add one. Side
    /// effect: the mirror also enables `previous_sibling` /
    /// `next_sibling` queries for any future need (e.g. insert_after
    /// shimming when we ship the wrapper-less remount path).
    static CHILDREN_OF: RefCell<HashMap<Element, Vec<Element>>> =
        RefCell::new(HashMap::new());
}

/// Install `r` as the current renderer for this thread, returning
/// whatever renderer was installed before (so the caller can restore
/// it later if needed).
///
/// Most production callers install exactly once and never restore.
/// Tests use the returned previous value to reset between cases.
pub fn install_renderer(r: Box<dyn DynRenderer>) -> Option<Box<dyn DynRenderer>> {
    CURRENT_RENDERER.with_borrow_mut(|slot| slot.replace(r))
}

/// Remove the current renderer, returning it to the caller. The
/// thread-local slot is left `None`. Subsequent dispatch calls warn
/// (in debug) and no-op.
pub fn uninstall_renderer(prev: Option<Box<dyn DynRenderer>>) {
    CURRENT_RENDERER.with_borrow_mut(|slot| *slot = prev);
}

/// Run `f` with `r` temporarily installed as the current renderer.
/// Restores whatever was previously installed when `f` returns
/// (including the `None` state). Useful for tests + scoped
/// rendering.
pub fn with_installed_renderer<R>(r: Box<dyn DynRenderer>, f: impl FnOnce() -> R) -> R {
    let prev = install_renderer(r);
    let result = f();
    let _new = CURRENT_RENDERER.with_borrow_mut(|slot| slot.take());
    if let Some(p) = prev {
        let _ = install_renderer(p);
    }
    result
}

/// Crate-internal sigil for "no renderer installed" diagnostics —
/// distinguishes "renderer panicked" from "no renderer in this
/// scope" in tests.
pub fn current_renderer_id() -> Option<&'static str> {
    CURRENT_RENDERER.with_borrow(|slot| slot.as_ref().map(|_| "installed"))
}

fn with_renderer<R>(f: impl FnOnce(&mut dyn DynRenderer) -> R, default: R) -> R {
    CURRENT_RENDERER.with_borrow_mut(|slot| match slot.as_mut() {
        Some(r) => f(r.as_mut()),
        None => {
            #[cfg(debug_assertions)]
            eprintln!("whisker-view: renderer call outside any installed renderer; ignored");
            default
        }
    })
}

// ---------------------------------------------------------------------------
// Free-function dispatch — what the `render!` macro and reactive
// effects call.
// ---------------------------------------------------------------------------

/// Free-fn helper used by the `render!` macro and reactive effects to
/// allocate an element of any tag the bridge knows. Routes both the
/// built-in `ElementTag` enum and tag-by-name strings through the
/// same owner-tracking + invalid-handle logic.
pub fn create_element_by_name(tag_name: &str) -> Element {
    let handle = with_renderer(|r| r.create_element_by_name(tag_name), Element(u32::MAX));
    if handle.id() != u32::MAX {
        crate::reactive::with_runtime(|rt| {
            if let Some(owner_id) = rt.current_owner() {
                if let Some(owner) = rt.owners.get_mut(owner_id) {
                    owner.elements.push(handle);
                }
            }
        });
    }
    handle
}

pub fn create_element(tag: ElementTag) -> Element {
    let handle = with_renderer(|r| r.create_element(tag), Element(u32::MAX));
    // Track the freshly-created element in whichever reactive owner
    // is currently active. `dispose_owner` later releases everything
    // in this list via `release_element`. This is what stops
    // `BridgeRenderer::elements` (and the underlying Lynx
    // FiberElement refcounts) from accumulating across `<Show>`
    // branch flips, `<For>` item removals, and per-component
    // remounts.
    if handle.id() != u32::MAX {
        crate::reactive::with_runtime(|rt| {
            if let Some(owner_id) = rt.current_owner() {
                if let Some(owner) = rt.owners.get_mut(owner_id) {
                    owner.elements.push(handle);
                }
            }
        });
    }
    handle
}

pub fn release_element(handle: Element) {
    with_renderer(|r| r.release_element(handle), ())
}

pub fn set_attribute(handle: Element, key: &str, value: &str) {
    with_renderer(|r| r.set_attribute(handle, key, value), ())
}

pub fn set_inline_styles(handle: Element, css: &str) {
    with_renderer(|r| r.set_inline_styles(handle, css), ())
}

pub fn set_update_list_info(handle: Element, count: i32) {
    with_renderer(|r| r.set_update_list_info(handle, count), ())
}

pub fn append_child(parent: Element, child: Element) {
    with_renderer(|r| r.append_child(parent, child), ());
    CHILDREN_OF.with_borrow_mut(|map| {
        map.entry(parent).or_default().push(child);
    });
    // Notify the component-mount machinery: if `child` is the body
    // root of a freshly-mounted `#[component]`, this is when its
    // MountSite learns where it landed (parent + previous sibling).
    crate::reactive::on_component_root_attached(parent, child);
}

pub fn remove_child(parent: Element, child: Element) {
    with_renderer(|r| r.remove_child(parent, child), ());
    CHILDREN_OF.with_borrow_mut(|map| {
        if let Some(children) = map.get_mut(&parent) {
            children.retain(|c| *c != child);
        }
    });
}

/// Insert `child` into `parent`'s child list at position `index`.
/// If `index >= current_len`, behaves like [`append_child`].
///
/// First-pass implementation: Lynx's C ABI doesn't yet expose
/// `insert_before` / `insert_at`, so we simulate ordered insertion
/// by detaching every sibling at or after `index`, appending the
/// new child, then re-appending the detached siblings in order. The
/// O(N) cost is fine for `<For>` reorders and #[component] remounts
/// where N is the parent's current child count. Replace with a
/// direct Lynx API once the bridge gains one.
pub fn insert_child_at(parent: Element, child: Element, index: usize) {
    let to_re_append: Vec<Element> = CHILDREN_OF.with_borrow(|map| {
        map.get(&parent)
            .map(|children| {
                if index >= children.len() {
                    Vec::new()
                } else {
                    children[index..].to_vec()
                }
            })
            .unwrap_or_default()
    });
    for c in &to_re_append {
        remove_child(parent, *c);
    }
    append_child(parent, child);
    for c in to_re_append {
        append_child(parent, c);
    }
}

/// Return the element handle that appears immediately before `child`
/// in `parent`'s child list, or `None` if `child` is the first child
/// or `parent` has no recorded children.
pub fn previous_sibling(parent: Element, child: Element) -> Option<Element> {
    CHILDREN_OF.with_borrow(|map| {
        let children = map.get(&parent)?;
        let idx = children.iter().position(|c| *c == child)?;
        if idx == 0 {
            None
        } else {
            Some(children[idx - 1])
        }
    })
}

/// Index of `child` in `parent`'s ordered child list, or `None` if
/// not tracked. Used by the wrapper-less remount path to re-insert
/// the new body root at the same position as the old one.
pub fn child_index(parent: Element, child: Element) -> Option<usize> {
    CHILDREN_OF.with_borrow(|map| {
        let children = map.get(&parent)?;
        children.iter().position(|c| *c == child)
    })
}

/// Snapshot of `parent`'s current ordered child list. Empty Vec if
/// the parent has no tracked children. Used by the batched
/// `remount_components_for` so it can compute the final desired
/// child order before any mutation churns the indices.
pub fn children_of(parent: Element) -> Vec<Element> {
    CHILDREN_OF.with_borrow(|map| map.get(&parent).cloned().unwrap_or_default())
}

/// Test/internal: clear the parent → children mirror. Call between
/// scenarios that share a thread (the production runtime never
/// needs this).
#[doc(hidden)]
pub fn __reset_children_mirror_for_tests() {
    CHILDREN_OF.with_borrow_mut(|map| map.clear());
}

pub fn set_event_listener(
    handle: Element,
    event_name: &str,
    bind_type: BindType,
    callback: Box<dyn Fn(WhiskerValue) + 'static>,
) {
    with_renderer(
        |r| r.set_event_listener(handle, event_name, bind_type, callback),
        (),
    )
}

/// Dispatch a reported event through the installed renderer's
/// reconstructed propagation chain. The driver's C entry point (the
/// bridge reporter forwards here) calls this. Returns whether the
/// event was consumed.
///
/// Planning runs under the renderer borrow; the listeners then fire
/// **after** the borrow is released, so a handler is free to mutate
/// signals / re-enter `view::*` without a re-entrant borrow panic.
pub fn dispatch_event(target_sign: i32, event_name: &str, body: WhiskerValue) -> bool {
    let plan = with_renderer(
        |r| r.plan_event_dispatch(target_sign, event_name, &body),
        EventDispatchPlan::default(),
    );
    for (listener, event) in plan.firings {
        listener(event);
    }
    plan.consumed
}

pub fn set_root(page: Element) {
    with_renderer(|r| r.set_root(page), ())
}

pub fn flush() {
    with_renderer(|r| r.flush(), ())
}

/// Opaque platform pointer for `handle`. Phase 7-Φ.H.2.3 — used by
/// `whisker-driver`'s `ElementRef::invoke` to call the C bridge
/// without leaking the bridge's `WhiskerElement*` type into the
/// runtime crate's public surface. Returns `0` if no renderer is
/// installed or the renderer doesn't have a native pointer for
/// `handle`.
pub fn module_component_ptr(handle: Element) -> usize {
    CURRENT_RENDERER.with_borrow(|slot| match slot.as_ref() {
        Some(r) => r.module_component_ptr(handle),
        None => 0,
    })
}
