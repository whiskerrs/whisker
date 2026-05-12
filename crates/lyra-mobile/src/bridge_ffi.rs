//! Raw `extern "C"` declarations matching `native/bridge/include/lyra_bridge.h`.
//!
//! These declarations are unsafe to call directly; use [`super::BridgeRenderer`]
//! for the safe wrapper.

use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct LyraEngine {
    _private: [u8; 0],
}

#[repr(C)]
pub struct LyraElement {
    _private: [u8; 0],
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum LyraElementTag {
    Page = 1,
    View = 2,
    Text = 3,
    RawText = 4,
    Image = 5,
    ScrollView = 6,
}

pub type LyraTasmCallback = extern "C" fn(user_data: *mut c_void);

extern "C" {
    pub fn lyra_bridge_engine_attach(lynx_view_ptr: *mut c_void) -> *mut LyraEngine;
    pub fn lyra_bridge_engine_release(engine: *mut LyraEngine);

    pub fn lyra_bridge_dispatch(
        engine: *mut LyraEngine,
        callback: LyraTasmCallback,
        user_data: *mut c_void,
    ) -> bool;

    pub fn lyra_bridge_create_element(
        engine: *mut LyraEngine,
        tag: LyraElementTag,
    ) -> *mut LyraElement;
    pub fn lyra_bridge_release_element(element: *mut LyraElement);

    pub fn lyra_bridge_set_attribute(
        element: *mut LyraElement,
        key: *const c_char,
        value: *const c_char,
    );
    pub fn lyra_bridge_set_inline_styles(element: *mut LyraElement, css: *const c_char);

    pub fn lyra_bridge_append_child(parent: *mut LyraElement, child: *mut LyraElement);
    pub fn lyra_bridge_remove_child(parent: *mut LyraElement, child: *mut LyraElement);

    pub fn lyra_bridge_set_event_listener(
        element: *mut LyraElement,
        event_name: *const c_char,
        callback: LyraEventCallback,
        user_data: *mut c_void,
    );

    pub fn lyra_bridge_set_root(engine: *mut LyraEngine, page: *mut LyraElement);
    pub fn lyra_bridge_flush(engine: *mut LyraEngine);
}

pub type LyraEventCallback = extern "C" fn(user_data: *mut c_void);
