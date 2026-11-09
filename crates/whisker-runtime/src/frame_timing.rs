use std::time::{Duration, Instant};

use whisker_engine::MeasurementProvider;
use whisker_engine::whisker_protocol::{
    MeasurementRequest, MeasurementResponse, PreparedContentId, SurfaceId,
};

/// Where one [`RuntimeInstance::drive_frame`](crate::RuntimeInstance::drive_frame)
/// call spent its time, recorded only while
/// [`RuntimeInstance::set_frame_timing`](crate::RuntimeInstance::set_frame_timing)
/// is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameTiming {
    /// The whole callback.
    pub total: Duration,
    /// Host events, reactive flushes, ready tasks, and mounts.
    pub reactive: Duration,
    /// Style, Taffy layout, and Host measurement.
    pub layout: Duration,
    /// Host `measure_batch` calls, also counted in `layout`.
    pub measure: Duration,
    /// Number of Host `measure_batch` calls.
    pub measure_batches: u32,
    /// Requests sent across all `measure_batch` calls; cache hits never reach the Host.
    pub measure_requests: u32,
    /// Handing the frame to the Host and waiting for its acknowledgement.
    pub present: Duration,
}

/// `Instant::now()` only when timing is on, so a disabled frame reads no clock.
pub(crate) fn now_if(enabled: bool) -> Option<Instant> {
    enabled.then(Instant::now)
}

/// Delegates to the Host provider, adding each batch to `timing` when present.
pub(crate) struct TimedProvider<'a, Provider> {
    pub(crate) inner: &'a mut Provider,
    pub(crate) timing: Option<&'a mut FrameTiming>,
}

impl<Provider: MeasurementProvider> MeasurementProvider for TimedProvider<'_, Provider> {
    type Error = Provider::Error;

    fn measure_batch(
        &mut self,
        surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        let Some(timing) = self.timing.as_deref_mut() else {
            return self.inner.measure_batch(surface, requests, responses);
        };
        let start = Instant::now();
        let result = self.inner.measure_batch(surface, requests, responses);
        timing.measure += start.elapsed();
        timing.measure_batches += 1;
        timing.measure_requests += u32::try_from(requests.len()).unwrap_or(u32::MAX);
        result
    }

    fn measures_synchronously(&self) -> bool {
        self.inner.measures_synchronously()
    }

    fn retain_prepared_content(
        &mut self,
        surface: SurfaceId,
        revision: u64,
        retained: &mut dyn Iterator<Item = PreparedContentId>,
    ) {
        self.inner
            .retain_prepared_content(surface, revision, retained);
    }
}
