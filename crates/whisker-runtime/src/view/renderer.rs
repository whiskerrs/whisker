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

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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
/// All mutating methods take `&self`, not `&mut self`. This is the
/// core of the re-entrancy fix for whisker issue #3: a native event
/// can fire *synchronously* during a renderer operation (e.g. Lynx
/// teardown inside [`remove_child`](Self::remove_child) triggering a
/// UIKit callback that dispatches a custom event), which re-enters the
/// renderer through [`dispatch_event`]. With `&self` methods the
/// thread-local [`CURRENT_RENDERER`] is held by a *shared* borrow in
/// [`with_renderer`], so a re-entrant call (also shared) is permitted
/// rather than panicking with "RefCell already borrowed". Renderers
/// own their mutable state behind per-field `RefCell`s and must scope
/// each field borrow so it does **not** span a re-entrant FFI call.
/// One `<list>` diff-action entry as it crosses the renderer: the
/// resolved (stable) item-key plus the per-item layout metadata Lynx's
/// adapter ingests from the action stream. For inserts `position` is
/// the ascending splice point into the post-removal list; for updates
/// it is the item's index in the FINAL list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItemAction {
    pub position: i32,
    pub key: String,
    /// Estimated main-axis size in px; `None` = native default.
    pub estimated_size: Option<i32>,
    pub full_span: bool,
    pub sticky_top: bool,
    pub sticky_bottom: bool,
    pub recyclable: bool,
}

pub trait DynRenderer {
    fn create_element(&self, tag: ElementTag) -> Element;
    /// Phase 7: tag-by-name dispatch for custom / xelement-style
    /// tags ("x-input", etc.) not in the built-in [`ElementTag`]
    /// enum. Returns [`Element::INVALID`] when the tag is unknown
    /// to Lynx's behaviour registry.
    fn create_element_by_name(&self, tag_name: &str) -> Element;
    fn release_element(&self, handle: Element);

    fn set_attribute(&self, handle: Element, key: &str, value: &str);
    /// Typed-attr variants. Lynx's prop dispatch on many UIs
    /// (`<list>`, `<scroll-view>`, …) gates branches on
    /// `value.IsNumber()` / `value.IsBool()` against the underlying
    /// `lepus::Value`, so a stringified attr from
    /// [`set_attribute`](Self::set_attribute) silently no-ops in
    /// those branches. Use these for any prop whose Lynx handler
    /// reads the value as anything other than a string. Default
    /// impls forward to the string path (good enough for test
    /// renderers that don't model the underlying type discrimination).
    fn set_attribute_int(&self, handle: Element, key: &str, value: i64) {
        self.set_attribute(handle, key, &value.to_string());
    }
    fn set_attribute_bool(&self, handle: Element, key: &str, value: bool) {
        self.set_attribute(handle, key, if value { "true" } else { "false" });
    }
    fn set_attribute_double(&self, handle: Element, key: &str, value: f64) {
        self.set_attribute(handle, key, &value.to_string());
    }
    fn set_inline_styles(&self, handle: Element, css: &str);

    /// Underlying Lynx sign (`impl_id`) for `handle`, or 0 if the
    /// renderer doesn't model signs (test renderers) or the handle
    /// is unknown. The list provider closure needs this to tell the
    /// C++ list which FiberElement to bind to an `index`. Whisker's
    /// own [`Element`] is a Vec index inside the renderer and is
    /// **not** the same number as Lynx's `impl_id`.
    fn element_sign(&self, _handle: Element) -> i32 {
        0
    }

    /// Hand a `<list>` element its item count so the bridge can build
    /// the `update-list-info` map (positional item-keys `w_<i>`) that
    /// Lynx's decoupled native list reads its items from. `item_keys`
    /// are the real (stable) item-keys in current order; `prev_count` is
    /// the previous call's item count (for the remove+insert diff). The
    /// `list` virtualizer calls this on every data update. Default no-op
    /// for test renderers that don't model list virtualisation.
    ///
    /// This is the FULL-REPLACE form — it severs every native item's
    /// identity, so the list cannot hold its scroll position across the
    /// update. The virtualizer prefers [`update_list_actions`]
    /// (Self::update_list_actions) and only falls back here.
    fn set_update_list_info(&self, _handle: Element, _item_keys: &[String], _prev_count: usize) {}

