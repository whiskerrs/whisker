use super::resource::{empty_string, push_string};
use super::*;

#[derive(Debug)]

pub(super) struct MobileMeasureError(&'static str);
impl std::fmt::Display for MobileMeasureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for MobileMeasureError {}

pub(super) struct MobileMeasurementHost {
    pub(super) callback: MeasureCallback,
    pub(super) data: *mut c_void,
}

impl MeasurementProvider for MobileMeasurementHost {
    type Error = MobileMeasureError;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        let mut batch = MobileMeasureBatch::new(requests);
        if !(self.callback)(
            self.data,
            batch.requests.as_ptr(),
            batch.requests.len(),
            batch.responses.as_mut_ptr(),
        ) {
            return Err(MobileMeasureError("mobile Host rejected measurement batch"));
        }
        for raw in &batch.responses {
            let Some(request) = requests.iter().find(|item| item.key.get() == raw.key) else {
                return Err(MobileMeasureError(
                    "mobile Host returned an unknown measurement key",
                ));
            };
            if raw.environment_epoch != request.environment_epoch {
                return Err(MobileMeasureError(
                    "mobile Host returned a stale measurement epoch",
                ));
            }
            let make_metrics = || MeasurementMetrics {
                size: MeasuredSize::new(raw.width, raw.height),
                first_baseline: (raw.metrics_mask & 1 != 0).then_some(raw.first_baseline),
                last_baseline: (raw.metrics_mask & 2 != 0).then_some(raw.last_baseline),
                overflow: None,
                prepared_content: (raw.metrics_mask & 4 != 0)
                    .then(|| PreparedContentId::new(raw.prepared_content))
                    .flatten(),
            };
            responses.push(match raw.status {
                MEASURE_READY => MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: raw.environment_epoch,
                    metrics: make_metrics(),
                },
                MEASURE_PENDING => MeasurementResponse::Pending {
                    key: request.key,
                    environment_epoch: raw.environment_epoch,
                    request_id: MeasurementRequestId::new(raw.request_id)
                        .ok_or(MobileMeasureError("pending measurement omitted request ID"))?,
                    provisional: (raw.metrics_mask & 8 != 0).then(make_metrics),
                },
                MEASURE_UNSUPPORTED => MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: raw.environment_epoch,
                    reason: match raw.reason {
                        1 => UnsupportedMeasurementReason::Element,
                        2 => UnsupportedMeasurementReason::PayloadVersion,
                        3 => UnsupportedMeasurementReason::Environment,
                        4 => UnsupportedMeasurementReason::Feature,
                        _ => UnsupportedMeasurementReason::Kind,
                    },
                },
                _ => {
                    return Err(MobileMeasureError(
                        "mobile Host returned an invalid measurement status",
                    ));
                }
            });
        }
        Ok(())
    }
}

pub(super) struct MobileMeasureBatch {
    _strings: Vec<CString>,
    _bytes: Vec<Vec<u8>>,
    _font_families: Vec<Box<[WhiskerStringRef]>>,
    _font_features: Vec<Box<[MobileFontFeature]>>,
    _font_variations: Vec<Box<[MobileFontVariation]>>,
    pub(super) requests: Vec<MobileMeasureRequest>,
    responses: Vec<MobileMeasureResponse>,
}

