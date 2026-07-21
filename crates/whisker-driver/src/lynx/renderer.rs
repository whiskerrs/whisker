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

use std::cell::RefCell;
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

/// The bridge-backed [`DynRenderer`]. All mutating methods take
/// `&self` (see the trait docs) — the three mutable fields therefore
/// live behind **per-field** `RefCell`s rather than `&mut self`.
///
/// ## Re-entrancy contract (whisker #3)
///
/// A native event can fire *synchronously* from inside an FFI call
/// that this renderer makes — e.g. `whisker_bridge_remove_child`
/// triggers Lynx teardown → a UIKit callback → a custom event →
/// [`whisker_runtime::view::dispatch_event`] → back into *this*
/// renderer's [`plan_event_dispatch`](Self::plan_event_dispatch),
/// which reads `parent_sign` (chain reconstruction) and `listeners`.
///
/// Therefore **no field borrow may span a re-entrant FFI call.** Each
/// mutating method that calls a Lynx C API capable of dispatching an
/// event must: read/compute everything it needs under a *short*
/// borrow, **drop** the borrow, make the FFI call, then re-borrow if
/// it must mutate afterwards. Per-field `RefCell`s (not one big lock)
/// also keep independent fields from false-conflicting.
pub struct BridgeRenderer {
    engine: NonNull<WhiskerEngine>,
    /// Index → raw C element pointer. `None` means the slot has been
    /// released. Index assigned at `create_element` time, returned in
    /// the public `Element`.
    elements: RefCell<Vec<Option<NonNull<WhiskerElement>>>>,
    /// Child Lynx-sign → parent Lynx-sign, mirroring the attached
    /// tree. Populated in [`append_child`](Self::append_child),
    /// cleared in [`remove_child`](Self::remove_child) /
    /// [`release_element`](Self::release_element). The event-dispatch
    /// chain walk (`target → root`) follows these links — Lynx's
    /// reporter only hands us the target, so we reconstruct the
    /// ancestor chain ourselves.
    parent_sign: RefCell<HashMap<i32, i32>>,
    /// `(element sign, event name)` → listeners registered for it.
    /// Keyed by Lynx sign so the reporter's target sign (and the
    /// ancestor signs we walk to) look up directly. A given
    /// `(element, event)` can hold more than one listener when capture
    /// and bubble handlers are both registered (mirrors Lynx storing a
    /// handler per type).
    #[allow(clippy::type_complexity)]
    listeners: RefCell<HashMap<(i32, String), Vec<Listener>>>,
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
            elements: RefCell::new(Vec::new()),
            parent_sign: RefCell::new(HashMap::new()),
            listeners: RefCell::new(HashMap::new()),
        })
    }

    fn engine_ptr(&self) -> *mut WhiskerEngine {
        self.engine.as_ptr()
    }

    /// Resolve `handle` to its raw C pointer. Copies the pointer out
    /// from under a short borrow of `elements` so the returned value
    /// never keeps the borrow alive — callers are free to make FFI
    /// calls (which may re-enter and re-borrow `elements`) with it.
    pub(crate) fn lookup(&self, handle: Element) -> Option<NonNull<WhiskerElement>> {
        self.elements
            .borrow()
            .get(handle.id() as usize)
            .and_then(|slot| *slot)
    }

    /// The Lynx element sign for `handle`, or `None` if the handle is
    /// unknown / released. Routes through the bridge (the sign is
    /// `lynx_element_id` of the underlying FiberElement).
    ///
    /// `lookup`'s `elements` borrow is dropped before the FFI call (it
    /// only returns a copied pointer), so this never holds a borrow
    /// across `whisker_bridge_element_sign` — a pure getter that does
    /// not tear down views or dispatch events.
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
    fn create_element(&self, tag: ElementTag) -> Element {
        // FFI first, with NO `elements` borrow held — then borrow only
        // to register the new pointer. Element creation can't dispatch
        // an event, but keeping the borrow off the FFI call obeys the
        // renderer's uniform re-entrancy contract.
        let raw = unsafe { ffi::whisker_bridge_create_element(self.engine_ptr(), map_tag(tag)) };
        let ptr = match NonNull::new(raw) {
            Some(p) => p,
            None => return Element::from_raw(u32::MAX),
        };
        let mut elements = self.elements.borrow_mut();
        let id = elements.len() as u32;
        elements.push(Some(ptr));
        Element::from_raw(id)
    }

    fn create_element_by_name(&self, tag_name: &str) -> Element {
        let Ok(c) = CString::new(tag_name) else {
            return Element::from_raw(u32::MAX);
        };
        let raw =
            unsafe { ffi::whisker_bridge_create_element_by_name(self.engine_ptr(), c.as_ptr()) };
        let ptr = match NonNull::new(raw) {
            Some(p) => p,
            None => return Element::from_raw(u32::MAX),
        };
        let mut elements = self.elements.borrow_mut();
        let id = elements.len() as u32;
        elements.push(Some(ptr));
        Element::from_raw(id)
    }

    fn element_sign(&self, handle: Element) -> i32 {
        // The list provider closure needs the Lynx `impl_id` to
        // return from `componentAtIndex`; Whisker's `Element` is
        // a Vec index inside this renderer, not the same number.
        self.sign_of(handle).unwrap_or(0)
    }

    fn release_element(&self, handle: Element) {
        // RE-ENTRANT FFI: `whisker_bridge_release_element` tears down
        // the native view, which can synchronously dispatch an event
        // that re-enters `plan_event_dispatch` (reads `parent_sign` +
        // `listeners`). So: resolve the sign and take the pointer out
        // of `elements` under SHORT borrows, DROP them, then make the
        // FFI call, then re-borrow `parent_sign`/`listeners` to clean
        // up. No field borrow spans the release call.
        let sign = self.sign_of(handle);
        let ptr = self
            .elements
            .borrow_mut()
            .get_mut(handle.id() as usize)
            .and_then(|slot| slot.take());
        if let Some(ptr) = ptr {
            unsafe { ffi::whisker_bridge_release_element(ptr.as_ptr()) };
        }
        if let Some(sign) = sign {
            self.parent_sign.borrow_mut().remove(&sign);
            self.listeners.borrow_mut().retain(|(s, _), _| *s != sign);
        }
    }

    fn set_attribute(&self, handle: Element, key: &str, value: &str) {
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

    fn set_attribute_int(&self, handle: Element, key: &str, value: i64) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(key_c) = CString::new(key) else { return };
        unsafe { ffi::whisker_bridge_set_attribute_int(ptr.as_ptr(), key_c.as_ptr(), value) };
    }

    fn set_attribute_bool(&self, handle: Element, key: &str, value: bool) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(key_c) = CString::new(key) else { return };
        unsafe { ffi::whisker_bridge_set_attribute_bool(ptr.as_ptr(), key_c.as_ptr(), value) };
    }

    fn set_attribute_double(&self, handle: Element, key: &str, value: f64) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(key_c) = CString::new(key) else { return };
        unsafe { ffi::whisker_bridge_set_attribute_double(ptr.as_ptr(), key_c.as_ptr(), value) };
    }

    fn set_inline_styles(&self, handle: Element, css: &str) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(css_c) = CString::new(css) else { return };
        unsafe { ffi::whisker_bridge_set_inline_styles(ptr.as_ptr(), css_c.as_ptr()) };
    }

    fn set_update_list_info(&self, handle: Element, item_keys: &[String], prev_count: usize) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        // Own the C strings for the duration of the call; build a
        // NULL-safe `char*` pointer array. No borrow of any renderer field
        // spans the FFI call.
        let c_keys: Vec<std::ffi::CString> = item_keys
            .iter()
            .map(|k| std::ffi::CString::new(k.as_str()).unwrap_or_default())
            .collect();
        let key_ptrs: Vec<*const std::os::raw::c_char> =
            c_keys.iter().map(|c| c.as_ptr()).collect();
        unsafe {
            ffi::whisker_bridge_list_set_item_count(
                ptr.as_ptr(),
                prev_count as i32,
                key_ptrs.as_ptr(),
                item_keys.len() as i32,
            )
        };
    }

    fn update_list_actions(
        &self,
        handle: Element,
        removals: &[i32],
        inserts: &[whisker_runtime::view::ListItemAction],
        updates: &[whisker_runtime::view::ListItemAction],
    ) -> bool {
        let Some(ptr) = self.lookup(handle) else {
            return false;
        };
        // Own the C strings for the duration of the call; no renderer
        // field borrow spans the FFI.
        fn to_raw(
            actions: &[whisker_runtime::view::ListItemAction],
        ) -> (Vec<std::ffi::CString>, Vec<ffi::WhiskerListItemActionRaw>) {
            let keys: Vec<std::ffi::CString> = actions
                .iter()
                .map(|a| std::ffi::CString::new(a.key.as_str()).unwrap_or_default())
                .collect();
            let raw: Vec<ffi::WhiskerListItemActionRaw> = actions
                .iter()
                .zip(keys.iter())
                .map(|(a, k)| ffi::WhiskerListItemActionRaw {
                    position: a.position,
                    item_key: k.as_ptr(),
                    estimated_main_axis_px: a.estimated_size.unwrap_or(-1),
                    full_span: a.full_span as u8,
                    sticky_top: a.sticky_top as u8,
                    sticky_bottom: a.sticky_bottom as u8,
                    recyclable: a.recyclable as u8,
                })
                .collect();
            (keys, raw)
        }
        let (_insert_keys, insert_raw) = to_raw(inserts);
        let (_update_keys, update_raw) = to_raw(updates);
        unsafe {
            ffi::whisker_bridge_list_update_actions(
                ptr.as_ptr(),
                removals.as_ptr(),
                removals.len() as i32,
                insert_raw.as_ptr(),
                insert_raw.len() as i32,
                update_raw.as_ptr(),
                update_raw.len() as i32,
            )
        }
    }

    fn set_attribute_object(&self, handle: Element, key: &str, obj: &[(String, f64)]) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(key_c) = std::ffi::CString::new(key) else {
            return;
        };
        let c_keys: Vec<std::ffi::CString> = obj
            .iter()
            .map(|(k, _)| std::ffi::CString::new(k.as_str()).unwrap_or_default())
            .collect();
        let key_ptrs: Vec<*const std::os::raw::c_char> =
            c_keys.iter().map(|c| c.as_ptr()).collect();
        let values: Vec<f64> = obj.iter().map(|(_, v)| *v).collect();
        unsafe {
            ffi::whisker_bridge_set_attribute_object(
                ptr.as_ptr(),
                key_c.as_ptr(),
                key_ptrs.as_ptr(),
                values.as_ptr(),
                obj.len() as i32,
            )
        };
    }

    fn install_list_native_item_provider(
        &self,
        handle: Element,
        provider: whisker_runtime::view::list_provider::NativeItemProvider,
    ) -> bool {
        // Delegate to the inherent impl in `crate::lynx::list_provider`,
        // which holds the C trampolines + `Box<dyn FnMut>` lifetime
        // plumbing (kept there so the FFI machinery stays clustered).
        BridgeRenderer::install_list_native_item_provider(self, handle, provider)
    }

    fn append_child(&self, parent: Element, child: Element) {
        // `lookup` returns copied pointers (its `elements` borrow is
        // already dropped). The FFI append can synchronously dispatch
        // an event, so we must NOT hold a `parent_sign` borrow across
        // it: do the FFI first, then borrow `parent_sign` to record
        // the edge.
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        unsafe { ffi::whisker_bridge_append_child(p.as_ptr(), c.as_ptr()) };
        // Mirror the attachment in sign space for the event chain walk.
        // (`insert_child_at` is built on append/remove, so it flows
        // through here too.) `sign_of` holds no `parent_sign` borrow,
        // so computing the signs first and inserting after is safe.
        if let (Some(cs), Some(ps)) = (self.sign_of(child), self.sign_of(parent)) {
            self.parent_sign.borrow_mut().insert(cs, ps);
        }
    }

    fn remove_child(&self, parent: Element, child: Element) {
        // RE-ENTRANT FFI: `whisker_bridge_remove_child` tears down the
        // native subtree, which can synchronously dispatch an event
        // that re-enters `plan_event_dispatch` (walks `parent_sign`,
        // reads `listeners`). We resolve the child's sign BEFORE the
        // FFI (the pointer is still live then), hold NO field borrow
        // across the FFI call, and only borrow `parent_sign` to drop
        // the edge AFTER it returns.
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        let child_sign = self.sign_of(child);
        unsafe { ffi::whisker_bridge_remove_child(p.as_ptr(), c.as_ptr()) };
        if let Some(cs) = child_sign {
            self.parent_sign.borrow_mut().remove(&cs);
        }
    }

    fn supports_insert_before(&self) -> bool {
        // Feature-detect the optional Lynx symbol (bound by the loader
        // via dlsym; NULL on an engine that predates it).
        unsafe { ffi::whisker_bridge_supports_insert_child_before() != 0 }
    }

    fn insert_child_before(&self, parent: Element, child: Element, reference: Option<Element>) {
        // Same FFI-borrow discipline as `append_child`: do the FFI first
        // (it can synchronously dispatch), then record the sign edge.
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        // A `reference` that isn't currently mounted degrades to append
        // (null reference), matching the mirror's intent.
        let r_ptr = reference
            .and_then(|r| self.lookup(r))
            .map_or(std::ptr::null_mut(), |r| r.as_ptr());
        unsafe { ffi::whisker_bridge_insert_child_before(p.as_ptr(), c.as_ptr(), r_ptr) };
        if let (Some(cs), Some(ps)) = (self.sign_of(child), self.sign_of(parent)) {
            self.parent_sign.borrow_mut().insert(cs, ps);
        }
    }

    fn set_event_listener(
        &self,
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
        // The native-handler registration FFI runs with NO `listeners`
        // borrow held — it only enables emission and cannot dispatch.
        // We borrow `listeners` afterwards to record the closure.
        if !is_gesture_event(event_name) {
            if let Ok(name_c) = CString::new(event_name) {
                unsafe {
                    ffi::whisker_bridge_set_native_event_handler(ptr.as_ptr(), name_c.as_ptr())
                };
            }
        }
        let mut listeners = self.listeners.borrow_mut();
        let entry = listeners.entry((sign, event_name.to_string())).or_default();
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
        // The `parent_sign` borrow is scoped to the walk and dropped
        // before we touch `listeners`; planning makes no FFI call, so
        // these read-only borrows can't span a re-entrant op.
        let chain = {
            let parent_sign = self.parent_sign.borrow();
            let mut chain = vec![target_sign];
            let mut cur = target_sign;
            let mut guard = 0usize;
            while let Some(&parent) = parent_sign.get(&cur) {
                chain.push(parent);
                cur = parent;
                guard += 1;
                // Defensive: a malformed tree shouldn't spin forever.
                if guard > 4096 {
                    break;
                }
            }
            chain
        };

        // Hold a single shared borrow of `listeners` across the plan.
        // `propagation::plan` only reads (no FFI), so this borrow can't
        // conflict with a re-entrant op; releasing it after planning
        // means the listeners then fire (in `dispatch_event`) with no
        // renderer-field borrow held at all.
        let empty: Vec<Listener> = Vec::new();
        let listeners = self.listeners.borrow();
        let (consumed, ordered) = propagation::plan(&chain, |sign| {
            listeners
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

    fn set_root(&self, page: Element) {
        // `lookup`'s `elements` borrow is dropped before the FFI call
        // (it returns a copied pointer), so even if attaching the root
        // dispatches an event, no field borrow spans the call.
        let Some(ptr) = self.lookup(page) else { return };
        unsafe { ffi::whisker_bridge_set_root(self.engine_ptr(), ptr.as_ptr()) };
    }

    fn flush(&self) {
        // No field borrow held; if flush triggers native layout that
        // dispatches an event, the re-entrant op sees no outstanding
        // borrow of any renderer field.
        unsafe { ffi::whisker_bridge_flush(self.engine_ptr()) };
    }

    fn module_component_ptr(&self, handle: Element) -> usize {
        // Cast the per-element `WhiskerElement*` to `usize` so the
        // runtime crate doesn't need to import bridge types. The
        // driver's element-method dispatch casts back to
        // `*mut WhiskerElement` before calling the bridge.
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
    // Contain panics from user event handlers (`on_tap`, etc.) so a
    // bad `unwrap()` in a handler drops the event instead of unwinding
    // across the C ABI and aborting the app. Report "not consumed" on
    // panic so the bridge falls back to its native chain.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        whisker_runtime::view::dispatch_event(target_sign, name, value)
    })) {
        Ok(consumed) => consumed,
        Err(_) => {
            eprintln!("whisker: panic in event handler for `{name}`; event dropped");
            false
        }
    }
}

