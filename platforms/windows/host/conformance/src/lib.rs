//! Bindings to the WinUI Host's conformance library, linked when the xtask provides it.

#![cfg(whisker_host_lib)]

use std::ffi::c_void;

use whisker_driver_sys::{
    MobileApplyResponse, MobileFrame, MobileHostCapabilities, MobileMeasureRequest,
    MobileMeasureResponse, MobileResourceCommand,
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
    pub fn whisker_host_conformance_resource(
        host: *mut WhiskerHostConformance,
        command: *const MobileResourceCommand,
    ) -> bool;
    pub fn whisker_host_conformance_register_raster(
        host: *mut WhiskerHostConformance,
        id: u64,
        width: u32,
        height: u32,
        rgba: *const u8,
    ) -> bool;
    /// Returns 0 for an unknown generation, then loading, ready, failed, released from 1.
    pub fn whisker_host_conformance_resource_state(
        host: *mut WhiskerHostConformance,
        id: u64,
        generation: u64,
        width: *mut u32,
        height: *mut u32,
    ) -> u32;
    pub fn whisker_host_conformance_capture(
        host: *mut WhiskerHostConformance,
        width: u32,
        height: u32,
        rgba: *mut u8,
    ) -> bool;
}
