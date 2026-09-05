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
//! In production the bridge driver installs the Host renderer
//! once at startup and keeps it for the life of the process.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::handle::Element;
use crate::element::ElementTag;
use crate::event::Dataset;
use crate::value::WhiskerValue;
use whisker_engine::whisker_layout::LayoutParticipation;
use whisker_protocol::{Accessibility, ElementSchema, LayoutGeometry};
use whisker_style::SpecifiedStyle;

/// One internal resolved-layout notification.
///
/// Participation is kept outside [`LayoutGeometry`] because geometry is part
/// of the Host protocol while this state is consumed only by Rust control
/// primitives.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutObservation {
    /// Resolved box geometry for the retained node.
    pub geometry: LayoutGeometry,
    /// Occupied size including resolved margins, for virtual layout accounting.
    pub margin_box_size: whisker_engine::whisker_layout::LayoutSize,
    /// Whether the node belongs to the active layout tree.
    pub participation: LayoutParticipation,
}

/// Event-handler propagation type for the four supported handler kinds
/// (`bind` / `catch` / `capture-bind` /
/// `capture-catch`). The variant chosen when registering a listener is
/// what drives the Host event chain:
///
///   - **phase**: capture handlers fire on the way *down* (root →
///     target); bind/catch (bubble) handlers fire on the way *up*
///     (target → root).
///   - **stop**: a `catch` handler stops propagation after it fires;
///     a `bind` handler lets the event continue along the chain.
///
/// Discriminants are stable for renderer implementations that store the mode
/// as an integer.
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
    /// Whether any listener matched.
    pub consumed: bool,
    /// Listeners to invoke, in propagation order.
    pub firings: Vec<EventFiring>,
}

/// Object-safe renderer trait. The renderer owns whatever per-element
/// state it needs and answers in [`Element`] IDs.
///
/// All mutating methods take `&self`, not `&mut self`, so the renderer
/// survives re-entrancy: a Host event can fire *synchronously*
/// during a renderer operation (e.g. Host teardown inside
/// [`remove_child`](Self::remove_child) triggering a UIKit callback
/// that dispatches a custom event) and re-enter through
/// [`dispatch_event`]. With `&self` methods the thread-local
/// the thread-local renderer is held by a *shared* borrow in the private
/// `with_renderer` helper, so the nested call is granted instead of
/// panicking with "RefCell already borrowed". Renderers own their
/// mutable state behind per-field `RefCell`s and must scope each field
/// borrow so it does **not** span a re-entrant FFI call.
pub trait DynRenderer {
    fn create_element(&self, tag: ElementTag) -> Element;
    /// Tag-by-name dispatch for custom / xelement-style tags
    /// ("x-input", etc.) not in the built-in [`ElementTag`] enum.
    /// Returns a handle whose [`id`](Element::id) is `u32::MAX` when the
    /// tag is unknown to the element registry.
    fn create_element_by_name(&self, tag_name: &str) -> Element;
    /// Schema-carrying path used by `#[module_element]`. Renderers that
    /// negotiate schemas out of band can keep the default name-only behavior.
    fn create_element_by_schema(&self, schema: &ElementSchema) -> Element {
        self.create_element_by_name(&schema.name)
    }
    fn release_element(&self, handle: Element);

    fn set_attribute(&self, handle: Element, key: &str, value: &str);
    /// Typed attribute variants preserve value kind for module property
    /// handlers. Default implementations serialize to the string path for
    /// renderers that do not model type discrimination.
    fn set_attribute_int(&self, handle: Element, key: &str, value: i64) {
        self.set_attribute(handle, key, &value.to_string());
    }
    fn set_attribute_bool(&self, handle: Element, key: &str, value: bool) {
        self.set_attribute(handle, key, if value { "true" } else { "false" });
    }
    fn set_attribute_double(&self, handle: Element, key: &str, value: f64) {
        self.set_attribute(handle, key, &value.to_string());
    }
    /// Applies renderer-independent typed style and reports whether it was
    /// accepted. The default `false` is useful for lightweight test renderers;
    /// application-facing style APIs do not fall back to raw CSS.
    fn set_specified_style(&self, _handle: Element, _style: &SpecifiedStyle) -> bool {
        false
    }

