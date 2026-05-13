//! Raw `extern "C"` declarations matching `bridge/include/tuft_bridge.h`.
//!
//! Everything here is `unsafe` to call. Safe wrappers (and the host shim
//! `tuft_app_main` / `tuft_tick` exports) live in `tuft-driver`. Users
//! never depend on this crate directly.

use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct TuftEngine {
    _private: [u8; 0],
}

#[repr(C)]
pub struct TuftElement {
    _private: [u8; 0],
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum TuftElementTag {
    Page = 1,
    View = 2,
    Text = 3,
    RawText = 4,
    Image = 5,
    ScrollView = 6,
}

pub type TuftTasmCallback = extern "C" fn(user_data: *mut c_void);
pub type TuftEventCallback = extern "C" fn(user_data: *mut c_void);

extern "C" {
    pub fn tuft_bridge_engine_attach(lynx_view_ptr: *mut c_void) -> *mut TuftEngine;
    pub fn tuft_bridge_engine_release(engine: *mut TuftEngine);

    pub fn tuft_bridge_dispatch(
        engine: *mut TuftEngine,
        callback: TuftTasmCallback,
        user_data: *mut c_void,
    ) -> bool;

    pub fn tuft_bridge_create_element(
        engine: *mut TuftEngine,
        tag: TuftElementTag,
    ) -> *mut TuftElement;
    pub fn tuft_bridge_release_element(element: *mut TuftElement);

    pub fn tuft_bridge_set_attribute(
        element: *mut TuftElement,
        key: *const c_char,
        value: *const c_char,
    );
    pub fn tuft_bridge_set_inline_styles(element: *mut TuftElement, css: *const c_char);

    pub fn tuft_bridge_append_child(parent: *mut TuftElement, child: *mut TuftElement);
    pub fn tuft_bridge_remove_child(parent: *mut TuftElement, child: *mut TuftElement);

    pub fn tuft_bridge_set_event_listener(
        element: *mut TuftElement,
        event_name: *const c_char,
        callback: TuftEventCallback,
        user_data: *mut c_void,
    );

    pub fn tuft_bridge_set_root(engine: *mut TuftEngine, page: *mut TuftElement);
    pub fn tuft_bridge_flush(engine: *mut TuftEngine);
}
