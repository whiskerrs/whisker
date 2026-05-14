//! Raw `extern "C"` declarations matching `bridge/include/whisker_bridge.h`.
//!
//! Everything here is `unsafe` to call. Safe wrappers (and the host shim
//! `whisker_app_main` / `whisker_tick` exports) live in `whisker-driver`. Users
//! never depend on this crate directly.

use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct WhiskerEngine {
    _private: [u8; 0],
}

#[repr(C)]
pub struct WhiskerElement {
    _private: [u8; 0],
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum WhiskerElementTag {
    Page = 1,
    View = 2,
    Text = 3,
    RawText = 4,
    Image = 5,
    ScrollView = 6,
}

pub type WhiskerTasmCallback = extern "C" fn(user_data: *mut c_void);
pub type WhiskerEventCallback = extern "C" fn(user_data: *mut c_void);

extern "C" {
    pub fn whisker_bridge_engine_attach(lynx_view_ptr: *mut c_void) -> *mut WhiskerEngine;
    pub fn whisker_bridge_engine_release(engine: *mut WhiskerEngine);

    pub fn whisker_bridge_dispatch(
        engine: *mut WhiskerEngine,
        callback: WhiskerTasmCallback,
        user_data: *mut c_void,
    ) -> bool;

    pub fn whisker_bridge_create_element(
        engine: *mut WhiskerEngine,
        tag: WhiskerElementTag,
    ) -> *mut WhiskerElement;
    pub fn whisker_bridge_release_element(element: *mut WhiskerElement);

    pub fn whisker_bridge_set_attribute(
        element: *mut WhiskerElement,
        key: *const c_char,
        value: *const c_char,
    );
    pub fn whisker_bridge_set_inline_styles(element: *mut WhiskerElement, css: *const c_char);

    pub fn whisker_bridge_append_child(parent: *mut WhiskerElement, child: *mut WhiskerElement);
    pub fn whisker_bridge_remove_child(parent: *mut WhiskerElement, child: *mut WhiskerElement);

    pub fn whisker_bridge_set_event_listener(
        element: *mut WhiskerElement,
        event_name: *const c_char,
        callback: WhiskerEventCallback,
        user_data: *mut c_void,
    );

    pub fn whisker_bridge_set_root(engine: *mut WhiskerEngine, page: *mut WhiskerElement);
    pub fn whisker_bridge_flush(engine: *mut WhiskerEngine);
}
