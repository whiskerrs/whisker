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

use super::handle::Element;
use crate::element::ElementTag;

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

    fn append_child(&mut self, parent: Element, child: Element);
    fn remove_child(&mut self, parent: Element, child: Element);

    fn set_event_listener(
        &mut self,
        handle: Element,
        event_name: &str,
        callback: Box<dyn Fn() + 'static>,
    );

    /// Variant that also passes the platform-side event-detail body
    /// (Lynx's `LynxEvent.generateEventBody` dict, serialised to a
    /// UTF-8 JSON string) to the callback. Used by
    /// `#[whisker::native_element]` for `on_<event>: String`
    /// prop declarations — `on_input` on `<input>` receives the
    /// updated text via this path.
    ///
    /// Renderers that don't support event payloads (in-memory test
    /// recorders, etc.) should forward to `set_event_listener` and
    /// invoke `callback` with an empty `String` when the event fires
    /// — same semantic as "empty payload" from the iOS bridge.
    fn set_event_listener_with_string_payload(
        &mut self,
        handle: Element,
        event_name: &str,
        callback: Box<dyn Fn(String) + 'static>,
    );

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
    fn native_element_ptr(&self, _handle: Element) -> usize {
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

pub fn set_event_listener(handle: Element, event_name: &str, callback: Box<dyn Fn() + 'static>) {
    with_renderer(|r| r.set_event_listener(handle, event_name, callback), ())
}

pub fn set_event_listener_with_string_payload(
    handle: Element,
    event_name: &str,
    callback: Box<dyn Fn(String) + 'static>,
) {
    with_renderer(
        |r| r.set_event_listener_with_string_payload(handle, event_name, callback),
        (),
    )
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
pub fn native_element_ptr(handle: Element) -> usize {
    CURRENT_RENDERER.with_borrow(|slot| match slot.as_ref() {
        Some(r) => r.native_element_ptr(handle),
        None => 0,
    })
}
