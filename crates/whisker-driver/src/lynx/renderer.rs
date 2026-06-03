//! `DynRenderer` impl that drives the C++ Lynx bridge.
//!
//! Must only be used from inside a `whisker_bridge_dispatch` callback
//! (i.e. on the Lynx TASM thread). The bootstrap installs an instance
//! of this type into the `whisker_runtime::view` thread-local before
//! invoking the user's `render!`-bearing fn, so the macro's
//! `create_element` / `set_attribute` / etc. calls land here.
//!
//! Translation layer: the public `Element` is a `u32` index
//! assigned by [`BridgeRenderer::create_element`]. Internally a
//! `Vec<Option<NonNull<WhiskerElement>>>` maps each index back to the
//! raw C pointer the bridge gave us. Released slots become `None`;
//! we don't currently reuse them (cheap; can be revisited if
//! per-frame churn ever matters).

use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;
use std::ptr::NonNull;
use std::rc::Rc;

use whisker_driver_sys::{self as ffi, WhiskerElement, WhiskerElementTag, WhiskerEngine};
use whisker_runtime::element::ElementTag;
use whisker_runtime::value::WhiskerValue;
use whisker_runtime::view::{BindType, DynRenderer, Element, EventDispatchPlan};

use super::propagation;

/// One registered listener: its propagation [`BindType`] plus the
/// closure to fire. `Rc` so the planner can clone it into the firing
/// list and the closure can run after the renderer borrow is released.
type Listener = (BindType, Rc<dyn Fn(WhiskerValue) + 'static>);

pub struct BridgeRenderer {
    engine: NonNull<WhiskerEngine>,
    /// Index → raw C element pointer. `None` means the slot has been
    /// released. Index assigned at `create_element` time, returned in
    /// the public `Element`.
    elements: Vec<Option<NonNull<WhiskerElement>>>,
    /// Child Lynx-sign → parent Lynx-sign, mirroring the attached
    /// tree. Populated in [`append_child`](Self::append_child),
    /// cleared in [`remove_child`](Self::remove_child) /
    /// [`release_element`](Self::release_element). The event-dispatch
    /// chain walk (`target → root`) follows these links — Lynx's
    /// reporter only hands us the target, so we reconstruct the
    /// ancestor chain ourselves.
    parent_sign: HashMap<i32, i32>,
    /// `(element sign, event name)` → listeners registered for it.
    /// Keyed by Lynx sign so the reporter's target sign (and the
    /// ancestor signs we walk to) look up directly. A given
    /// `(element, event)` can hold more than one listener when capture
    /// and bubble handlers are both registered (mirrors Lynx storing a
    /// handler per type).
    #[allow(clippy::type_complexity)]
    listeners: HashMap<(i32, String), Vec<Listener>>,
}

impl BridgeRenderer {
    /// # Safety
    /// `engine` must point to a valid `WhiskerEngine` returned from
    /// `whisker_bridge_engine_attach`. Caller guarantees the
    /// renderer is only used inside a `whisker_bridge_dispatch`
    /// callback for the same engine.
    pub unsafe fn from_raw(engine: *mut WhiskerEngine) -> Option<Self> {
        NonNull::new(engine).map(|engine| Self {
            engine,
            elements: Vec::new(),
            parent_sign: HashMap::new(),
            listeners: HashMap::new(),
        })
    }

    fn engine_ptr(&self) -> *mut WhiskerEngine {
        self.engine.as_ptr()
    }

    pub(crate) fn lookup(&self, handle: Element) -> Option<NonNull<WhiskerElement>> {
        self.elements
            .get(handle.id() as usize)
            .and_then(|slot| *slot)
    }

    /// The Lynx element sign for `handle`, or `None` if the handle is
    /// unknown / released. Routes through the bridge (the sign is
    /// `lynx_element_id` of the underlying FiberElement).
    fn sign_of(&self, handle: Element) -> Option<i32> {
        let ptr = self.lookup(handle)?;
        let sign = unsafe { ffi::whisker_bridge_element_sign(ptr.as_ptr()) };
        // 0 is the bridge's "null element" sentinel; a real element
        // sign is non-zero.
        (sign != 0).then_some(sign)
    }
}

fn map_tag(tag: ElementTag) -> WhiskerElementTag {
    match tag {
        ElementTag::Page => WhiskerElementTag::Page,
        ElementTag::View => WhiskerElementTag::View,
        ElementTag::Text => WhiskerElementTag::Text,
        ElementTag::RawText => WhiskerElementTag::RawText,
        ElementTag::ScrollView => WhiskerElementTag::ScrollView,
    }
}

impl DynRenderer for BridgeRenderer {
    fn create_element(&mut self, tag: ElementTag) -> Element {
        let raw = unsafe { ffi::whisker_bridge_create_element(self.engine_ptr(), map_tag(tag)) };
        let ptr = match NonNull::new(raw) {
            Some(p) => p,
            None => return Element::from_raw(u32::MAX),
        };
        let id = self.elements.len() as u32;
        self.elements.push(Some(ptr));
        Element::from_raw(id)
    }

    fn create_element_by_name(&mut self, tag_name: &str) -> Element {
        let Ok(c) = CString::new(tag_name) else {
            return Element::from_raw(u32::MAX);
        };
        let raw =
            unsafe { ffi::whisker_bridge_create_element_by_name(self.engine_ptr(), c.as_ptr()) };
        let ptr = match NonNull::new(raw) {
            Some(p) => p,
            None => return Element::from_raw(u32::MAX),
        };
        let id = self.elements.len() as u32;
        self.elements.push(Some(ptr));
        Element::from_raw(id)
    }

    fn element_sign(&self, handle: Element) -> i32 {
        // The list provider closure needs the Lynx `impl_id` to
        // return from `componentAtIndex`; Whisker's `Element` is
        // a Vec index inside this renderer, not the same number.
        self.sign_of(handle).unwrap_or(0)
    }

    fn release_element(&mut self, handle: Element) {
        // Resolve the sign before releasing so we can drop the element's
        // listeners + parent link. After release the underlying pointer
        // is gone, so `sign_of` would fail.
        let sign = self.sign_of(handle);
        if let Some(slot) = self.elements.get_mut(handle.id() as usize) {
            if let Some(ptr) = slot.take() {
                unsafe { ffi::whisker_bridge_release_element(ptr.as_ptr()) };
            }
        }
        if let Some(sign) = sign {
            self.parent_sign.remove(&sign);
            self.listeners.retain(|(s, _), _| *s != sign);
        }
    }

    fn set_attribute(&mut self, handle: Element, key: &str, value: &str) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(key_c) = CString::new(key) else { return };
        let Ok(value_c) = CString::new(value) else {
            return;
        };
        unsafe {
            ffi::whisker_bridge_set_attribute(ptr.as_ptr(), key_c.as_ptr(), value_c.as_ptr())
        };
    }

    fn set_inline_styles(&mut self, handle: Element, css: &str) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(css_c) = CString::new(css) else { return };
        unsafe { ffi::whisker_bridge_set_inline_styles(ptr.as_ptr(), css_c.as_ptr()) };
    }

    fn set_update_list_info(&mut self, handle: Element, count: i32) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        unsafe { ffi::whisker_bridge_list_set_item_count(ptr.as_ptr(), count) };
    }

    fn install_list_native_item_provider(
        &mut self,
        handle: Element,
        provider: whisker_runtime::view::list_provider::NativeItemProvider,
    ) -> bool {
        // Delegate to the inherent impl in `crate::lynx::list_provider`,
        // which holds the C trampolines + `Box<dyn FnMut>` lifetime
        // plumbing (kept there so the FFI machinery stays clustered).
        BridgeRenderer::install_list_native_item_provider(self, handle, provider)
    }

    fn append_child(&mut self, parent: Element, child: Element) {
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        unsafe { ffi::whisker_bridge_append_child(p.as_ptr(), c.as_ptr()) };
        // Mirror the attachment in sign space for the event chain walk.
        // (`insert_child_at` is built on append/remove, so it flows
        // through here too.)
        if let (Some(cs), Some(ps)) = (self.sign_of(child), self.sign_of(parent)) {
            self.parent_sign.insert(cs, ps);
        }
    }

    fn remove_child(&mut self, parent: Element, child: Element) {
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        unsafe { ffi::whisker_bridge_remove_child(p.as_ptr(), c.as_ptr()) };
        if let Some(cs) = self.sign_of(child) {
            self.parent_sign.remove(&cs);
        }
    }

    fn set_event_listener(
        &mut self,
        handle: Element,
        event_name: &str,
        bind_type: BindType,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    ) {
        // Listeners live here in the driver (keyed by Lynx sign), not in
        // the bridge: Whisker reconstructs propagation in Rust because
        // Lynx's reporter hook fires once at the target, before — and
        // bypassing — the engine's own capture/bubble chain (which
        // targets the absent JS runtime). See `plan_event_dispatch`.
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Some(sign) = self.sign_of(handle) else {
            return;
        };
        // Component-specific events (scroll / layout / uiappear / …) are
        // only EMITTED by Lynx's UI components when the element has a
        // handler bound for that name (they're gated on its event set).
        // Touch/gesture events bypass the event set — they flow through
        // the gesture pipeline to the reporter regardless — so we only
        // register a native handler for the non-gesture events, both to
        // unblock their emission and to avoid any double-fire on touch.
        if !is_gesture_event(event_name) {
            if let Ok(name_c) = CString::new(event_name) {
                unsafe {
                    ffi::whisker_bridge_set_native_event_handler(ptr.as_ptr(), name_c.as_ptr())
                };
            }
        }
        let entry = self
            .listeners
            .entry((sign, event_name.to_string()))
            .or_default();
        // Replace any handler of the SAME bind/catch/capture type for
        // this (element, event) — mirrors Lynx's per-type handler slot.
        // A different type (e.g. capture + bubble on one element) is
        // kept alongside.
        entry.retain(|(bt, _)| *bt != bind_type);
        entry.push((bind_type, Rc::from(callback)));
    }

    fn plan_event_dispatch(
        &self,
        target_sign: i32,
        event_name: &str,
        body: &WhiskerValue,
    ) -> EventDispatchPlan {
        // Reconstruct the response chain (target → root) from the
        // parent mirror — Lynx's reporter only hands us the target.
        let mut chain = vec![target_sign];
        let mut cur = target_sign;
        let mut guard = 0usize;
        while let Some(&parent) = self.parent_sign.get(&cur) {
            chain.push(parent);
            cur = parent;
            guard += 1;
            // Defensive: a malformed tree shouldn't spin forever.
            if guard > 4096 {
                break;
            }
        }

        let empty: Vec<Listener> = Vec::new();
        let (consumed, ordered) = propagation::plan(&chain, |sign| {
            self.listeners
                .get(&(sign, event_name.to_string()))
                .map(Vec::as_slice)
                .unwrap_or(&empty)
        });

        // Each listener receives the body with its `currentTarget`
        // rewritten to the element whose handler is firing (the
        // reporter's body always names the original target).
        let firings = ordered
            .into_iter()
            .map(|(sign, listener)| (listener, with_current_target(body, sign)))
            .collect();

        EventDispatchPlan { consumed, firings }
    }

    fn set_root(&mut self, page: Element) {
        let Some(ptr) = self.lookup(page) else { return };
        unsafe { ffi::whisker_bridge_set_root(self.engine_ptr(), ptr.as_ptr()) };
    }

    fn flush(&mut self) {
        unsafe { ffi::whisker_bridge_flush(self.engine_ptr()) };
    }

    fn module_component_ptr(&self, handle: Element) -> usize {
        // Cast the per-element `WhiskerElement*` to `usize` so the
        // runtime crate doesn't need to import bridge types. The
        // driver's `ElementRef::invoke` casts back to
        // `*mut WhiskerElement` before calling
        // `whisker_bridge_invoke_element_method`. Phase 7-Φ.H.2.3.
        self.lookup(handle)
            .map(|p| p.as_ptr() as usize)
            .unwrap_or(0)
    }
}

