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

use std::ffi::{c_void, CString};
use std::ptr::NonNull;

use whisker_driver_sys::{self as ffi, WhiskerElement, WhiskerElementTag, WhiskerEngine};
use whisker_runtime::element::ElementTag;
use whisker_runtime::value::WhiskerValue;
use whisker_runtime::view::{DynRenderer, Element};

pub struct BridgeRenderer {
    engine: NonNull<WhiskerEngine>,
    /// Index → raw C element pointer. `None` means the slot has been
    /// released. Index assigned at `create_element` time, returned in
    /// the public `Element`.
    elements: Vec<Option<NonNull<WhiskerElement>>>,
    /// Owned per-event listener closures. Double-boxed so the inner
    /// `Box<dyn Fn>`'s fat-pointer address stays stable as the outer
    /// `Vec` reallocates — that address is the `user_data` the C
    /// bridge stores and hands back to the trampoline. Every event
    /// now carries a [`WhiskerValue`] payload (the unit / typed
    /// closures wrap into this single shape at the builder layer).
    #[allow(clippy::vec_box, clippy::type_complexity)]
    listeners: Vec<Box<Box<dyn Fn(WhiskerValue) + 'static>>>,
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
            listeners: Vec::new(),
        })
    }

    fn engine_ptr(&self) -> *mut WhiskerEngine {
        self.engine.as_ptr()
    }

    fn lookup(&self, handle: Element) -> Option<NonNull<WhiskerElement>> {
        self.elements
            .get(handle.id() as usize)
            .and_then(|slot| *slot)
    }
}

fn map_tag(tag: ElementTag) -> WhiskerElementTag {
    match tag {
        ElementTag::Page => WhiskerElementTag::Page,
        ElementTag::View => WhiskerElementTag::View,
        ElementTag::Text => WhiskerElementTag::Text,
        ElementTag::RawText => WhiskerElementTag::RawText,
        ElementTag::Image => WhiskerElementTag::Image,
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

    fn release_element(&mut self, handle: Element) {
        if let Some(slot) = self.elements.get_mut(handle.id() as usize) {
            if let Some(ptr) = slot.take() {
                unsafe { ffi::whisker_bridge_release_element(ptr.as_ptr()) };
            }
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

    fn append_child(&mut self, parent: Element, child: Element) {
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        unsafe { ffi::whisker_bridge_append_child(p.as_ptr(), c.as_ptr()) };
    }

    fn remove_child(&mut self, parent: Element, child: Element) {
        let Some(p) = self.lookup(parent) else { return };
        let Some(c) = self.lookup(child) else { return };
        unsafe { ffi::whisker_bridge_remove_child(p.as_ptr(), c.as_ptr()) };
    }

    fn set_event_listener(
        &mut self,
        handle: Element,
        event_name: &str,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    ) {
        let Some(ptr) = self.lookup(handle) else {
            return;
        };
        let Ok(name_c) = CString::new(event_name) else {
            return;
        };
        let outer: Box<Box<dyn Fn(WhiskerValue) + 'static>> = Box::new(callback);
        let raw = Box::as_ref(&outer) as *const Box<dyn Fn(WhiskerValue) + 'static> as *mut c_void;
        self.listeners.push(outer);
        // The bridge hands the event body across as a `WhiskerValueRaw`
        // tree (same wire as module args); the trampoline copies it
        // into an owned `WhiskerValue` via `from_raw`.
        unsafe {
            ffi::whisker_bridge_set_event_listener_with_value(
                ptr.as_ptr(),
                name_c.as_ptr(),
                rust_event_trampoline,
                raw,
            )
        };
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

extern "C" fn rust_event_trampoline(user_data: *mut c_void, payload: *const ffi::WhiskerValueRaw) {
    if user_data.is_null() {
        return;
    }
    // The bridge hands the event body as a `WhiskerValueRaw` tree
    // (NULL / `WHISKER_VALUE_NULL` = no body). Copy it into an owned
    // `WhiskerValue` — same value model as module args/returns.
    let value = if payload.is_null() {
        WhiskerValue::Null
    } else {
        // SAFETY: `payload` points to a valid `WhiskerValueRaw` owned
        // by the bridge, valid for the duration of this call
        // (documented contract). `from_raw` copies the data out.
        unsafe { crate::module::from_raw(&*payload) }
    };
    let cb = unsafe { &*(user_data as *const Box<dyn Fn(WhiskerValue) + 'static>) };
    cb(value);
}