    /// Stores the framework-level identifier used in event metadata.
    fn set_element_id(&self, _handle: Element, _id: String) {}

    /// Stores structured metadata surfaced on event targets.
    fn set_dataset(&self, _handle: Element, _dataset: Dataset) {}

    /// Replaces common accessibility semantics independently of element schema.
    fn set_accessibility(&self, _handle: Element, _accessibility: Accessibility) {}

    /// Sets the maximum number of lines for a plain-text element (`0` clears).
    fn set_text_max_lines(&self, _handle: Element, _max_lines: u32) {}

    /// Returns the current typed style when the renderer owns a retained Rust
    /// scene. Framework control flow uses this only for semantic validation;
    /// Hosts do not need to expose native presentation state.
    fn specified_style(&self, _handle: Element) -> Option<SpecifiedStyle> {
        None
    }

    /// Set an object-valued attribute (`{obj[i].0: obj[i].1}` of doubles)
    /// — e.g. `<list>` `item-snap` {factor, offset}. Default no-op.
    fn set_attribute_object(&self, _handle: Element, _key: &str, _obj: &[(String, f64)]) {}

    fn append_child(&self, parent: Element, child: Element);
    fn remove_child(&self, parent: Element, child: Element);

    /// Whether this renderer can insert a child before a reference
    /// sibling in one operation. When `false`, positioned insertion is
    /// simulated by append + rotate (see the private `insert_or_append`).
    /// Defaults to `false` so mock / host renderers opt in explicitly.
    fn supports_insert_before(&self) -> bool {
        false
    }

    /// Insert `child` into `parent` immediately before `reference`
    /// (`None` = append at the tail). Only called when
    /// [`supports_insert_before`](Self::supports_insert_before) is
    /// `true`; the default panics to catch a mis-wired caller.
    fn insert_child_before(&self, _parent: Element, _child: Element, _reference: Option<Element>) {
        unreachable!("insert_child_before called on a renderer without support");
    }

    /// Register `callback` for `event_name` on `handle`.
    ///
    /// The callback receives the event body Host hands the handler
    /// as a [`WhiskerValue`] tree (the same wire as module
    /// args/returns). A built-in builder's `on_<event>` method or a
    /// `#[whisker::module_element]` `on_<event>` prop wraps a
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

    /// Observes resolved Rust layout for framework control primitives.
    /// Ordinary renderers may ignore this; SurfaceRuntime reports after each
    /// successful Taffy pass without involving a Host event.
    fn observe_layout(
        &self,
        _handle: Element,
        _callback: Box<dyn Fn(LayoutObservation) + 'static>,
    ) {
    }