/// Whether `event_name` is a touch/gesture event that Lynx delivers to
/// the reporter through its gesture pipeline regardless of the
/// element's event set. These need no native handler registration;
/// every other (component-emitted) event does, or Lynx never fires it.
fn is_gesture_event(event_name: &str) -> bool {
    matches!(
        event_name,
        "tap" | "longpress" | "click" | "touchstart" | "touchmove" | "touchend" | "touchcancel"
    )
}

/// Clone `body`, rewriting its `currentTarget.uid` to `sign` — the
/// element whose handler is about to fire. Lynx's reporter only fills
/// the original target, so as we replay propagation up the chain each
/// listener gets a body naming *its* element as the current target.
/// Non-map bodies (e.g. a bodyless event's `Null`) pass through.
fn with_current_target(body: &WhiskerValue, sign: i32) -> WhiskerValue {
    let mut cloned = body.clone();
    if let WhiskerValue::Map(ref mut map) = cloned {
        let ct = map
            .entry("currentTarget".to_string())
            .or_insert_with(|| WhiskerValue::Map(BTreeMap::new()));
        match ct {
            WhiskerValue::Map(ct_map) => {
                ct_map.insert("uid".to_string(), WhiskerValue::Int(sign as i64));
            }
            other => {
                let mut ct_map = BTreeMap::new();
                ct_map.insert("uid".to_string(), WhiskerValue::Int(sign as i64));
                *other = WhiskerValue::Map(ct_map);
            }
        }
    }
    cloned
}