    /// Explicit `<list>` diff actions — the minimal-action form.
    /// `removals` are ascending indices into the PRE-update item-key
    /// list (applied first); `inserts` splice into the post-removal
    /// list at ascending positions, carrying the per-item layout
    /// metadata (the action stream is the ONLY channel Lynx's adapter
    /// ingests it from); `updates` refresh a SURVIVING item's metadata
    /// in place (`position` = its index in the FINAL list). Items
    /// mentioned in no action keep their native identity, which lets
    /// the list hold its scroll position across appends.
    ///
    /// Returns whether the renderer delivered the actions — `false`
    /// (the default, also reported when the loaded Lynx predates the
    /// capi) tells the virtualizer to fall back to the full-replace
    /// [`set_update_list_info`](Self::set_update_list_info).
    fn update_list_actions(
        &self,
        _handle: Element,
        _removals: &[i32],
        _inserts: &[ListItemAction],
        _updates: &[ListItemAction],
    ) -> bool {
        false
    }

    /// Set an object-valued attribute (`{obj[i].0: obj[i].1}` of doubles)
    /// — e.g. `<list>` `item-snap` {factor, offset}. Default no-op.
    fn set_attribute_object(&self, _handle: Element, _key: &str, _obj: &[(String, f64)]) {}

    /// Install a native item provider on a `<list>` element. The
    /// `provider`'s callbacks are invoked by Lynx's list machinery to
    /// fetch / recycle item elements on demand. Returns `true` if the
    /// install reached the bridge — `false` is reported when the
    /// renderer has no live native handle for `_handle` or doesn't
    /// model list virtualisation (test renderers default here).
    /// The default drops `provider` so test code doesn't leak boxed
    /// closures.
    fn install_list_native_item_provider(
        &self,
        _handle: Element,
        provider: super::list_provider::NativeItemProvider,
    ) -> bool {
        drop(provider);
        false
    }

    fn append_child(&self, parent: Element, child: Element);
    fn remove_child(&self, parent: Element, child: Element);

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
        &self,
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

    fn set_root(&self, page: Element);
    fn flush(&self);

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

    /// Reverse direction of [`CHILDREN_OF`]: child → its mirror
    /// parent. Maintained in lockstep with [`append_child`] /
    /// [`remove_child`]. We need this to walk *up* the mirror — the
    /// [phantom hoisting](create_phantom_element) machinery looks for
    /// the nearest non-phantom ancestor on every tree mutation, and
    /// the mirror-only direction is the only place that information
    /// lives.
    ///
    /// Each child has at most one parent (we don't model the DOM's
    /// "move from one parent to another" — every move is detach +
    /// re-attach through us). Missing-entry = the child is currently
    /// detached (no parent).
    static PARENT_OF: RefCell<HashMap<Element, Element>> =
        RefCell::new(HashMap::new());

    /// IDs allocated by [`create_phantom_element`]. A phantom is an
    /// Element that lives in [`CHILDREN_OF`] / [`PARENT_OF`] but is
    /// **not** present in Lynx. It behaves like a *transparent
    /// container*: any real child mounted under a phantom is hoisted
    /// to the phantom's nearest non-phantom ancestor in Lynx; if
    /// there is no such ancestor yet (the phantom is still
    /// unattached), the real children stay in the mirror only and
    /// land in Lynx when the phantom subtree is finally attached.
    static PHANTOM_ELEMENTS: RefCell<HashSet<Element>> =
        RefCell::new(HashSet::new());

    /// Monotonic counter for phantom IDs, starting at [`PHANTOM_BASE`]
    /// (`1 << 31`). The bridge renderer allocates real IDs from 0
    /// upward, so the two ranges can't realistically collide (a
    /// session would need 2 billion real elements before the real
    /// counter reached `PHANTOM_BASE`).
    static NEXT_PHANTOM_ID: Cell<u32> = const { Cell::new(PHANTOM_BASE) };
}

/// Phantom IDs occupy the high half of `u32`; real IDs start at 0
/// from the bridge renderer's counter, so the two ranges stay
/// disjoint without coordination.
pub const PHANTOM_BASE: u32 = 1 << 31;

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

/// Run `f` against the installed renderer under a **shared** borrow of
/// the [`CURRENT_RENDERER`] slot.
///
/// The shared borrow is the re-entrancy fix: `RefCell` permits any
/// number of simultaneous shared borrows, so if `f` (e.g. a
/// `remove_child` that synchronously tears down native views) causes a
/// native callback to re-enter Whisker through [`dispatch_event`] →
/// another `with_renderer`, that nested shared borrow is granted
/// instead of aborting with "already borrowed". This works because
/// every [`DynRenderer`] method now takes `&self` and owns its mutable
/// state behind interior `RefCell`s.
///
/// Slot *swapping* ([`install_renderer`] / [`uninstall_renderer`])
/// still uses `with_borrow_mut`; those are never called during
/// dispatch, so they can't conflict with an outstanding shared borrow.
fn with_renderer<R>(f: impl FnOnce(&dyn DynRenderer) -> R, default: R) -> R {
    CURRENT_RENDERER.with_borrow(|slot| match slot.as_ref() {
        Some(r) => f(r.as_ref()),
        None => {
            #[cfg(debug_assertions)]
            eprintln!("whisker-view: renderer call outside any installed renderer; ignored");
            default
        }
    })
}

