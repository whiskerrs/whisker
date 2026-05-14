//! Renderer impl that calls into the C++ bridge.
//!
//! Must only be used from inside a `whisker_bridge_dispatch` callback (i.e.
//! on the Lynx TASM thread).

use std::ffi::{c_void, CString};
use std::ptr::NonNull;
use whisker_driver_sys::{self as ffi, WhiskerElement, WhiskerElementTag, WhiskerEngine};
use whisker_runtime::element::ElementTag;
use whisker_runtime::renderer::Renderer;

pub struct BridgeRenderer {
    engine: NonNull<WhiskerEngine>,
    /// Owned closures behind every event listener we registered with
    /// the bridge. The double `Box<Box<…>>` is intentional and *not*
    /// the `clippy::vec_box` smell: `Box<dyn Fn()>` is a fat (data +
    /// vtable) pointer that doesn't fit in the C ABI's
    /// `user_data: *mut c_void`. The outer `Box` gives us a stable
    /// heap location whose *thin* address we can hand the bridge;
    /// the inner `Box<dyn Fn()>` is the actual erased closure. Vec
    /// because each registration leaks one entry — fine for the demo
    /// lifetime; a future iteration could reclaim when listeners are
    /// replaced.
    #[allow(clippy::vec_box)]
    listeners: Vec<Box<Box<dyn Fn() + 'static>>>,
}

impl BridgeRenderer {
    /// # Safety
    /// `engine` must point to a valid WhiskerEngine returned from
    /// `whisker_bridge_engine_attach`. Caller must ensure this renderer is
    /// only used inside a `whisker_bridge_dispatch` callback for the same
    /// engine.
    pub unsafe fn from_raw(engine: *mut WhiskerEngine) -> Option<Self> {
        NonNull::new(engine).map(|engine| Self {
            engine,
            listeners: Vec::new(),
        })
    }

    fn engine_ptr(&self) -> *mut WhiskerEngine {
        self.engine.as_ptr()
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElementHandle(*mut WhiskerElement);

// SAFETY: handles are only used from a single thread (TASM).
unsafe impl Send for ElementHandle {}

impl Renderer for BridgeRenderer {
    type ElementHandle = ElementHandle;

    fn create_element(&mut self, tag: ElementTag) -> Self::ElementHandle {
        let raw = unsafe { ffi::whisker_bridge_create_element(self.engine_ptr(), map_tag(tag)) };
        ElementHandle(raw)
    }

    fn release_element(&mut self, handle: Self::ElementHandle) {
        unsafe { ffi::whisker_bridge_release_element(handle.0) }
    }

    fn set_attribute(&mut self, handle: Self::ElementHandle, key: &str, value: &str) {
        let key_c = match CString::new(key) {
            Ok(c) => c,
            Err(_) => return,
        };
        let value_c = match CString::new(value) {
            Ok(c) => c,
            Err(_) => return,
        };
        unsafe { ffi::whisker_bridge_set_attribute(handle.0, key_c.as_ptr(), value_c.as_ptr()) }
    }

    fn set_inline_styles(&mut self, handle: Self::ElementHandle, css: &str) {
        let css_c = match CString::new(css) {
            Ok(c) => c,
            Err(_) => return,
        };
        unsafe { ffi::whisker_bridge_set_inline_styles(handle.0, css_c.as_ptr()) }
    }

    fn append_child(&mut self, parent: Self::ElementHandle, child: Self::ElementHandle) {
        unsafe { ffi::whisker_bridge_append_child(parent.0, child.0) }
    }

    fn remove_child(&mut self, parent: Self::ElementHandle, child: Self::ElementHandle) {
        unsafe { ffi::whisker_bridge_remove_child(parent.0, child.0) }
    }

    fn set_event_listener(
        &mut self,
        handle: Self::ElementHandle,
        event_name: &str,
        callback: Box<dyn Fn() + 'static>,
    ) {
        let name_c = match CString::new(event_name) {
            Ok(c) => c,
            Err(_) => return,
        };
        // Double-box: outer Box owned by `self.listeners`, inner is the
        // Box<dyn Fn> passed in. Convert to raw pointer for the C ABI.
        let outer: Box<Box<dyn Fn() + 'static>> = Box::new(callback);
        let raw = Box::as_ref(&outer) as *const Box<dyn Fn() + 'static> as *mut c_void;
        self.listeners.push(outer);
        unsafe {
            ffi::whisker_bridge_set_event_listener(
                handle.0,
                name_c.as_ptr(),
                rust_event_trampoline,
                raw,
            )
        }
    }

    fn set_root(&mut self, page: Self::ElementHandle) {
        unsafe { ffi::whisker_bridge_set_root(self.engine_ptr(), page.0) }
    }

    fn flush(&mut self) {
        unsafe { ffi::whisker_bridge_flush(self.engine_ptr()) }
    }
}

extern "C" fn rust_event_trampoline(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: `user_data` was set up in `set_event_listener` to point at
    // a `Box<dyn Fn() + 'static>` whose owning Box lives in
    // `BridgeRenderer::listeners`. We borrow it without taking ownership.
    let cb = unsafe { &*(user_data as *const Box<dyn Fn() + 'static>) };
    cb();
}
