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

use super::handle::ElementHandle;
use crate::element::ElementTag;

/// Object-safe renderer trait. The renderer owns whatever per-element
/// state it needs and answers in `ElementHandle` IDs.
///
/// Mirrors the shape of [`crate::renderer::Renderer`] but is
/// type-erased — the handle type is always [`ElementHandle`]. Existing
/// `R: Renderer` implementations bridge into here via a small adapter
/// that maintains its own `ElementHandle → R::ElementHandle` map.
pub trait DynRenderer {
    fn create_element(&mut self, tag: ElementTag) -> ElementHandle;
    fn release_element(&mut self, handle: ElementHandle);

    fn set_attribute(&mut self, handle: ElementHandle, key: &str, value: &str);
    fn set_inline_styles(&mut self, handle: ElementHandle, css: &str);

    fn append_child(&mut self, parent: ElementHandle, child: ElementHandle);
    fn remove_child(&mut self, parent: ElementHandle, child: ElementHandle);

    fn set_event_listener(
        &mut self,
        handle: ElementHandle,
        event_name: &str,
        callback: Box<dyn Fn() + 'static>,
    );

    fn set_root(&mut self, page: ElementHandle);
    fn flush(&mut self);
}

thread_local! {
    /// The active renderer for this thread. `None` outside any mount.
    ///
    /// Wrapped in `RefCell<Option<Box<dyn>>>` rather than holding the
    /// renderer directly so [`install_renderer`] can swap one out for
    /// another atomically and tests can run with no renderer installed
    /// (where dispatch functions silently no-op + warn).
    static CURRENT_RENDERER: RefCell<Option<Box<dyn DynRenderer>>> = const { RefCell::new(None) };
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

pub fn create_element(tag: ElementTag) -> ElementHandle {
    let handle = with_renderer(|r| r.create_element(tag), ElementHandle(u32::MAX));
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

pub fn release_element(handle: ElementHandle) {
    with_renderer(|r| r.release_element(handle), ())
}

pub fn set_attribute(handle: ElementHandle, key: &str, value: &str) {
    with_renderer(|r| r.set_attribute(handle, key, value), ())
}

pub fn set_inline_styles(handle: ElementHandle, css: &str) {
    with_renderer(|r| r.set_inline_styles(handle, css), ())
}

pub fn append_child(parent: ElementHandle, child: ElementHandle) {
    with_renderer(|r| r.append_child(parent, child), ())
}

pub fn remove_child(parent: ElementHandle, child: ElementHandle) {
    with_renderer(|r| r.remove_child(parent, child), ())
}

pub fn set_event_listener(
    handle: ElementHandle,
    event_name: &str,
    callback: Box<dyn Fn() + 'static>,
) {
    with_renderer(|r| r.set_event_listener(handle, event_name, callback), ())
}

pub fn set_root(page: ElementHandle) {
    with_renderer(|r| r.set_root(page), ())
}

pub fn flush() {
    with_renderer(|r| r.flush(), ())
}
