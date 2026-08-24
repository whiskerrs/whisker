use whisker_engine::MeasurementProvider;
use whisker_protocol::{
    AvailableSpace, MeasuredSize, MeasurementMetrics, MeasurementPayload, MeasurementRequest,
    MeasurementResponse, PreparedContentId, SurfaceId, UnsupportedMeasurementReason,
};

use crate::{WebError, js_error, paint, px, set_style};

pub(crate) struct DomMeasurementProvider {
    document: web_sys::Document,
}

impl DomMeasurementProvider {
    pub(crate) fn new(document: web_sys::Document) -> Self {
        Self { document }
    }
}

impl MeasurementProvider for DomMeasurementProvider {
    type Error = WebError;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        let body = self
            .document
            .body()
            .ok_or_else(|| WebError("document body is unavailable".into()))?;
        for request in requests {
            let MeasurementPayload::Text(text) = &request.payload else {
                responses.push(MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    metrics: MeasurementMetrics {
                        size: MeasuredSize::new(0.0, 0.0),
                        first_baseline: None,
                        last_baseline: None,
                        overflow: None,
                        prepared_content: None,
                    },
                });
                continue;
            };
            if text.style.uses_extended_typography() {
                responses.push(MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    reason: UnsupportedMeasurementReason::Feature,
                });
                continue;
            }
            let probe = self
                .document
                .create_element("div")
                .map_err(|error| js_error("create text measurement probe", error))?;
            set_style(&probe, "position", "absolute")?;
            set_style(&probe, "visibility", "hidden")?;
            set_style(&probe, "pointer-events", "none")?;
            set_style(&probe, "left", "-100000px")?;
            set_style(&probe, "top", "0")?;
            set_style(&probe, "box-sizing", "border-box")?;
            paint::text::apply_metrics_style(&probe, text)?;
            match request.constraints.available_space[0] {
                AvailableSpace::Definite(width) => {
                    set_style(&probe, "width", &px(width.max(0.0)))?;
                }
                AvailableSpace::MinContent => set_style(&probe, "width", "min-content")?,
                AvailableSpace::MaxContent => set_style(&probe, "width", "max-content")?,
            }
            if let Some(width) = request.constraints.known_dimensions[0] {
                set_style(&probe, "width", &px(width))?;
            }
            if let Some(height) = request.constraints.known_dimensions[1] {
                set_style(&probe, "height", &px(height))?;
            }
            probe.set_text_content(Some(&text.text));
            body.append_child(&probe)
                .map_err(|error| js_error("attach text measurement probe", error))?;
            let rect = probe.get_bounding_client_rect();
            probe.remove();
            let baseline = text.style.font_size * 0.8;
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics {
                    size: MeasuredSize::new(rect.width() as f32, rect.height() as f32),
                    first_baseline: Some(baseline),
                    last_baseline: Some(baseline),
                    overflow: None,
                    prepared_content: PreparedContentId::new(request.key.get()),
                },
            });
        }
        Ok(())
    }
}