    /// Registers a callback that runs once after all per-element layout
    /// notifications for a completed layout pass. Renderers without retained
    /// layout notification batches may ignore it.
    fn observe_layout_batch_end(&self, _handle: Element, _callback: Box<dyn Fn() + 'static>) {}

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
    /// plans nothing (renderers without a Host event source); the
    /// Host bridge renderer overrides it.
    fn plan_event_dispatch(
        &self,
        _target_sign: i32,
        _event_name: &str,
        _body: &WhiskerValue,
    ) -> EventDispatchPlan {
        EventDispatchPlan::default()
    }

    /// Handles an element command through the retained semantic frame path.
    /// Renderers without command support leave the default `None`.
    fn invoke_element_command(
        &self,
        _handle: Element,
        _command: &str,
        _parameters: WhiskerValue,
    ) -> Option<Result<(), String>> {
        None
    }

    fn set_root(&self, root: Element);
    fn flush(&self);
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
    /// The mirror makes sibling/index queries deterministic without requiring
    /// a reverse query API from each Host.
    static CHILDREN_OF: RefCell<HashMap<Element, Vec<Element>>> =
        RefCell::new(HashMap::new());

    /// Reverse direction of [`CHILDREN_OF`]: child → its mirror
    /// parent. Maintained in lockstep with [`append_child`] /
    /// [`remove_child`] so [phantom hoisting](create_phantom_element)
    /// can walk *up* to the nearest non-phantom ancestor.
    ///
    /// Each child has at most one parent — every move is a detach +
    /// re-attach through us, never a DOM-style reparent. A missing
    /// entry means the child is currently detached.
    static PARENT_OF: RefCell<HashMap<Element, Element>> =
        RefCell::new(HashMap::new());

    /// IDs allocated by [`create_phantom_element`]. A phantom is an
    /// Element that lives in [`CHILDREN_OF`] / [`PARENT_OF`] but is
    /// **not** present in Host. It behaves like a *transparent
    /// container*: any real child mounted under a phantom is hoisted
    /// to the phantom's nearest non-phantom ancestor in Host; if
    /// there is no such ancestor yet (the phantom is still
    /// unattached), the real children stay in the mirror only and
    /// land in Host when the phantom subtree is finally attached.
    static PHANTOM_ELEMENTS: RefCell<HashSet<Element>> =
        RefCell::new(HashSet::new());

    /// Monotonic counter for phantom IDs, starting at [`PHANTOM_BASE`].
    static NEXT_PHANTOM_ID: Cell<u32> = const { Cell::new(PHANTOM_BASE) };
}

pub(crate) struct ViewRuntimeState {
    children: HashMap<Element, Vec<Element>>,
    parents: HashMap<Element, Element>,
    phantoms: HashSet<Element>,
    next_phantom_id: u32,
}

impl ViewRuntimeState {
    pub(crate) fn new() -> Self {
        Self {
            children: HashMap::new(),
            parents: HashMap::new(),
            phantoms: HashSet::new(),
            next_phantom_id: PHANTOM_BASE,
        }
    }
}

pub(crate) fn swap_runtime_state(state: &mut ViewRuntimeState) {
    CHILDREN_OF.with_borrow_mut(|active| std::mem::swap(active, &mut state.children));
    PARENT_OF.with_borrow_mut(|active| std::mem::swap(active, &mut state.parents));
    PHANTOM_ELEMENTS.with_borrow_mut(|active| std::mem::swap(active, &mut state.phantoms));
    NEXT_PHANTOM_ID.with(|active| {
        let current = active.replace(state.next_phantom_id);
        state.next_phantom_id = current;
    });
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

/// Restore `prev` (typically what [`install_renderer`] handed back) as
/// the current renderer, dropping whatever is installed now. Passing
/// `None` leaves the slot empty, after which dispatch calls warn (in
/// debug) and no-op.
pub fn uninstall_renderer(prev: Option<Box<dyn DynRenderer>>) {
    CURRENT_RENDERER.with_borrow_mut(|slot| *slot = prev);
}

/// Run `f` with `r` temporarily installed as the current renderer.
/// Restores whatever was previously installed when `f` returns
/// (including the `None` state). Useful for tests + scoped
/// rendering.
pub fn with_installed_renderer<R>(r: Box<dyn DynRenderer>, f: impl FnOnce() -> R) -> R {
    let prev = install_renderer(r);
    struct Restore(Option<Option<Box<dyn DynRenderer>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _current = CURRENT_RENDERER.with_borrow_mut(|slot| slot.take());
            if let Some(previous) = self.0.take().flatten() {
                let _ = install_renderer(previous);
            }
        }
    }
    let _restore = Restore(Some(prev));
    f()
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
/// The borrow must stay shared: if `f` (e.g. a `remove_child` that
/// synchronously tears down native views) causes a native callback to
/// re-enter Whisker through [`dispatch_event`] → another
/// `with_renderer`, `RefCell` grants the nested shared borrow instead
/// of aborting with "already borrowed".
///
/// Slot *swapping* ([`install_renderer`] / [`uninstall_renderer`]) uses
/// `with_borrow_mut`; those are never called during dispatch, so they
/// can't conflict with an outstanding shared borrow.
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

/// Allocate an element by tag name, registering it with the current
/// reactive owner so `Owner::dispose` releases it.
pub fn create_element_by_name(tag_name: &str) -> Element {
    let handle = with_renderer(|r| r.create_element_by_name(tag_name), Element(u32::MAX));
    if handle.id() != u32::MAX {
        crate::reactive::with_runtime(|rt| {
            if let Some(owner_id) = rt.current_owner()
                && let Some(owner) = rt.owners.get_mut(owner_id)
            {
                owner.elements.push(handle);
            }
        });
    }
    handle
}

/// Allocate a module element while making its Host-independent schema
/// available to the active retained renderer.
pub fn create_element_by_schema(schema: &ElementSchema) -> Element {
    let handle = with_renderer(
        |renderer| renderer.create_element_by_schema(schema),
        Element(u32::MAX),
    );
    if handle.id() != u32::MAX {
        crate::reactive::with_runtime(|runtime| {
            if let Some(owner_id) = runtime.current_owner()
                && let Some(owner) = runtime.owners.get_mut(owner_id)
            {
                owner.elements.push(handle);
            }
        });
    }
    handle
}

pub fn create_element(tag: ElementTag) -> Element {
    let handle = with_renderer(|r| r.create_element(tag), Element(u32::MAX));
    // Register with the current reactive owner so `Owner::dispose`
    // releases it — otherwise the renderer's element map and Host element
    // records accumulate across every `<Show>` flip,
    // `<For>` removal, and component remount.
    if handle.id() != u32::MAX {
        crate::reactive::with_runtime(|rt| {
            if let Some(owner_id) = rt.current_owner()
                && let Some(owner) = rt.owners.get_mut(owner_id)
            {
                owner.elements.push(handle);
            }
        });
    }
    handle
}

pub fn release_element(handle: Element) {
    if is_phantom(handle) {
        // Phantom never reached Host; tear down mirror state only.
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

/// Stores a framework-level element identifier.
pub fn set_element_id(handle: Element, id: String) {
    with_renderer(|renderer| renderer.set_element_id(handle, id), ())
}

/// Stores structured event metadata for an element.
pub fn set_dataset(handle: Element, dataset: Dataset) {
    with_renderer(|renderer| renderer.set_dataset(handle, dataset), ())
}

/// Replaces the common accessibility semantics for an element.
pub fn set_accessibility(handle: Element, accessibility: Accessibility) {
    with_renderer(
        |renderer| renderer.set_accessibility(handle, accessibility),
        (),
    )
}

/// Sets the maximum number of lines for a plain-text element.
pub fn set_text_max_lines(handle: Element, max_lines: u32) {
    with_renderer(
        |renderer| renderer.set_text_max_lines(handle, max_lines),
        (),
    )
}

/// Allocate a phantom element — an opaque positional marker the
/// runtime registers in the mirror but **never** forwards to Host.
/// Phantoms behave as *transparent containers*: any real descendant
/// attached under a phantom is hoisted to the phantom's nearest
/// non-phantom mirror ancestor in Host, preserving source order.
///
/// The phantom joins the current reactive owner's `elements` list like
/// a real element, so the same dispose-cascade reaches it;
/// [`release_element`] then clears its mirror state without touching
/// Host.
///
/// **Use case**: the wrapper-less `fragment` builtin and the
/// `For` / `Show` control-flow components — each allocates one
/// phantom as its "transparent grouping" element so its reactive
/// children appear in the mirror tree as a group while landing in Host
/// as flat siblings of the surrounding non-phantom container.
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
        if let Some(owner_id) = rt.current_owner()
            && let Some(owner) = rt.owners.get_mut(owner_id)
        {
            owner.elements.push(handle);
        }
    });
    handle
}

/// Whether `handle` was allocated by [`create_phantom_element`]. The
/// dispatchers below call this on every tree mutation to decide
/// whether to skip the FFI step.
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
/// The hoisting path passes the *parent* of the just-mutated child:
/// what determines the effective Host parent is the surrounding tree,
/// not the child's own type.
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
/// pre-order before `target`. Used to compute the Host-side position
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
/// real parent — we walk it and hand the real descendants to Host
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
        return; // phantoms carry no Host-side styling — silently no-op
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

/// Attempts to apply renderer-independent typed style.
pub fn set_specified_style(handle: Element, style: &SpecifiedStyle) -> bool {
    if is_phantom(handle) {
        return true;
    }
    with_renderer(
        |renderer| renderer.set_specified_style(handle, style),
        false,
    )
}

#[doc(hidden)]
pub fn specified_style(handle: Element) -> Option<SpecifiedStyle> {
    if is_phantom(handle) {
        return None;
    }
    with_renderer(|renderer| renderer.specified_style(handle), None)
}

pub fn set_attribute_object(handle: Element, key: &str, obj: &[(String, f64)]) {
    if is_phantom(handle) {
        return;
    }
    with_renderer(|r| r.set_attribute_object(handle, key, obj), ())
}

/// Append `child` as the last mirror child of `parent`. The Host-
/// side effect depends on whether either end of the edge is a
/// phantom:
///
///   - both real → the bridge sees `append_child(parent, child)`
///     exactly as before.
///   - phantom child → no FFI for `child` itself (it never reaches
///     Host); if `child` brings a transparent subtree of real
///     descendants with it, they're replayed into the nearest real
///     ancestor at the position the parent's transparent layout
///     puts them.
///   - phantom parent → `child` is hoisted up the phantom chain to
///     the nearest real ancestor (if any); inserted there at the
///     position the mirror order puts it.
///   - phantom parent with no real ancestor → no Host call at all;
///     the subtree is queued in the mirror only. When the topmost
///     phantom is later attached to a real ancestor, the same
///     replay path handles the queued descendants in source order.
pub fn append_child(parent: Element, child: Element) {
    CHILDREN_OF.with_borrow_mut(|map| {
        map.entry(parent).or_default().push(child);
    });
    PARENT_OF.with_borrow_mut(|map| {
        map.insert(child, parent);
    });

    if !realize_hoisted_child(parent, child) {
        with_renderer(|r| r.append_child(parent, child), ());
    }

    // If `child` is the body root of a freshly-mounted `#[component]`,
    // its MountSite learns where it landed — hot-reload remount reads
    // that anchor to put the replacement back in the same place.
    crate::reactive::on_component_root_attached(parent, child);
}

/// Realize `child` — already placed in the mirror at its target
/// position — into the real Host tree, hoisting/replaying real
/// descendants when either end is a phantom. Reads `child`'s *current*
/// mirror position, so it serves both a tail append ([`append_child`])
/// and a positioned insert ([`insert_child_at`]) unchanged.
///
/// Returns `true` when a phantom was involved (and handled here);
/// `false` when both ends are real, leaving the caller to do the direct
/// real-to-real attach (`r.append_child` for a tail append, or a
/// positioned [`insert_or_append`] for a mid-list insert).
fn realize_hoisted_child(parent: Element, child: Element) -> bool {
    let parent_is_phantom = is_phantom(parent);
    let child_is_phantom = is_phantom(child);
    if parent_is_phantom {
        // No real ancestor yet (topmost phantom still detached) means
        // nothing to tell Host — the next attach replays this subtree.
        if let Some(real_anc) = nearest_real_ancestor(parent) {
            let to_attach: Vec<Element> = if child_is_phantom {
                collect_transparent_real_descendants(child)
            } else {
                vec![child]
            };
            // Back-to-front: each child's positioned-insert reference
            // is its next real sibling, so it must already be in the
            // Host tree. A forward pass references batch-mates that
            // aren't on-device yet and Host drops all but the last.
            for real in to_attach.into_iter().rev() {
                let pos = count_real_descendants_before(real_anc, real);
                insert_or_append(real_anc, real, pos);
            }
        }
        true
    } else if child_is_phantom {
        // Back-to-front for the same reason as the branch above.
        for real in collect_transparent_real_descendants(child)
            .into_iter()
            .rev()
        {
            let pos = count_real_descendants_before(parent, real);
            insert_or_append(parent, real, pos);
        }
        true
    } else {
        false
    }
}

/// Detach `child` from `parent` in the mirror. Host-side: any real
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

/// Internal helper: place `real_child` at `position` inside
/// `real_parent`'s Host child list.
///
/// The mirror already includes `real_child` at `position` in the
/// parent's real-only DFS pre-order, so the element that should sit
/// *after* it in Host is the next real descendant (`position + 1`), if
/// any — that's the reference node for a positioned insert.
///
/// A renderer reporting `supports_insert_before` gets one native call
/// with no sibling churn. The append + rotate fallback (test mocks, a
/// Host too old for the capi) detaches and re-appends every real
/// sibling that must sit after `real_child`, which re-anchors stateful
/// native siblings — a focused `<input>` loses focus.
fn insert_or_append(real_parent: Element, real_child: Element, position: usize) {
    let real_descendants = collect_transparent_real_descendants(real_parent);
    let reference = real_descendants.get(position + 1).copied();

    if with_renderer(|r| r.supports_insert_before(), false) {
        with_renderer(
            |r| r.insert_child_before(real_parent, real_child, reference),
            (),
        );
        return;
    }

    // `real_descendants` already includes `real_child` at `position`,
    // so the siblings to rotate past it are everything after it.
    with_renderer(|r| r.append_child(real_parent, real_child), ());
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

/// Places `child` at mirror `index` in `parent`'s child list (appends
/// when `index >= len`). The following siblings are **not touched** on
/// the Host side: `child` is realized with a positioned insert
/// (the private `insert_or_append` → native `insert_before` on Host), so a
/// stateful native sibling (a focused `<input>`, a scrolled list) keeps
/// its state.
pub fn insert_child_at(parent: Element, child: Element, index: usize) {
    CHILDREN_OF.with_borrow_mut(|map| {
        let children = map.entry(parent).or_default();
        if index < children.len() {
            children.insert(index, child);
        } else {
            children.push(child);
        }
    });
    PARENT_OF.with_borrow_mut(|map| {
        map.insert(child, parent);
    });

    if !realize_hoisted_child(parent, child) {
        let pos = count_real_descendants_before(parent, child);
        insert_or_append(parent, child, pos);
    }

    crate::reactive::on_component_root_attached(parent, child);
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
        // Phantoms aren't in Host's event chain.
        drop(callback);
        return;
    }
    with_renderer(
        |r| r.set_event_listener(handle, event_name, bind_type, callback),
        (),
    )
}

/// Registers an internal resolved-layout observer on one real element.
pub fn observe_layout(handle: Element, callback: Box<dyn Fn(LayoutObservation) + 'static>) {
    if is_phantom(handle) {
        drop(callback);
        return;
    }
    with_renderer(|renderer| renderer.observe_layout(handle, callback), ())
}

/// Registers an internal callback at the end of each completed resolved-layout
/// notification batch. The callback is owned by `handle`, so removing that
/// element also removes the registration.
pub fn observe_layout_batch_end(handle: Element, callback: Box<dyn Fn() + 'static>) {
    if is_phantom(handle) {
        drop(callback);
        return;
    }
    with_renderer(
        |renderer| renderer.observe_layout_batch_end(handle, callback),
        (),
    )
}

/// Gives the installed renderer the first opportunity to handle an element
/// command. `None` asks the driver to use its legacy bridge path.
#[doc(hidden)]
pub fn try_invoke_element_command(
    handle: Element,
    command: &str,
    parameters: WhiskerValue,
) -> Option<Result<(), String>> {
    if is_phantom(handle) {
        return None;
    }
    with_renderer(
        |renderer| renderer.invoke_element_command(handle, command, parameters),
        None,
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

pub fn set_root(root: Element) {
    with_renderer(|renderer| renderer.set_root(root), ())
}

pub fn flush() {
    with_renderer(|r| r.flush(), ())
}
