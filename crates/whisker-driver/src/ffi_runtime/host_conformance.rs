//! C ABI frame and measurement adapters for driving a native Host from
//! shared conformance fixtures without mounting an application.

use std::ffi::c_void;

use whisker_engine::{FrameSink, MeasurementProvider};

use super::frame::MobileFrameSink;
use super::measurement::MobileMeasurementHost;
use crate::abi::{MeasureCallback, MobileHostCapabilities, PresentFrameCallback};

/// Encodes each presented packet as a C frame for `present`.
///
/// # Panics
///
/// Panics when `capabilities` fails the same negotiation `whisker_view_create` performs.
pub fn frame_sink(
    present: PresentFrameCallback,
    data: *mut c_void,
    capabilities: &MobileHostCapabilities,
) -> impl FrameSink<Error: std::fmt::Debug> + use<> {
    MobileFrameSink {
        present,
        data,
        capabilities: super::decode_host_capabilities(capabilities)
            .unwrap_or_else(|error| panic!("Host capability negotiation failed: {error}")),
    }
}

/// Encodes each measurement batch as C requests for `measure`.
pub fn measurement_provider(
    measure: MeasureCallback,
    data: *mut c_void,
) -> impl MeasurementProvider<Error: std::fmt::Debug> + use<> {
    MobileMeasurementHost {
        callback: measure,
        data,
        prepared: std::collections::HashMap::new(),
    }
}
