//! Bindings to the WinUI Host's conformance library, linked when the xtask provides it.

#![cfg(whisker_host_lib)]

use std::ffi::c_void;

use whisker_driver_sys::{
    MobileApplyResponse, MobileFrame, MobileHostCapabilities, MobileMeasureRequest,
    MobileMeasureResponse,
};

#[repr(C)]
pub struct WhiskerHostConformance {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    pub fn whisker_host_conformance_create() -> *mut WhiskerHostConformance;
    pub fn whisker_host_conformance_destroy(host: *mut WhiskerHostConformance);
    pub fn whisker_host_conformance_capabilities() -> MobileHostCapabilities;
    pub fn whisker_host_conformance_present(
        host: *mut c_void,
        frame: *const MobileFrame,
        response: *mut MobileApplyResponse,
    ) -> bool;
    pub fn whisker_host_conformance_measure(
        host: *mut c_void,
        requests: *const MobileMeasureRequest,
        count: usize,
        responses: *mut MobileMeasureResponse,
    ) -> bool;
    pub fn whisker_host_conformance_capture(
        host: *mut WhiskerHostConformance,
        width: u32,
        height: u32,
        rgba: *mut u8,
    ) -> bool;
}
