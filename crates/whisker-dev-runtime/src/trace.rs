//! Frame timings recorded on the UI thread for `whisker run --trace`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use whisker_runtime::FrameTiming;

/// Bounds memory while the socket is stalled; one second at 120 Hz fits.
const MAX_BUFFERED: usize = 600;

static ENABLED: AtomicBool = AtomicBool::new(false);
static FRAMES: Mutex<Vec<FrameTiming>> = Mutex::new(Vec::new());

pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        take();
    }
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn record(timing: FrameTiming) {
    if let Ok(mut frames) = FRAMES.lock() {
        if frames.len() < MAX_BUFFERED {
            frames.push(timing);
        }
    }
}

pub(crate) fn take() -> Vec<FrameTiming> {
    FRAMES
        .lock()
        .map(|mut frames| std::mem::take(&mut *frames))
        .unwrap_or_default()
}

/// Each frame is `[total, reactive, layout, measure, present]` in microseconds
/// followed by `[measure_batches, measure_requests]`.
pub(crate) fn encode(frames: &[FrameTiming]) -> String {
    let micros = |duration: std::time::Duration| duration.as_micros() as u64;
    let frames: Vec<[u64; 7]> = frames
        .iter()
        .map(|frame| {
            [
                micros(frame.total),
                micros(frame.reactive),
                micros(frame.layout),
                micros(frame.measure),
                micros(frame.present),
                u64::from(frame.measure_batches),
                u64::from(frame.measure_requests),
            ]
        })
        .collect();
    serde_json::json!({ "kind": "trace", "frames": frames }).to_string()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn frames_encode_as_microsecond_tuples() {
        let frame = FrameTiming {
            total: Duration::from_micros(900),
            reactive: Duration::from_micros(100),
            layout: Duration::from_micros(500),
            measure: Duration::from_micros(300),
            measure_batches: 2,
            measure_requests: 7,
            present: Duration::from_micros(200),
        };
        assert_eq!(
            encode(&[frame]),
            r#"{"frames":[[900,100,500,300,200,2,7]],"kind":"trace"}"#
        );
    }
}
