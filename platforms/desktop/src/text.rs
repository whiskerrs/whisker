use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::Infallible;

use glyphon::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap,
    cosmic_text::{Align, FeatureTag, FontFeatures},
};
use whisker_engine::MeasurementProvider;
use whisker_protocol::{
    AvailableSpace, ElementMeasurement, LayoutRect, MeasureFontFamily, MeasureFontStyle,
    MeasureLineHeight, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, MeasuredSize,
    MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse,
    PreparedContentId, SurfaceId, TextMeasurePayload, UnsupportedMeasurementReason,
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

fn cosmic_alignment(value: whisker_protocol::MeasureTextAlignment) -> Option<Align> {
    match value {
        whisker_protocol::MeasureTextAlignment::Start => None,
        whisker_protocol::MeasureTextAlignment::End => Some(Align::End),
        whisker_protocol::MeasureTextAlignment::Left => Some(Align::Left),
        whisker_protocol::MeasureTextAlignment::Right => Some(Align::Right),
        whisker_protocol::MeasureTextAlignment::Center => Some(Align::Center),
    }
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

    fn shape_text(
        &mut self,
        payload: &TextMeasurePayload,
        text: &str,
        width: Option<f32>,
        height: Option<f32>,
        line_height: f32,
    ) -> Buffer {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(payload.style.font_size, line_height),
        );
        buffer.set_wrap(
            &mut self.font_system,
            match (payload.wrap, payload.word_break) {
                (MeasureTextWrap::NoWrap, _) => Wrap::None,
                (MeasureTextWrap::Wrap, MeasureTextWordBreak::Normal) => Wrap::Word,
                (MeasureTextWrap::Wrap, MeasureTextWordBreak::BreakAll) => Wrap::Glyph,
                (MeasureTextWrap::Wrap, MeasureTextWordBreak::KeepAll) => Wrap::Word,
            },
        );
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
        let mut font_features = FontFeatures::new();
        for feature in &payload.style.features {
            font_features.set(FeatureTag::new(&feature.tag.get()), feature.value);
        }
        let variation_weight = payload
            .style
            .variations
            .iter()
            .rev()
            .find(|variation| variation.tag.get() == *b"wght")
            .map_or(payload.style.font_weight, |variation| {
                variation.value.clamp(1.0, 1000.0) as u16
            });
        let attrs = Attrs::new()
            .family(family)
            .style(style)
            .weight(Weight(variation_weight))
            .letter_spacing(payload.style.letter_spacing)
            .font_features(font_features);
        let indent = payload.indent.resolve(width.unwrap_or(0.0));
        if indent == 0.0 {
            buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced);
        } else {
            // cosmic-text has no paragraph-indent switch. An internal
            // zero-width shaping span gives the first visual line the exact
            // additional advance while preserving the author-visible text.
            let indent_attrs = attrs
                .clone()
                .letter_spacing(indent / payload.style.font_size.max(f32::EPSILON));
            buffer.set_rich_text(
                &mut self.font_system,
                [("\u{200B}", indent_attrs), (text, attrs.clone())],
                &attrs,
                Shaping::Advanced,
                None,
            );
        }
        let alignment = cosmic_alignment(payload.alignment);
        for line in &mut buffer.lines {
            line.set_align(alignment);
        }
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
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
        let display_text = if payload.word_break == MeasureTextWordBreak::KeepAll {
            Cow::Owned(protect_cjk_breaks(&payload.text))
        } else {
            Cow::Borrowed(payload.text.as_str())
        };
        let mut buffer = self.shape_text(payload, &display_text, width, height, line_height);
        if payload.overflow == MeasureTextOverflow::Ellipsis
            && let Some(width) = width
            && !buffer_fits(&buffer, width, payload.max_lines)
        {
            let characters = payload.text.chars().collect::<Vec<_>>();
            let mut lower = 0;
            let mut upper = characters.len();
            let mut best = String::from("…");
            while lower <= upper {
                let middle = lower + (upper - lower) / 2;
                let mut candidate = characters[..middle].iter().collect::<String>();
                candidate.push('…');
                let candidate = if payload.word_break == MeasureTextWordBreak::KeepAll {
                    protect_cjk_breaks(&candidate)
                } else {
                    candidate
                };
                let candidate_buffer =
                    self.shape_text(payload, &candidate, Some(width), height, line_height);
                if buffer_fits(&candidate_buffer, width, payload.max_lines) {
                    best = candidate;
                    lower = middle + 1;
                } else if middle == 0 {
                    break;
                } else {
                    upper = middle - 1;
                }
            }
            buffer = self.shape_text(payload, &best, Some(width), height, line_height);
        }

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

fn buffer_fits(buffer: &Buffer, width: f32, max_lines: Option<u32>) -> bool {
    let lines = buffer
        .lines
        .iter()
        .flat_map(|line| line.layout_opt().into_iter().flatten())
        .collect::<Vec<_>>();
    max_lines.is_none_or(|limit| lines.len() <= limit as usize)
        && lines.iter().all(|line| line.w <= width + 0.01)
}

fn protect_cjk_breaks(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_cjk = false;
    for character in value.chars() {
        let current_is_cjk = matches!(
            character as u32,
            0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF
        );
        if previous_was_cjk && current_is_cjk {
            result.push('\u{2060}');
        }
        result.push(character);
        previous_was_cjk = current_is_cjk;
    }
    result
}

impl MeasurementProvider for NativeTextHost {
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
        ElementTypeId, MeasureConstraints, MeasurementKey, NodeId, ReplacedContentMeasurePayload,
    };

    fn registry() -> DesktopElementRegistry {
        DesktopElementRegistry::bind(
            &standard_element_registrations(),
            &crate::element::built_in_element_factories(),
        )
        .unwrap()
    }

    #[test]
    fn every_protocol_alignment_maps_to_cosmic_text() {
        use whisker_protocol::MeasureTextAlignment as Protocol;
        assert_eq!(cosmic_alignment(Protocol::Start), None);
        assert_eq!(cosmic_alignment(Protocol::End), Some(Align::End));
        assert_eq!(cosmic_alignment(Protocol::Left), Some(Align::Left));
        assert_eq!(cosmic_alignment(Protocol::Right), Some(Align::Right));
        assert_eq!(cosmic_alignment(Protocol::Center), Some(Align::Center));
    }

    fn text_element_type() -> ElementTypeId {
        standard_element_registrations()
            .into_iter()
            .find(|registration| registration.name == whisker::TEXT_ELEMENT_NAME)
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
                ..whisker_protocol::TextMeasureStyle::default()
            },
            locale: None,
            direction: whisker_protocol::MeasureTextDirection::Auto,
            alignment: whisker_protocol::MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::Wrap,
            word_break: Default::default(),
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
    fn native_text_measurement_includes_first_line_indent() {
        let payload = TextMeasurePayload {
            text: "Whisker".into(),
            style: whisker_protocol::TextMeasureStyle {
                font_size: 16.0,
                ..whisker_protocol::TextMeasureStyle::default()
            },
            locale: None,
            direction: whisker_protocol::MeasureTextDirection::Auto,
            alignment: whisker_protocol::MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::NoWrap,
            word_break: Default::default(),
            max_lines: None,
            overflow: whisker_protocol::MeasureTextOverflow::Clip,
        };
        let mut indented = payload.clone();
        indented.indent.logical_pixels = 24.0;

        let mut host = NativeTextHost::new(registry());
        let (_, plain_metrics) = host.prepare_text(
            &payload,
            &request(7, MeasurementPayload::Text(payload.clone())),
        );
        let (_, indented_metrics) = host.prepare_text(
            &indented,
            &request(8, MeasurementPayload::Text(indented.clone())),
        );

        assert!(
            (indented_metrics.size.width - plain_metrics.size.width - 24.0).abs() < 0.1,
            "plain={}, indented={}",
            plain_metrics.size.width,
            indented_metrics.size.width
        );
    }

    #[test]
    fn native_text_shapes_breaking_and_ellipsis_policies() {
        assert_eq!(protect_cjk_breaks("日本 A"), "日\u{2060}本 A");

        let payload = TextMeasurePayload {
            text: "a deliberately overflowing line that cannot fit".into(),
            style: whisker_protocol::TextMeasureStyle {
                font_size: 16.0,
                ..whisker_protocol::TextMeasureStyle::default()
            },
            locale: None,
            direction: whisker_protocol::MeasureTextDirection::Auto,
            alignment: whisker_protocol::MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::NoWrap,
            word_break: MeasureTextWordBreak::Normal,
            max_lines: Some(1),
            overflow: MeasureTextOverflow::Ellipsis,
        };
        let mut host = NativeTextHost::new(registry());
        let (prepared, metrics) = host.prepare_text(
            &payload,
            &request(9, MeasurementPayload::Text(payload.clone())),
        );

        assert!(
            prepared.buffer.lines[0].text().ends_with('…'),
            "text={:?}, layouts={:?}",
            prepared.buffer.lines[0].text(),
            prepared.buffer.lines[0].layout_opt()
        );
        assert!(metrics.size.width <= 200.01);
        assert!(buffer_fits(&prepared.buffer, 200.0, Some(1)));

        let mut break_all = payload;
        break_all.text = "unbreakable".repeat(8);
        break_all.wrap = MeasureTextWrap::Wrap;
        break_all.word_break = MeasureTextWordBreak::BreakAll;
        break_all.overflow = MeasureTextOverflow::Clip;
        break_all.max_lines = None;
        let (prepared, _) = host.prepare_text(
            &break_all,
            &request(10, MeasurementPayload::Text(break_all.clone())),
        );
        let line_count = prepared.buffer.lines[0]
            .layout_opt()
            .expect("text was shaped")
            .len();
        assert!(line_count > 1);
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
                ..whisker_protocol::TextMeasureStyle::default()
            },
            locale: Some("en-US".into()),
            direction: whisker_protocol::MeasureTextDirection::LeftToRight,
            alignment: whisker_protocol::MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::NoWrap,
            word_break: Default::default(),
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