impl MobileMeasureBatch {
    pub(super) fn new(source: &[MeasurementRequest]) -> Self {
        let mut strings = Vec::new();
        let mut bytes = Vec::new();
        let mut font_families = Vec::new();
        let mut font_features = Vec::new();
        let mut font_variations = Vec::new();
        let mut requests = Vec::with_capacity(source.len());
        let mut responses = Vec::with_capacity(source.len());
        for request in source {
            let mut raw = MobileMeasureRequest {
                key: request.key.get(),
                node: request.node.get(),
                element_type: request.element_type.get(),
                kind: 0,
                environment_epoch: request.environment_epoch,
                known_width: request.constraints.known_dimensions[0].unwrap_or_default(),
                known_height: request.constraints.known_dimensions[1].unwrap_or_default(),
                known_mask: u32::from(request.constraints.known_dimensions[0].is_some())
                    | (u32::from(request.constraints.known_dimensions[1].is_some()) << 1),
                available_width: available_value(request.constraints.available_space[0]),
                available_height: available_value(request.constraints.available_space[1]),
                available_width_kind: available_kind(request.constraints.available_space[0]),
                available_height_kind: available_kind(request.constraints.available_space[1]),
                font_style: 0,
                wrap: 0,
                word_break: 0,
                overflow: 0,
                text: empty_string(),
                locale: empty_string(),
                font_families: std::ptr::null(),
                font_family_count: 0,
                font_size: 0.0,
                font_weight: 400,
                payload_version: 0,
                line_height: 0.0,
                letter_spacing: 0.0,
                font_features: std::ptr::null(),
                font_feature_count: 0,
                font_variations: std::ptr::null(),
                font_variation_count: 0,
                font_optical_sizing: 1,
                _font_pad: [0; 7],
                indent_logical_pixels: 0.0,
                indent_percentage: 0.0,
                max_lines: 0,
                payload: WhiskerBytesRef {
                    ptr: std::ptr::null(),
                    len: 0,
                },
                intrinsic_width: 0.0,
                intrinsic_height: 0.0,
                intrinsic_mask: 0,
                direction: 0,
                alignment: 0,
                _flow_pad: [0; 6],
            };
            match &request.payload {
                MeasurementPayload::Text(value) => {
                    raw.kind = MEASURE_TEXT;
                    raw.text = push_string(&mut strings, &value.text);
                    raw.locale = value
                        .locale
                        .as_deref()
                        .map(|value| push_string(&mut strings, value))
                        .unwrap_or_else(empty_string);
                    font_families.push(
                        value
                            .style
                            .font_families
                            .iter()
                            .map(|family| match family {
                                MeasureFontFamily::System => push_string(&mut strings, "system"),
                                MeasureFontFamily::Named(value) => push_string(&mut strings, value),
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    let families = font_families.last().unwrap();
                    raw.font_families = nonempty_ptr(families);
                    raw.font_family_count = families.len();
                    raw.font_size = value.style.font_size;
                    raw.font_weight = value.style.font_weight;
                    raw.font_style = match value.style.font_style {
                        MeasureFontStyle::Normal => 0,
                        MeasureFontStyle::Italic => 1,
                        MeasureFontStyle::Oblique => 2,
                    };
                    raw.wrap = u8::from(matches!(value.wrap, MeasureTextWrap::Wrap));
                    raw.word_break = match value.word_break {
                        MeasureTextWordBreak::Normal => 0,
                        MeasureTextWordBreak::BreakAll => 1,
                        MeasureTextWordBreak::KeepAll => 2,
                    };
                    raw.overflow =
                        u8::from(matches!(value.overflow, MeasureTextOverflow::Ellipsis));
                    raw.line_height = match value.style.line_height {
                        MeasureLineHeight::Normal => 0.0,
                        MeasureLineHeight::LogicalPixels(value) => value,
                    };
                    raw.letter_spacing = value.style.letter_spacing;
                    font_features.push(mobile_font_features(&value.style.features));
                    let features = font_features.last().unwrap();
                    raw.font_features = nonempty_ptr(features);
                    raw.font_feature_count = features.len();
                    font_variations.push(mobile_font_variations(&value.style.variations));
                    let variations = font_variations.last().unwrap();
                    raw.font_variations = nonempty_ptr(variations);
                    raw.font_variation_count = variations.len();
                    raw.font_optical_sizing = u8::from(matches!(
                        value.style.optical_sizing,
                        whisker_engine::whisker_protocol::FontOpticalSizing::None
                    ));
                    raw.indent_logical_pixels = value.indent.logical_pixels;
                    raw.indent_percentage = value.indent.percentage;
                    raw.max_lines = value.max_lines.unwrap_or(0);
                    raw.direction = match value.direction {
                        MeasureTextDirection::Auto => 0,
                        MeasureTextDirection::LeftToRight => 1,
                        MeasureTextDirection::RightToLeft => 2,
                    };
                    raw.alignment = match value.alignment {
                        whisker_engine::whisker_protocol::MeasureTextAlignment::Start => 0,
                        whisker_engine::whisker_protocol::MeasureTextAlignment::End => 1,
                        whisker_engine::whisker_protocol::MeasureTextAlignment::Left => 2,
                        whisker_engine::whisker_protocol::MeasureTextAlignment::Right => 3,
                        whisker_engine::whisker_protocol::MeasureTextAlignment::Center => 4,
                    };
                }
                MeasurementPayload::ReplacedContent(value) => {
                    raw.kind = MEASURE_REPLACED_CONTENT;
                    if let Some(size) = value.intrinsic_size {
                        raw.intrinsic_width = size.width;
                        raw.intrinsic_height = size.height;
                        raw.intrinsic_mask = 3;
                    }
                }
                MeasurementPayload::NativeControl(value) => {
                    raw.kind = MEASURE_NATIVE_CONTROL;
                    raw.payload_version = value.version;
                    raw.payload = push_bytes(&mut bytes, &value.state);
                }
                MeasurementPayload::EmbeddedSurface(value) => {
                    raw.kind = MEASURE_EMBEDDED_SURFACE;
                    if let Some(size) = value.preferred_size {
                        raw.intrinsic_width = size.width;
                        raw.intrinsic_height = size.height;
                        raw.intrinsic_mask = 3;
                    }
                }
                MeasurementPayload::Custom(value) => {
                    raw.kind = MEASURE_CUSTOM;
                    raw.payload_version = value.version;
                    raw.payload = push_bytes(&mut bytes, &value.data);
                }
            }
            responses.push(MobileMeasureResponse {
                key: raw.key,
                environment_epoch: raw.environment_epoch,
                ..MobileMeasureResponse::default()
            });
            requests.push(raw);
        }
        Self {
            _strings: strings,
            _bytes: bytes,
            _font_families: font_families,
            _font_features: font_features,
            _font_variations: font_variations,
            requests,
            responses,
        }
    }
}

fn push_bytes(storage: &mut Vec<Vec<u8>>, value: &[u8]) -> WhiskerBytesRef {
    let value = value.to_vec();
    let result = WhiskerBytesRef {
        ptr: value.as_ptr(),
        len: value.len(),
    };
    storage.push(value);
    result
}

pub(super) fn mobile_font_features(
    values: &[whisker_engine::whisker_protocol::FontFeature],
) -> Box<[MobileFontFeature]> {
    values
        .iter()
        .map(|value| MobileFontFeature {
            tag: value.tag.get(),
            value: value.value,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

pub(super) fn mobile_font_variations(
    values: &[whisker_engine::whisker_protocol::FontVariation],
) -> Box<[MobileFontVariation]> {
    values
        .iter()
        .map(|value| MobileFontVariation {
            tag: value.tag.get(),
            value: value.value,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

pub(super) fn nonempty_ptr<T>(values: &[T]) -> *const T {
    if values.is_empty() {
        std::ptr::null()
    } else {
        values.as_ptr()
    }
}
fn available_kind(value: AvailableSpace) -> u8 {
    match value {
        AvailableSpace::Definite(_) => 0,
        AvailableSpace::MinContent => 1,
        AvailableSpace::MaxContent => 2,
    }
}
fn available_value(value: AvailableSpace) -> f32 {
    match value {
        AvailableSpace::Definite(value) => value,
        _ => 0.0,
    }
}