/// Register [`whisker_event_dispatch_entry`] with the bridge so the
/// platform reporter routes events through Whisker's reconstructed
/// propagation. Idempotent; called once from bootstrap.
pub(crate) fn register_event_dispatcher() {
    unsafe { ffi::whisker_bridge_register_event_dispatcher(whisker_event_dispatch_entry) };
}

// ----- Core-originated custom events -----------------------------------------
//
// The `<list>` scroll family (`scroll` / `scrolltoupper` / `scrolltolower`
// / `snap` / `layoutcomplete` / impression events) is generated inside
// Lynx's C++ core, not by the platform UI layer, so it never reaches the
// platform reporter. The bridge routes those events here through the
// fork's `lynx_shell_set_custom_event_callback` capi instead.
//
// Unlike reporter events (which arrive from the platform event stack,
// outside any engine call), these fire synchronously from INSIDE Lynx's
// scroll/layout pipeline — often while the renderer `RefCell` is
// borrowed (`renderer_flush` → Lynx layout → `layoutcomplete`). Running
// user handlers inline would re-enter the borrow and panic, so the
// entry queues the event and [`drain_custom_events`] dispatches the
// backlog at the top of the next frame tick.

thread_local! {
    /// Pending core-originated events, drained by `tick_frame`. TASM
    /// (main) thread only — both the enqueue (Lynx pipeline) and the
    /// drain (frame tick) run there.
    static CUSTOM_EVENT_QUEUE: std::cell::RefCell<Vec<(i32, String, WhiskerValue)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// C entry point for core-originated custom events (registered via
/// `whisker_bridge_register_custom_event_dispatcher`). Copies the event
/// out, queues it, and asks the host for a frame. Always reports
/// "consumed" — Whisker owns event delivery; there is no JS runtime for
/// the engine to forward to.
extern "C" fn whisker_custom_event_entry(
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
        Ok(s) => s.to_owned(),
        Err(_) => return false,
    };
    let value = if body.is_null() {
        WhiskerValue::Null
    } else {
        // SAFETY: `body` points to a valid `WhiskerValueRaw` owned by
        // the bridge, valid for this call. `from_raw` copies it out.
        unsafe { crate::module::from_raw(&*body) }
    };
    CUSTOM_EVENT_QUEUE.with(|q| q.borrow_mut().push((target_sign, name, value)));
    // Schedule a frame so the backlog drains promptly even when the
    // render loop is idle (no signal writes pending).
    whisker_runtime::host_wake::wake_runtime();
    true
}