/// C entry point the bridge reporter forwards every reported event to
/// (registered via `whisker_bridge_register_event_dispatcher` at
/// bootstrap). Reconstructs the [`WhiskerValue`] body, runs it through
/// the installed renderer's propagation chain, and returns whether any
/// listener consumed it (so the reporter can tell Lynx to skip its
/// native chain).
///
/// Runs on the Lynx TASM thread, where the renderer is installed.
extern "C" fn whisker_event_dispatch_entry(
    target_sign: i32,
    event_name: *const std::os::raw::c_char,
    body: *const ffi::WhiskerValueRaw,
) -> bool {
    if event_name.is_null() {
        return false;
    }
    // SAFETY: the bridge passes a valid NUL-terminated event name for
    // the duration of the call.
    let name = match unsafe { std::ffi::CStr::from_ptr(event_name) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    // The bridge normalises a missing body to `WHISKER_VALUE_NULL`, so
    // `body` is non-null; guard anyway.
    let value = if body.is_null() {
        WhiskerValue::Null
    } else {
        // SAFETY: `body` points to a valid `WhiskerValueRaw` owned by
        // the bridge, valid for this call. `from_raw` copies it out.
        unsafe { crate::module::from_raw(&*body) }
    };
    whisker_runtime::view::dispatch_event(target_sign, name, value)
}

/// Register [`whisker_event_dispatch_entry`] with the bridge so the
/// platform reporter routes events through Whisker's reconstructed
/// propagation. Idempotent; called once from bootstrap.
pub(crate) fn register_event_dispatcher() {
    unsafe { ffi::whisker_bridge_register_event_dispatcher(whisker_event_dispatch_entry) };
}
