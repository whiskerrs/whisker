use std::collections::HashMap;
use std::convert::Infallible;

use glyphon::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap,
};
use whisker_engine::MeasurementHost;
use whisker_protocol::{
    AvailableSpace, ElementMeasurement, LayoutRect, MeasureFontFamily, MeasureFontStyle,
    MeasureLineHeight, MeasureTextWrap, MeasuredSize, MeasurementMetrics, MeasurementPayload,
    MeasurementRequest, MeasurementResponse, PreparedContentId, SurfaceId, TextMeasurePayload,
    UnsupportedMeasurementReason,
};

use crate::element::DesktopElementRegistry;

pub(crate) struct PreparedText {
    pub(crate) buffer: Buffer,
}

pub(crate) struct NativeTextHost {
    elements: DesktopElementRegistry,
    pub(crate) font_system: FontSystem,
    pub(crate) swash_cache: SwashCache,
    pub(crate) prepared: HashMap<PreparedContentId, PreparedText>,
}

impl NativeTextHost {
    pub(crate) fn new(elements: DesktopElementRegistry) -> Self {
        Self {
            elements,
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            prepared: HashMap::new(),
        }
    }

    fn prepare_text(
        &mut self,
        payload: &TextMeasurePayload,
        request: &MeasurementRequest,
    ) -> (PreparedText, MeasurementMetrics) {
        let line_height = match payload.style.line_height {
            MeasureLineHeight::Normal => payload.style.font_size * 1.2,
            MeasureLineHeight::LogicalPixels(value) => value,
        };
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(payload.style.font_size, line_height),
        );
        buffer.set_wrap(
            &mut self.font_system,
            match payload.wrap {
                MeasureTextWrap::Wrap => Wrap::WordOrGlyph,
                MeasureTextWrap::NoWrap => Wrap::None,
            },
        );

        let available_width = match request.constraints.available_space[0] {
            AvailableSpace::Definite(value) => Some(value.max(0.0)),
            AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
        };
        let width = request.constraints.known_dimensions[0].or(available_width);
        let line_limit_height = payload.max_lines.map(|lines| lines as f32 * line_height);
        let available_height = match request.constraints.available_space[1] {
            AvailableSpace::Definite(value) => Some(value.max(0.0)),
            AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
        };
        let height = request.constraints.known_dimensions[1]
            .or(available_height)
            .map(|value| line_limit_height.map_or(value, |limit| value.min(limit)))
            .or(line_limit_height);
        buffer.set_size(&mut self.font_system, width, height);

        let family = payload
            .style
            .font_families
            .first()
            .map_or(Family::SansSerif, |family| match family {
                MeasureFontFamily::System => Family::SansSerif,
                MeasureFontFamily::Named(name) => Family::Name(name),
            });
        let style = match payload.style.font_style {
            MeasureFontStyle::Normal => Style::Normal,
            MeasureFontStyle::Italic => Style::Italic,
            MeasureFontStyle::Oblique => Style::Oblique,
        };
        let attrs = Attrs::new()
            .family(family)
            .style(style)
            .weight(Weight(payload.style.font_weight))
            .letter_spacing(payload.style.letter_spacing);
        buffer.set_text(
            &mut self.font_system,
            &payload.text,
            &attrs,
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut measured_width = 0.0_f32;
        let mut measured_height = 0.0_f32;
        let mut first_baseline = None;
        let mut last_baseline = None;
        for run in buffer.layout_runs() {
            measured_width = measured_width.max(run.line_w);
            measured_height = measured_height.max(run.line_top + run.line_height);
            first_baseline.get_or_insert(run.line_y);
            last_baseline = Some(run.line_y);
        }
        if payload.text.is_empty() {
            measured_height = line_height;
            first_baseline = Some(payload.style.font_size * 0.8);
            last_baseline = first_baseline;
        }
        if let Some(known) = request.constraints.known_dimensions[0] {
            measured_width = known;
        }
        if let Some(known) = request.constraints.known_dimensions[1] {
            measured_height = known;
        }
        let size = MeasuredSize::new(measured_width.max(0.0), measured_height.max(0.0));
        (
            PreparedText { buffer },
            MeasurementMetrics {
                size,
                first_baseline,
                last_baseline,
                overflow: Some(LayoutRect {
                    width: measured_width.max(0.0),
                    height: measured_height.max(0.0),
                    ..LayoutRect::default()
                }),
                prepared_content: None,
            },
        )
    }
}

