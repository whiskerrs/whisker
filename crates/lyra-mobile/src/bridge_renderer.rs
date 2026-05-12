//! Renderer impl that calls into the C++ bridge.
//!
//! Must only be used from inside a `lyra_bridge_dispatch` callback (i.e.
//! on the Lynx TASM thread).

use crate::bridge_ffi::{self as ffi, LyraElement, LyraElementTag, LyraEngine};
use lyra_runtime::element::ElementTag;
use lyra_runtime::renderer::Renderer;
use std::ffi::CString;
use std::ptr::NonNull;

pub struct BridgeRenderer {
    engine: NonNull<LyraEngine>,
}

impl BridgeRenderer {
    /// # Safety
    /// `engine` must point to a valid LyraEngine returned from
    /// `lyra_bridge_engine_attach`. Caller must ensure this renderer is
    /// only used inside a `lyra_bridge_dispatch` callback for the same
    /// engine.
    pub unsafe fn from_raw(engine: *mut LyraEngine) -> Option<Self> {
        NonNull::new(engine).map(|engine| Self { engine })
    }

    fn engine_ptr(&self) -> *mut LyraEngine {
        self.engine.as_ptr()
    }
}

fn map_tag(tag: ElementTag) -> LyraElementTag {
    match tag {
        ElementTag::Page => LyraElementTag::Page,
        ElementTag::View => LyraElementTag::View,
        ElementTag::Text => LyraElementTag::Text,
        ElementTag::RawText => LyraElementTag::RawText,
        ElementTag::Image => LyraElementTag::Image,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElementHandle(*mut LyraElement);

// SAFETY: handles are only used from a single thread (TASM).
unsafe impl Send for ElementHandle {}

impl Renderer for BridgeRenderer {
    type ElementHandle = ElementHandle;

    fn create_element(&mut self, tag: ElementTag) -> Self::ElementHandle {
        let raw = unsafe { ffi::lyra_bridge_create_element(self.engine_ptr(), map_tag(tag)) };
        ElementHandle(raw)
    }

    fn release_element(&mut self, handle: Self::ElementHandle) {
        unsafe { ffi::lyra_bridge_release_element(handle.0) }
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
        unsafe { ffi::lyra_bridge_set_attribute(handle.0, key_c.as_ptr(), value_c.as_ptr()) }
    }

    fn set_inline_styles(&mut self, handle: Self::ElementHandle, css: &str) {
        let css_c = match CString::new(css) {
            Ok(c) => c,
            Err(_) => return,
        };
        unsafe { ffi::lyra_bridge_set_inline_styles(handle.0, css_c.as_ptr()) }
    }

    fn append_child(&mut self, parent: Self::ElementHandle, child: Self::ElementHandle) {
        unsafe { ffi::lyra_bridge_append_child(parent.0, child.0) }
    }

    fn remove_child(&mut self, parent: Self::ElementHandle, child: Self::ElementHandle) {
        unsafe { ffi::lyra_bridge_remove_child(parent.0, child.0) }
    }

    fn set_root(&mut self, page: Self::ElementHandle) {
        unsafe { ffi::lyra_bridge_set_root(self.engine_ptr(), page.0) }
    }

    fn flush(&mut self) {
        unsafe { ffi::lyra_bridge_flush(self.engine_ptr()) }
    }
}