/// Whether core-originated events are queued and waiting for a drain.
/// `tick_frame` checks this at the END of a frame: an event queued
/// mid-tick (a `layoutcomplete` fired by this tick's own
/// `renderer_flush`) has already had its `wake_runtime()` edge consumed
/// by the tick in progress, so without a re-wake it would sit in the
/// queue until some unrelated frame happened to run.
pub(crate) fn has_pending_custom_events() -> bool {
    CUSTOM_EVENT_QUEUE.with(|q| !q.borrow().is_empty())
}

/// Dispatch every queued core-originated event through the same
/// propagation path reporter events take. Called at the top of each
/// frame tick, before the reactive flush, so handler signal writes
/// render in the same frame. Events queued *during* this drain (e.g. a
/// `layoutcomplete` fired by a flush a handler triggered) wait for the
/// next frame — single pass, no loop-until-empty.
pub(crate) fn drain_custom_events() {
    let backlog = CUSTOM_EVENT_QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for (target_sign, name, value) in backlog {
        // Contain handler panics per-event (same contract as
        // `whisker_event_dispatch_entry`).
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            whisker_runtime::view::dispatch_event(target_sign, &name, value)
        }))
        .is_err()
        {
            eprintln!("whisker: panic in event handler for `{name}`; event dropped");
        }
    }
}

/// Register [`whisker_custom_event_entry`] and point Lynx's core
/// custom-event callback at the bridge. `engine` must be inside a
/// `whisker_bridge_dispatch` callback (TASM thread, fiber-arch
/// initialized). Returns whether the loaded Lynx supports the capi
/// (`false` on an older fork — list events stay dark, as before).
pub(crate) fn register_custom_event_dispatcher(engine: *mut ffi::WhiskerEngine) -> bool {
    unsafe {
        ffi::whisker_bridge_register_custom_event_dispatcher(whisker_custom_event_entry);
        ffi::whisker_bridge_install_custom_event_reporter(engine)
    }
}