// Free-function dispatch — what the `render!` macro and reactive
// effects call.

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
    // Register the element with the current reactive owner so
    // `Owner::dispose` releases it. Without this, `BridgeRenderer`'s
    // element map (and Lynx FiberElement refcounts) accumulate across
    // `<Show>` flips, `<For>` removals, and component remounts.
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
    if is_phantom(handle) {
        // Phantom never reached Lynx; tear down mirror state only.
        PHANTOM_ELEMENTS.with_borrow_mut(|s| {
            s.remove(&handle);
        });
        CHILDREN_OF.with_borrow_mut(|m| {
            m.remove(&handle);
        });
        PARENT_OF.with_borrow_mut(|m| {
            m.remove(&handle);
        });
        return;
    }
    with_renderer(|r| r.release_element(handle), ())
}

/// Allocate a phantom element — an opaque positional marker the
/// runtime registers in the mirror but **never** forwards to Lynx.
/// Phantoms behave as *transparent containers*: any real descendant
/// attached under a phantom is hoisted to the phantom's nearest
/// non-phantom mirror ancestor in Lynx, preserving source order.
///
/// Phantom IDs come from [`NEXT_PHANTOM_ID`], starting at
/// [`PHANTOM_BASE`] (`1 << 31`); the bridge renderer's real-element
/// counter starts at 0, so the two ranges are disjoint in any
/// realistic session.
///
/// Owner-tracking parity: the freshly-allocated phantom is added to
/// the currently-active reactive owner's `elements` list, so the
/// same dispose-cascade that releases real elements also reaches
/// phantoms — [`release_element`] detects the phantom case and
/// clears its mirror + set membership without touching Lynx.
///
/// **Use case**: the wrapper-less `fragment` builtin and the
/// `For` / `Show` control-flow components — each allocates one
/// phantom as its "transparent grouping" element so its reactive
/// children appear in the user's mirror tree as a group while
/// landing in Lynx as flat siblings of the surrounding non-phantom
/// container.
pub fn create_phantom_element() -> Element {
    let id = NEXT_PHANTOM_ID.with(|c| {
        let id = c.get();
        c.set(id.wrapping_add(1));
        id
    });
    let handle = Element::from_raw(id);
    PHANTOM_ELEMENTS.with_borrow_mut(|s| {
        s.insert(handle);
    });
    crate::reactive::with_runtime(|rt| {
        if let Some(owner_id) = rt.current_owner() {
            if let Some(owner) = rt.owners.get_mut(owner_id) {
                owner.elements.push(handle);
            }
        }
    });
    handle
}

/// Whether `handle` was allocated by [`create_phantom_element`].
/// Cheap thread-local lookup — the bridge dispatchers below call
/// this on every tree-mutation to decide whether to skip the FFI
/// step.
pub fn is_phantom(handle: Element) -> bool {
    if handle.id() < PHANTOM_BASE {
        return false;
    }
    PHANTOM_ELEMENTS.with_borrow(|s| s.contains(&handle))
}

/// Walk *up* the mirror from `start` (not including `start` itself)
/// until a non-phantom ancestor is found. Returns `None` if `start`
/// has no parent or the entire chain to the root is phantoms.
///
/// `start` may itself be either a phantom or a real element — the
/// function just looks at its ancestors. For the hoisting path the
/// caller usually passes the *parent* of the just-mutated child,
/// because the child's own type isn't what determines the
/// effective Lynx parent; the surrounding tree is.
fn nearest_real_ancestor(start: Element) -> Option<Element> {
    let mut current = start;
    loop {
        let parent = PARENT_OF.with_borrow(|m| m.get(&current).copied())?;
        if !is_phantom(parent) {
            return Some(parent);
        }
        current = parent;
    }
}

