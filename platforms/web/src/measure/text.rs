use std::collections::HashMap;

use whisker_engine::MeasurementProvider;
use whisker_protocol::{
    AvailableSpace, ElementMeasurement, ElementRegistration, ElementTypeId, MeasuredSize,
    MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse,
    PreparedContentId, SurfaceId, UnsupportedMeasurementReason,
};

use crate::module_api::{WebElementFactory, WebMeasurementHandler};
use crate::{WebError, js_error, paint, px, set_style};

pub(crate) struct DomMeasurementProvider {
    document: web_sys::Document,
    element_measurements: HashMap<ElementTypeId, ElementMeasurement>,
    module_measurements: HashMap<ElementTypeId, WebMeasurementHandler>,
}

enum PendingMeasurement {
    Ready(MeasurementResponse),
    Text {
        probe: web_sys::Element,
        key: whisker_protocol::MeasurementKey,
        environment_epoch: u64,
        baseline: f32,
    },
}

impl DomMeasurementProvider {
    pub(crate) fn new(document: web_sys::Document) -> Self {
        Self {
            document,
            element_measurements: HashMap::new(),
            module_measurements: HashMap::new(),
        }
    }

    pub(crate) fn with_elements(
        document: web_sys::Document,
        registrations: &[ElementRegistration],
        factories: &[WebElementFactory],
    ) -> Result<Self, WebError> {
        let factories = factories
            .iter()
            .map(|factory| (factory.name.as_str(), factory))
            .collect::<HashMap<_, _>>();
        let mut provider = Self::new(document);
        for registration in registrations {
            let factory = factories.get(registration.name.as_str()).ok_or_else(|| {
                WebError(format!(
                    "missing DOM factory for element {}",
                    registration.name
                ))
            })?;
            let factory = factory.bind(registration)?;
            provider
                .element_measurements
                .insert(registration.element_type, registration.measurement);
            if let Some(measure) = factory.measurer {
                provider
                    .module_measurements
                    .insert(registration.element_type, measure);
            }
        }
        Ok(provider)
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
        let mut pending = Vec::with_capacity(requests.len());
        for request in requests {
            if let Some(measure) = self.module_measurements.get(&request.element_type) {
                pending.push(PendingMeasurement::Ready(match measure(&request.into()) {
                    Some(size) => MeasurementResponse::Ready {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        metrics: MeasurementMetrics::from_size(size),
                    },
                    None => MeasurementResponse::Unsupported {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        reason: UnsupportedMeasurementReason::Feature,
                    },
                }));
                continue;
            }
            let MeasurementPayload::Text(text) = &request.payload else {
                let reason = if self
                    .element_measurements
                    .contains_key(&request.element_type)
                {
                    UnsupportedMeasurementReason::Kind
                } else {
                    UnsupportedMeasurementReason::Element
                };
                pending.push(PendingMeasurement::Ready(
                    MeasurementResponse::Unsupported {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        reason,
                    },
                ));
                continue;
            };
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
            pending.push(PendingMeasurement::Text {
                probe,
                key: request.key,
                environment_epoch: request.environment_epoch,
                baseline: text.style.font_size * 0.8,
            });
        }
        for measurement in &pending {
            if let PendingMeasurement::Text { probe, .. } = measurement
                && let Err(error) = body.append_child(probe)
            {
                for attached in &pending {
                    if let PendingMeasurement::Text { probe, .. } = attached {
                        probe.remove();
                    }
                }
                return Err(js_error("attach text measurement probe", error));
            }
        }
        responses.reserve(pending.len());
        for measurement in &pending {
            match measurement {
                PendingMeasurement::Ready(response) => responses.push(response.clone()),
                PendingMeasurement::Text {
                    probe,
                    key,
                    environment_epoch,
                    baseline,
                } => {
                    let rect = probe.get_bounding_client_rect();
                    responses.push(MeasurementResponse::Ready {
                        key: *key,
                        environment_epoch: *environment_epoch,
                        metrics: MeasurementMetrics {
                            size: MeasuredSize::new(rect.width() as f32, rect.height() as f32),
                            first_baseline: Some(*baseline),
                            last_baseline: Some(*baseline),
                            overflow: None,
                            prepared_content: PreparedContentId::new(key.get()),
                        },
                    });
                }
            }
        }
        for measurement in pending {
            if let PendingMeasurement::Text { probe, .. } = measurement {
                probe.remove();
            }
        }
        Ok(())
    }
}
