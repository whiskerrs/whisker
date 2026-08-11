//! `DynRenderer` impl that drives the C++ Lynx bridge.
//!
//! Must only be used from inside a `whisker_bridge_dispatch` callback
//! (i.e. on the Lynx TASM thread). The bootstrap installs an instance
//! of this type into the `whisker_runtime::view` thread-local before
//! invoking the user's `render!`-bearing fn, so the macro's
//! `create_element` / `set_attribute` / etc. calls land here.
//!
//! Translation layer: the public `Element` is a `u32` index assigned
//! by [`BridgeRenderer::create_element`], mapped back to the bridge's
//! raw C pointer through a `Vec<Option<NonNull<WhiskerElement>>>`.
//! Released slots become `None` and are not reused.

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
/// ## Re-entrancy contract
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
    /// tree. The event-dispatch chain walk (`target → root`) follows
    /// these links, because Lynx's reporter hands us only the target.
    parent_sign: RefCell<HashMap<i32, i32>>,
    /// `(element sign, event name)` → listeners, keyed by Lynx sign so
    /// the reporter's target sign (and the ancestors we walk to) look up
    /// directly. One pair can hold several listeners when capture and
    /// bubble handlers are both registered.
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
        // FFI first with no `elements` borrow held, then borrow only to
        // register the pointer — the uniform re-entrancy contract, even
        // though element creation can't itself dispatch.
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
        // `whisker_bridge_release_element` tears down the native view
        // and can synchronously dispatch an event that re-enters
        // `plan_event_dispatch`, so no field borrow may span it: resolve
        // the sign and pointer first, then FFI, then re-borrow to clean
        // up.
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
        // Own the C strings for the duration of the call, and build a
        // NULL-safe `char*` array.
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
        // Own the C strings for the duration of the call.
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
        // The C trampolines and `Box<dyn FnMut>` lifetime plumbing live
        // in `crate::lynx::list_provider`.
        BridgeRenderer::install_list_native_item_provider(self, handle, provider)
    }

    fn append_child(&self, parent: Element, child: Element) {
        // The FFI append can synchronously dispatch, so no
        // `parent_sign` borrow may span it — FFI first, record after.
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        unsafe { ffi::whisker_bridge_append_child(p.as_ptr(), c.as_ptr()) };
        // Mirror the attachment in sign space for the event chain walk.
        // `insert_child_at` is built on append/remove and flows through
        // here too.
        if let (Some(cs), Some(ps)) = (self.sign_of(child), self.sign_of(parent)) {
            self.parent_sign.borrow_mut().insert(cs, ps);
        }
    }

    fn remove_child(&self, parent: Element, child: Element) {
        // `whisker_bridge_remove_child` tears down the native subtree
        // and can synchronously dispatch a re-entrant event. Resolve the
        // child's sign BEFORE the FFI, while its pointer is still live,
        // and drop the edge only after it returns.
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        let child_sign = self.sign_of(child);
        unsafe { ffi::whisker_bridge_remove_child(p.as_ptr(), c.as_ptr()) };
        if let Some(cs) = child_sign {
            self.parent_sign.borrow_mut().remove(&cs);
        }
    }

    fn supports_insert_before(&self) -> bool {
        // whisker pins a Lynx (v3.8.0-whisker.13+) exporting the
        // positioned-insert symbol, and the loader binds it strictly, so
        // the bridge always drives the native path.
        true
    }

    fn insert_child_before(&self, parent: Element, child: Element, reference: Option<Element>) {
        // Same FFI-borrow discipline as `append_child`.
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        // An unmounted `reference` passes NULL, degrading to append.
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
        // Lynx's UI components only EMIT a component event (scroll /
        // layout / uiappear / …) when the element's event set has a
        // handler bound for that name. Touch/gesture events bypass the
        // event set entirely, so registering a native handler for them
        // would risk a double-fire without unblocking anything.
        if !is_gesture_event(event_name) {
            if let Ok(name_c) = CString::new(event_name) {
                unsafe {
                    ffi::whisker_bridge_set_native_event_handler(ptr.as_ptr(), name_c.as_ptr())
                };
            }
        }
        let mut listeners = self.listeners.borrow_mut();
        let entry = listeners.entry((sign, event_name.to_string())).or_default();
        // Replace any handler of the SAME bind/catch/capture type,
        // mirroring Lynx's per-type handler slot; a different type
        // (capture alongside bubble) is kept.
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
        // parent mirror — Lynx's reporter hands us only the target.
        let chain = {
            let parent_sign = self.parent_sign.borrow();
            let mut chain = vec![target_sign];
            let mut cur = target_sign;
            let mut guard = 0usize;
            while let Some(&parent) = parent_sign.get(&cur) {
                chain.push(parent);
                cur = parent;
                guard += 1;
                // A malformed tree must not spin forever.
                if guard > 4096 {
                    break;
                }
            }
            chain
        };

        // `propagation::plan` only reads and makes no FFI call, so this
        // borrow is safe to hold; releasing it before the listeners fire
        // means they run with no renderer-field borrow held at all.
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
        // No field borrow spans the call, so attaching the root may
        // safely dispatch.
        let Some(ptr) = self.lookup(page) else { return };
        unsafe { ffi::whisker_bridge_set_root(self.engine_ptr(), ptr.as_ptr()) };
    }

    fn flush(&self) {
        // No field borrow held — flush can trigger native layout that
        // dispatches re-entrantly.
        unsafe { ffi::whisker_bridge_flush(self.engine_ptr()) };
    }

    fn module_component_ptr(&self, handle: Element) -> usize {
        // `usize` so the runtime crate needn't import bridge types; the
        // driver's element-method dispatch casts it back.
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
    // The bridge normalises a missing body to `WHISKER_VALUE_NULL`.
    let value = if body.is_null() {
        WhiskerValue::Null
    } else {
        // SAFETY: `body` points to a valid `WhiskerValueRaw` owned by
        // the bridge, valid for this call. `from_raw` copies it out.
        unsafe { crate::module::from_raw(&*body) }
    };
    // Contain handler panics so a bad `unwrap()` drops the event
    // instead of unwinding across the C ABI. Reporting "not consumed"
    // lets the bridge fall back to its native chain.
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
/// initialized). Returns whether the loaded Lynx supports the capi;
/// `false` on an older fork, where list events stay dark.
pub(crate) fn register_custom_event_dispatcher(engine: *mut ffi::WhiskerEngine) -> bool {
    unsafe {
        ffi::whisker_bridge_register_custom_event_dispatcher(whisker_custom_event_entry);
        ffi::whisker_bridge_install_custom_event_reporter(engine)
    }
}