/// Count the number of *real* (non-phantom) elements reachable from
/// `root` through a strictly transparent path (phantom-only ancestors
/// between `root` and the reached element) that appear in DFS
/// pre-order before `target`. Used to compute the Lynx-side position
/// at which a newly-attached real element should land in
/// [`nearest_real_ancestor(target)`].
///
/// Excludes `root` itself; counts real descendants only. If `target`
/// is not under `root`, returns the total count (= "append at end").
fn count_real_descendants_before(root: Element, target: Element) -> usize {
    fn walk(node: Element, target: Element, count: &mut usize, found: &mut bool) {
        if *found {
            return;
        }
        let children = CHILDREN_OF.with_borrow(|m| m.get(&node).cloned().unwrap_or_default());
        for child in children {
            if *found {
                return;
            }
            if child == target {
                *found = true;
                return;
            }
            if is_phantom(child) {
                walk(child, target, count, found);
            } else {
                *count += 1;
            }
        }
    }
    let mut count = 0usize;
    let mut found = false;
    walk(root, target, &mut count, &mut found);
    count
}

/// DFS pre-order collect every real (non-phantom) descendant of
/// `root` reachable through a strictly transparent chain (phantom-
/// only ancestors). Used when a phantom subtree gets attached to a
/// real parent — we walk it and hand the real descendants to Lynx
/// in the right order.
fn collect_transparent_real_descendants(root: Element) -> Vec<Element> {
    let mut out = Vec::new();
    fn walk(node: Element, out: &mut Vec<Element>) {
        let children = CHILDREN_OF.with_borrow(|m| m.get(&node).cloned().unwrap_or_default());
        for child in children {
            if is_phantom(child) {
                walk(child, out);
            } else {
                out.push(child);
            }
        }
    }
    walk(root, &mut out);
    out
}

pub fn set_attribute(handle: Element, key: &str, value: &str) {
    if is_phantom(handle) {
        return; // phantoms carry no Lynx-side styling — silently no-op
    }
    with_renderer(|r| r.set_attribute(handle, key, value), ())
}

pub fn set_attribute_int(handle: Element, key: &str, value: i64) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(|r| r.set_attribute_int(handle, key, value), ())
}

pub fn set_attribute_bool(handle: Element, key: &str, value: bool) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(|r| r.set_attribute_bool(handle, key, value), ())
}

pub fn set_attribute_double(handle: Element, key: &str, value: f64) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(|r| r.set_attribute_double(handle, key, value), ())
}

pub fn set_inline_styles(handle: Element, css: &str) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(|r| r.set_inline_styles(handle, css), ())
}

/// See [`DynRenderer::element_sign`]. Returns 0 when no renderer is
/// installed (e.g. test setups using the mock renderer) or when
/// `handle` is a phantom (phantoms have no Lynx `impl_id`).
pub fn element_sign(handle: Element) -> i32 {
    if is_phantom(handle) {
        return 0;
    }
    with_renderer(|r| r.element_sign(handle), 0)
}

pub fn set_update_list_info(handle: Element, item_keys: &[String], prev_count: usize) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(
        |r| r.set_update_list_info(handle, item_keys, prev_count),
        (),
    )
}

pub fn update_list_actions(
    handle: Element,
    removals: &[i32],
    inserts: &[ListItemAction],
    updates: &[ListItemAction],
) -> bool {
    if is_phantom(handle) {
        return false;
    }
    with_renderer(
        |r| r.update_list_actions(handle, removals, inserts, updates),
        false,
    )
}

pub fn set_attribute_object(handle: Element, key: &str, obj: &[(String, f64)]) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(|r| r.set_attribute_object(handle, key, obj), ())
}

pub fn install_list_native_item_provider(
    handle: Element,
    provider: super::list_provider::NativeItemProvider,
) -> bool {
    if is_phantom(handle) {
        drop(provider);
        return false;
    }
    with_renderer(
        |r| r.install_list_native_item_provider(handle, provider),
        false,
    )
}

