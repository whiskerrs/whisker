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
    pub(crate) prepared:
        std::rc::Rc<std::cell::RefCell<HashMap<PreparedContentId, web_sys::Element>>>,
    element_measurements: HashMap<ElementTypeId, ElementMeasurement>,
    module_measurements: HashMap<ElementTypeId, WebMeasurementHandler>,
}

enum PendingMeasurement {
    Ready(MeasurementResponse),
    Text {
        probe: web_sys::Element,
        first_baseline: web_sys::Element,
        last_baseline: web_sys::Element,
        key: whisker_protocol::MeasurementKey,
        environment_epoch: u64,
        payload: whisker_protocol::TextMeasurePayload,
    },
}

impl Drop for PendingMeasurement {
    fn drop(&mut self) {
        if let Self::Text { probe, .. } = self {
            probe.remove();
        }
    }
}

impl DomMeasurementProvider {
    pub(crate) fn new(document: web_sys::Document) -> Self {
        Self {
            document,
            prepared: Default::default(),
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
            if let Some(width) = request.constraints.known_dimensions[0] {
                set_style(&probe, "width", &px(width))?;
            } else {
                match request.constraints.available_space[0] {
                    AvailableSpace::Definite(width) => {
                        set_style(&probe, "width", "fit-content")?;
                        set_style(&probe, "max-width", &px(width.max(0.0)))?;
                    }
                    AvailableSpace::MinContent => set_style(&probe, "width", "min-content")?,
                    AvailableSpace::MaxContent => set_style(&probe, "width", "max-content")?,
                }
            }
            if let Some(height) = request.constraints.known_dimensions[1] {
                set_style(&probe, "height", &px(height))?;
            }
            paint::text::apply_content(&probe, text, &[])?;
            let first_baseline = baseline_marker(&self.document)?;
            let last_baseline = baseline_marker(&self.document)?;
            probe
                .insert_before(&first_baseline, probe.first_child().as_ref())
                .map_err(|error| js_error("attach first baseline marker", error))?;
            probe
                .append_child(&last_baseline)
                .map_err(|error| js_error("attach last baseline marker", error))?;
            pending.push(PendingMeasurement::Text {
                probe,
                first_baseline,
                last_baseline,
                key: request.key,
                environment_epoch: request.environment_epoch,
                payload: text.clone(),
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
                    first_baseline,
                    last_baseline,
                    key,
                    environment_epoch,
                    payload,
                } => {
                    let paragraph_dom = if payload.runs.is_empty() && payload.attachments.is_empty()
                    {
                        None
                    } else {
                        Some(super::paragraph::ParagraphDom::read(probe, payload)?)
                    };
                    let visible_end = paragraph_dom
                        .as_ref()
                        .map(|dom| dom.prepare())
                        .transpose()?;
                    let rect = probe.get_bounding_client_rect();
                    let first_baseline =
                        (first_baseline.get_bounding_client_rect().top() - rect.top()) as f32;
                    let last_baseline =
                        (last_baseline.get_bounding_client_rect().top() - rect.top()) as f32;
                    let paragraph = paragraph_dom
                        .as_ref()
                        .zip(visible_end)
                        .map(|(dom, end)| dom.metrics(end, first_baseline))
                        .transpose()?;
                    let inline_placements = paragraph_dom
                        .as_ref()
                        .zip(visible_end)
                        .map(|(dom, end)| dom.placements(end))
                        .transpose()?
                        .unwrap_or_default();
                    if paragraph.is_some() {
                        let reference = probe
                            .query_selector("[data-whisker-paragraph]")
                            .map_err(|error| js_error("retain prepared paragraph", error))?
                            .ok_or_else(|| WebError("prepared paragraph is missing".into()))?;
                        self.prepared.borrow_mut().insert(
                            PreparedContentId::new(key.get()).expect("nonzero measurement key"),
                            reference,
                        );
                    }
                    responses.push(MeasurementResponse::Ready {
                        key: *key,
                        environment_epoch: *environment_epoch,
                        metrics: MeasurementMetrics {
                            paragraph,
                            inline_placements,
                            size: MeasuredSize::new(rect.width() as f32, rect.height() as f32),
                            first_baseline: Some(first_baseline),
                            last_baseline: Some(last_baseline),
                            overflow: None,
                            prepared_content: PreparedContentId::new(key.get()),
                        },
                    });
                }
            }
        }
        Ok(())
    }

    fn retain_prepared_content(
        &mut self,
        _: SurfaceId,
        _: u64,
        ids: &mut dyn Iterator<Item = PreparedContentId>,
    ) {
        let retained: std::collections::HashSet<_> = ids.collect();
        self.prepared
            .borrow_mut()
            .retain(|id, _| retained.contains(id));
    }
}

fn baseline_marker(document: &web_sys::Document) -> Result<web_sys::Element, WebError> {
    let marker = document
        .create_element("span")
        .map_err(|error| js_error("create text baseline marker", error))?;
    set_style(&marker, "display", "inline-block")?;
    set_style(&marker, "width", "0")?;
    set_style(&marker, "height", "0")?;
    set_style(&marker, "padding", "0")?;
    set_style(&marker, "margin", "0")?;
    set_style(&marker, "border", "0")?;
    set_style(&marker, "vertical-align", "baseline")?;
    Ok(marker)
}