impl MeasurementHost for NativeTextHost {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        for request in requests {
            let response = match (
                self.elements.measurement(request.element_type),
                &request.payload,
            ) {
                (Ok(ElementMeasurement::Text), MeasurementPayload::Text(payload)) => {
                    let (prepared, mut metrics) = self.prepare_text(payload, request);
                    let id = PreparedContentId::new(request.key.get())
                        .expect("measurement keys are always non-zero");
                    self.prepared.insert(id, prepared);
                    metrics.prepared_content = Some(id);
                    MeasurementResponse::Ready {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        metrics,
                    }
                }
                (Err(_), _) => MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    reason: UnsupportedMeasurementReason::Element,
                },
                (Ok(_), _) => MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    reason: UnsupportedMeasurementReason::Kind,
                },
            };
            responses.push(response);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::standard_element_registrations;
    use whisker_protocol::{
        ElementContentKind, ElementTypeId, MeasureConstraints, MeasurementKey, NodeId,
        ReplacedContentMeasurePayload,
    };

    fn registry() -> DesktopElementRegistry {
        DesktopElementRegistry::bind(
            &standard_element_registrations(),
            &crate::element::standard_desktop_element_factories(),
        )
        .unwrap()
    }

    fn text_element_type() -> ElementTypeId {
        standard_element_registrations()
            .into_iter()
            .find(|registration| registration.content == ElementContentKind::Text)
            .unwrap()
            .element_type
    }

    fn request(key: u64, payload: MeasurementPayload) -> MeasurementRequest {
        MeasurementRequest {
            key: MeasurementKey::new(key).unwrap(),
            node: NodeId::new(1).unwrap(),
            element_type: text_element_type(),
            environment_epoch: 3,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [AvailableSpace::Definite(200.0), AvailableSpace::MaxContent],
            },
            payload,
        }
    }

    #[test]
    fn native_text_measurement_returns_the_buffer_used_for_paint() {
        let payload = TextMeasurePayload {
            text: "Whisker native text".into(),
            style: whisker_protocol::TextMeasureStyle {
                font_families: vec![MeasureFontFamily::System],
                font_size: 18.0,
                font_weight: 500,
                font_style: MeasureFontStyle::Normal,
                line_height: MeasureLineHeight::LogicalPixels(24.0),
                letter_spacing: 0.5,
            },
            locale: None,
            direction: whisker_protocol::MeasureTextDirection::Auto,
            wrap: MeasureTextWrap::Wrap,
            max_lines: Some(2),
            overflow: whisker_protocol::MeasureTextOverflow::Clip,
        };
        let mut host = NativeTextHost::new(registry());
        let mut responses = Vec::new();
        host.measure_batch(
            SurfaceId::new(1).unwrap(),
            &[request(7, MeasurementPayload::Text(payload))],
            &mut responses,
        )
        .unwrap();
        let MeasurementResponse::Ready {
            environment_epoch,
            metrics,
            ..
        } = &responses[0]
        else {
            panic!("native text is immediately ready");
        };
        assert_eq!(*environment_epoch, 3);
        assert!(metrics.size.width > 0.0);
        assert!(metrics.size.height > 0.0);
        assert!(metrics.first_baseline.is_some());
        assert!(metrics.last_baseline.is_some());
        let prepared = metrics.prepared_content.unwrap();
        assert!(host.prepared.contains_key(&prepared));
    }

    #[test]
    fn empty_text_and_non_text_measurements_are_well_formed() {
        let empty = TextMeasurePayload {
            text: String::new(),
            style: whisker_protocol::TextMeasureStyle {
                font_families: vec![MeasureFontFamily::Named("Helvetica".into())],
                font_size: 12.0,
                font_weight: 400,
                font_style: MeasureFontStyle::Italic,
                line_height: MeasureLineHeight::Normal,
                letter_spacing: 0.0,
            },
            locale: Some("en-US".into()),
            direction: whisker_protocol::MeasureTextDirection::LeftToRight,
            wrap: MeasureTextWrap::NoWrap,
            max_lines: None,
            overflow: whisker_protocol::MeasureTextOverflow::Ellipsis,
        };
        let mut host = NativeTextHost::new(registry());
        let mut responses = Vec::new();
        let mut unknown_element = request(9, MeasurementPayload::Text(empty.clone()));
        unknown_element.element_type = ElementTypeId::new(900).unwrap();
        host.measure_batch(
            SurfaceId::new(1).unwrap(),
            &[
                request(7, MeasurementPayload::Text(empty)),
                request(
                    8,
                    MeasurementPayload::ReplacedContent(ReplacedContentMeasurePayload::default()),
                ),
                unknown_element,
            ],
            &mut responses,
        )
        .unwrap();
        assert_eq!(responses.len(), 3);
        assert!(matches!(
            &responses[0],
            MeasurementResponse::Ready { metrics, .. }
                if metrics.size.height > 0.0 && metrics.prepared_content.is_some()
        ));
        assert!(matches!(
            &responses[1],
            MeasurementResponse::Unsupported {
                reason: UnsupportedMeasurementReason::Kind,
                ..
            }
        ));
        assert!(matches!(
            &responses[2],
            MeasurementResponse::Unsupported {
                reason: UnsupportedMeasurementReason::Element,
                ..
            }
        ));
    }
}