/// Append `child` as the last mirror child of `parent`. The Lynx-
/// side effect depends on whether either end of the edge is a
/// phantom:
///
///   - both real → the bridge sees `append_child(parent, child)`
///     exactly as before.
///   - phantom child → no FFI for `child` itself (it never reaches
///     Lynx); if `child` brings a transparent subtree of real
///     descendants with it, they're replayed into the nearest real
///     ancestor at the position the parent's transparent layout
///     puts them.
///   - phantom parent → `child` is hoisted up the phantom chain to
///     the nearest real ancestor (if any); inserted there at the
///     position the mirror order puts it.
///   - phantom parent with no real ancestor → no Lynx call at all;
///     the subtree is queued in the mirror only. When the topmost
///     phantom is later attached to a real ancestor, the same
///     replay path handles the queued descendants in source order.
pub fn append_child(parent: Element, child: Element) {
    // Mirror update — unconditional.
    CHILDREN_OF.with_borrow_mut(|map| {
        map.entry(parent).or_default().push(child);
    });
    PARENT_OF.with_borrow_mut(|map| {
        map.insert(child, parent);
    });

    // Lynx-side effect depends on phantom-ness of either end.
    let parent_is_phantom = is_phantom(parent);
    let child_is_phantom = is_phantom(child);
    if parent_is_phantom {
        // Hoist into the nearest real ancestor. When no real ancestor
        // exists yet (topmost phantom still detached), skip the
        // bridge step — the next attach will replay things.
        if let Some(real_anc) = nearest_real_ancestor(parent) {
            let to_attach: Vec<Element> = if child_is_phantom {
                collect_transparent_real_descendants(child)
            } else {
                vec![child]
            };
            for real in to_attach {
                let pos = count_real_descendants_before(real_anc, real);
                bridge_insert_or_append(real_anc, real, pos);
            }
        }
    } else if child_is_phantom {
        // Phantom child carries a transparent subtree; replay any
        // real descendants now in DFS pre-order.
        for real in collect_transparent_real_descendants(child) {
            let pos = count_real_descendants_before(parent, real);
            bridge_insert_or_append(parent, real, pos);
        }
    } else {
        with_renderer(|r| r.append_child(parent, child), ());
    }

    // Wrapper-less component mount handshake: if `child` is the body
    // root of a freshly-mounted `#[component]`, its MountSite now
    // learns where it landed (parent + previous sibling). Hot-reload
    // remount uses this to keep mount sites anchored across patches.
    crate::reactive::on_component_root_attached(parent, child);
}

/// Detach `child` from `parent` in the mirror. Lynx-side: any real
/// descendants of `child` (or `child` itself if it's real) are
/// removed from the nearest real ancestor.
pub fn remove_child(parent: Element, child: Element) {
    let parent_is_phantom = is_phantom(parent);
    let child_is_phantom = is_phantom(child);

    if parent_is_phantom {
        if let Some(real_anc) = nearest_real_ancestor(parent) {
            let to_detach: Vec<Element> = if child_is_phantom {
                collect_transparent_real_descendants(child)
            } else {
                vec![child]
            };
            for real in to_detach {
                with_renderer(|r| r.remove_child(real_anc, real), ());
            }
        }
    } else if child_is_phantom {
        for real in collect_transparent_real_descendants(child) {
            with_renderer(|r| r.remove_child(parent, real), ());
        }
    } else {
        with_renderer(|r| r.remove_child(parent, child), ());
    }

    CHILDREN_OF.with_borrow_mut(|map| {
        if let Some(children) = map.get_mut(&parent) {
            children.retain(|c| *c != child);
        }
    });
    PARENT_OF.with_borrow_mut(|map| {
        map.remove(&child);
    });
}

/// Internal helper: ask the bridge to place `real_child` at
/// `position` inside `real_parent`'s Lynx child list. The C ABI
/// doesn't expose `insert_at`, so we simulate by appending and
/// rotating: every real sibling that should sit *after* the child
/// (per the mirror's DFS pre-order of real-only descendants) is
/// detached and re-appended, ending up to the right of the new
/// child. O(siblings_to_move) bridge calls.
fn bridge_insert_or_append(real_parent: Element, real_child: Element, position: usize) {
    // Append lands the child at the tail in Lynx.
    with_renderer(|r| r.append_child(real_parent, real_child), ());

    // Mirror already includes the child at its target slot; compute
    // the DFS real-only order to find the siblings that need to be
    // rotated past it.
    let real_descendants = collect_transparent_real_descendants(real_parent);

    // Everything after `position` in mirror order must end up to the
    // right of `real_child` in Lynx — detach and re-append in order.
    // The "after" slice excludes `real_child` itself (at
    // `real_descendants[position]`).
    if position + 1 < real_descendants.len() {
        let to_move: Vec<Element> = real_descendants[position + 1..].to_vec();
        for sib in &to_move {
            with_renderer(|r| r.remove_child(real_parent, *sib), ());
        }
        for sib in &to_move {
            with_renderer(|r| r.append_child(real_parent, *sib), ());
        }
    }
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
    if is_phantom(handle) {
        // Phantoms aren't in Lynx's event chain.
        drop(callback);
        return;
    }
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
    if is_phantom(handle) {
        return 0;
    }
    CURRENT_RENDERER.with_borrow(|slot| match slot.as_ref() {
        Some(r) => r.module_component_ptr(handle),
        None => 0,
    })
}
