//! Text lines for the frame timings `whisker run --trace` receives.

use whisker_dev_server::FrameSample;

/// Frames slower than one 60 Hz VSync are listed individually.
const SLOW_FRAME_US: u64 = 16_667;

/// One summary of the batch, then one line per slow frame.
pub fn lines(frames: &[FrameSample]) -> Vec<String> {
    if frames.is_empty() {
        return Vec::new();
    }
    let mut totals: Vec<u64> = frames.iter().map(|frame| frame.total_us).collect();
    totals.sort_unstable();
    let percentile = |p: usize| totals[(totals.len() - 1) * p / 100];
    let count = frames.len() as u64;
    let sum = |field: fn(&FrameSample) -> u64| frames.iter().map(field).sum::<u64>();
    let average = FrameSample {
        reactive_us: sum(|frame| frame.reactive_us) / count,
        layout_us: sum(|frame| frame.layout_us) / count,
        measure_us: sum(|frame| frame.measure_us) / count,
        present_us: sum(|frame| frame.present_us) / count,
        measure_batches: sum(|frame| frame.measure_batches),
        measure_requests: sum(|frame| frame.measure_requests),
        ..FrameSample::default()
    };
    let mut lines = vec![format!(
        "[trace] {count} frame{} · p50 {} p95 {} max {} · avg {}",
        if count == 1 { "" } else { "s" },
        ms(percentile(50)),
        ms(percentile(95)),
        ms(percentile(100)),
        phases(&average),
    )];
    lines.extend(
        frames
            .iter()
            .filter(|frame| frame.total_us > SLOW_FRAME_US)
            .map(|frame| {
                format!(
                    "[trace] slow frame {} · {}",
                    ms(frame.total_us),
                    phases(frame)
                )
            }),
    );
    lines
}

fn phases(frame: &FrameSample) -> String {
    format!(
        "reactive {} · layout {} (measure {}, {} req / {} calls) · present {}",
        ms(frame.reactive_us),
        ms(frame.layout_us),
        ms(frame.measure_us),
        frame.measure_requests,
        frame.measure_batches,
        ms(frame.present_us),
    )
}

fn ms(micros: u64) -> String {
    format!("{:.1}ms", micros as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(total_us: u64, measure_requests: u64) -> FrameSample {
        FrameSample {
            total_us,
            reactive_us: 1_000,
            layout_us: 2_000,
            measure_us: 500,
            present_us: 300,
            measure_batches: 1,
            measure_requests,
        }
    }

    #[test]
    fn summary_reports_percentiles_averages_and_measure_totals() {
        let lines = lines(&[frame(4_000, 3), frame(6_000, 5), frame(20_000, 40)]);
        assert_eq!(
            lines,
            [
                "[trace] 3 frames · p50 6.0ms p95 6.0ms max 20.0ms · avg reactive 1.0ms · \
                 layout 2.0ms (measure 0.5ms, 48 req / 3 calls) · present 0.3ms",
                "[trace] slow frame 20.0ms · reactive 1.0ms · layout 2.0ms (measure 0.5ms, \
                 40 req / 1 calls) · present 0.3ms",
            ]
        );
    }

    #[test]
    fn an_empty_batch_prints_nothing() {
        assert!(lines(&[]).is_empty());
    }
}
