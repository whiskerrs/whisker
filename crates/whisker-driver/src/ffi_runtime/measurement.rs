use super::resource::{empty_string, push_string};
use super::*;

#[derive(Debug)]

pub(super) struct MobileMeasureError(pub(super) &'static str);
impl std::fmt::Display for MobileMeasureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for MobileMeasureError {}

pub(super) struct MobileMeasurementHost {
    pub(super) callback: MeasureCallback,
    pub(super) data: *mut c_void,
    pub(super) prepared: std::collections::HashMap<PreparedContentId, NativePreparedLayout>,
}

pub(super) struct NativePreparedLayout {
    data: *mut c_void,
    release: extern "C" fn(*mut c_void),
}

impl Drop for NativePreparedLayout {
    fn drop(&mut self) {
        (self.release)(self.data);
    }
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
            nonempty_ptr(&batch.requests),
            batch.requests.len(),
            nonempty_mut_ptr(&mut batch.responses),
        ) {
            return Err(MobileMeasureError("mobile Host rejected measurement batch"));
        }
        for (request, raw) in requests.iter().zip(&mut batch.responses) {
            if raw.key != request.key.get() {
                return Err(MobileMeasureError(
                    "mobile Host reordered measurement responses",
                ));
            }
            if raw.environment_epoch != request.environment_epoch {
                return Err(MobileMeasureError(
                    "mobile Host returned a stale measurement epoch",
                ));
            }
            if !raw.paragraph.is_null() && raw.release_paragraph.is_none() {
                return Err(MobileMeasureError(
                    "owned paragraph geometry omitted its release callback",
                ));
            }
            if raw.prepared_layout.is_null() != raw.release_prepared_layout.is_none() {
                return Err(MobileMeasureError("invalid prepared layout ownership"));
            }
            let paragraph = super::paragraph_response::decode(raw.paragraph)?;
            if !raw.prepared_layout.is_null() {
                let id = PreparedContentId::new(raw.prepared_content)
                    .filter(|_| raw.metrics_mask & 4 != 0)
                    .ok_or(MobileMeasureError("prepared layout omitted its content ID"))?;
                let lease = NativePreparedLayout {
                    data: std::mem::replace(&mut raw.prepared_layout, std::ptr::null_mut()),
                    release: raw
                        .release_prepared_layout
                        .take()
                        .expect("validated layout ownership"),
                };
                self.prepared.insert(id, lease);
            }

            let make_metrics = || MeasurementMetrics {
                paragraph: paragraph.metrics.clone(),
                inline_placements: paragraph.placements.clone(),
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

    fn retain_prepared_content(
        &mut self,
        _: SurfaceId,
        _: u64,
        ids: &mut dyn Iterator<Item = PreparedContentId>,
    ) {
        if self.prepared.is_empty() {
            return;
        }
        let retained: std::collections::HashSet<_> = ids.collect();
        self.prepared.retain(|id, _| retained.contains(id));
    }
}

pub(super) struct MobileMeasureBatch {
    _paragraphs: super::paragraph::MobileParagraphs,
    _strings: Vec<Box<[u8]>>,
    _payload_arena: RawValueArena,
    // FFI pointers must survive subsequent pushes.
    #[allow(clippy::vec_box)]
    _payloads: Vec<Box<WhiskerValueRaw>>,
    _font_families: Vec<Box<[WhiskerStringRef]>>,
    _font_features: Vec<Box<[MobileFontFeature]>>,
    _font_variations: Vec<Box<[MobileFontVariation]>>,
    pub(super) requests: Vec<MobileMeasureRequest>,
    responses: Vec<MobileMeasureResponse>,
}

impl Drop for MobileMeasureBatch {
    fn drop(&mut self) {
        for response in &mut self.responses {
            if let Some(release) = response.release_prepared_layout.take() {
                release(std::mem::replace(
                    &mut response.prepared_layout,
                    std::ptr::null_mut(),
                ));
            }
            if let Some(release) = response.release_paragraph.take() {
                release(std::mem::replace(
                    &mut response.paragraph,
                    std::ptr::null_mut(),
                ));
            }
        }
    }
}

impl MobileMeasureBatch {
    pub(super) fn new(source: &[MeasurementRequest]) -> Self {
        let mut strings = Vec::new();
        let mut paragraphs = super::paragraph::MobileParagraphs::default();
        let mut payload_arena = RawValueArena::default();
        let mut payloads = Vec::new();
        let mut font_families = Vec::new();
        let mut font_features = Vec::new();
        let mut font_variations = Vec::new();
        let mut requests = Vec::with_capacity(source.len());
        let mut responses = Vec::with_capacity(source.len());
        for request in source {
            let mut raw = MobileMeasureRequest {
                paragraph: std::ptr::null(),
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
                payload: std::ptr::null(),
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
                    raw.paragraph = paragraphs.push(value, &[], None);
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
                    raw.wrap = u8::from(value.wrap != MeasureTextWrap::NoWrap);
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
                    raw.payload = push_value(
                        &mut payload_arena,
                        &mut payloads,
                        &WhiskerValue::Bytes(value.state.clone()),
                    );
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
                    raw.payload = push_value(&mut payload_arena, &mut payloads, &value.data);
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
            _paragraphs: paragraphs,
            _payload_arena: payload_arena,
            _payloads: payloads,
            _font_families: font_families,
            _font_features: font_features,
            _font_variations: font_variations,
            requests,
            responses,
        }
    }
}

#[allow(clippy::vec_box)]
fn push_value(
    arena: &mut RawValueArena,
    storage: &mut Vec<Box<WhiskerValueRaw>>,
    value: &WhiskerValue,
) -> *const WhiskerValueRaw {
    storage.push(Box::new(arena.encode(value)));
    storage
        .last()
        .expect("retained measurement payload")
        .as_ref()
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

fn nonempty_mut_ptr<T>(values: &mut [T]) -> *mut T {
    if values.is_empty() {
        std::ptr::null_mut()
    } else {
        values.as_mut_ptr()
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

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_engine::whisker_protocol::{
        ElementTypeId, MeasureConstraints, MeasurementKey, ReplacedContentMeasurePayload,
    };

    extern "C" fn reorder_responses(
        _data: *mut c_void,
        _requests: *const MobileMeasureRequest,
        count: usize,
        responses: *mut MobileMeasureResponse,
    ) -> bool {
        assert_eq!(count, 2);
        // SAFETY: `measure_batch` provides exactly `count` writable response slots.
        let responses = unsafe { std::slice::from_raw_parts_mut(responses, count) };
        responses[0].key = 2;
        responses[1].key = 1;
        responses.iter_mut().for_each(|response| {
            response.status = MEASURE_READY;
            response.width = 10.0;
            response.height = 10.0;
        });
        true
    }

    fn request(key: u64) -> MeasurementRequest {
        MeasurementRequest {
            key: MeasurementKey::new(key).unwrap(),
            node: NodeId::new(key).unwrap(),
            element_type: ElementTypeId::new(1).unwrap(),
            environment_epoch: 1,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [AvailableSpace::MaxContent, AvailableSpace::MaxContent],
            },
            payload: MeasurementPayload::ReplacedContent(ReplacedContentMeasurePayload::default()),
        }
    }

    #[test]
    fn prepared_layout_leases_follow_cached_ids_and_release_rejected_batches() {
        use std::{cell::Cell, rc::Rc};
        struct Host {
            released: Rc<Cell<usize>>,
            accept: bool,
        }
        extern "C" fn release(pointer: *mut c_void) {
            // SAFETY: each callback receives its uniquely owned lease allocation.
            let counter = unsafe { Box::from_raw(pointer.cast::<Rc<Cell<usize>>>()) };
            counter.set(counter.get() + 1);
        }
        extern "C" fn measure(
            data: *mut c_void,
            requests: *const MobileMeasureRequest,
            count: usize,
            responses: *mut MobileMeasureResponse,
        ) -> bool {
            // SAFETY: the test retains Host and the adapter supplies count request/response entries.
            let (host, requests, responses) = unsafe {
                (
                    &*data.cast::<Host>(),
                    std::slice::from_raw_parts(requests, count),
                    std::slice::from_raw_parts_mut(responses, count),
                )
            };
            for (request, response) in requests.iter().zip(responses) {
                response.key = request.key;
                response.environment_epoch = request.environment_epoch;
                response.status = MEASURE_READY;
                response.width = 12.0;
                response.height = 16.0;
                response.metrics_mask = 4;
                response.prepared_content = request.key;
                response.prepared_layout = Box::into_raw(Box::new(host.released.clone())).cast();
                response.release_prepared_layout = Some(release);
            }
            host.accept
        }
        let released = Rc::new(Cell::new(0));
        let mut state = Host {
            released: released.clone(),
            accept: true,
        };
        let mut host = MobileMeasurementHost {
            callback: measure,
            data: (&mut state as *mut Host).cast(),
            prepared: Default::default(),
        };
        let surface = SurfaceId::new(1).unwrap();
        host.measure_batch(surface, &[request(1), request(2)], &mut Vec::new())
            .unwrap();
        assert_eq!(released.get(), 0);
        host.retain_prepared_content(
            surface,
            1,
            &mut [PreparedContentId::new(2).unwrap()].into_iter(),
        );
        assert_eq!(released.get(), 1);
        state.accept = false;
        assert!(
            host.measure_batch(surface, &[request(3), request(4)], &mut Vec::new())
                .is_err()
        );
        assert_eq!(released.get(), 3);
        drop(host);
        assert_eq!(released.get(), 4);
    }

    #[test]
    fn host_responses_must_preserve_request_positions() {
        let mut host = MobileMeasurementHost {
            prepared: std::collections::HashMap::new(),
            callback: reorder_responses,
            data: std::ptr::null_mut(),
        };
        let requests = [request(1), request(2)];
        let mut responses = Vec::new();

        let error = host
            .measure_batch(SurfaceId::new(1).unwrap(), &requests, &mut responses)
            .unwrap_err();

        assert_eq!(error.0, "mobile Host reordered measurement responses");
    }

    extern "C" fn observe_empty_batch(
        data: *mut c_void,
        requests: *const MobileMeasureRequest,
        count: usize,
        responses: *mut MobileMeasureResponse,
    ) -> bool {
        // SAFETY: the test passes a live `bool` as callback data.
        unsafe { *data.cast::<bool>() = requests.is_null() && responses.is_null() && count == 0 };
        true
    }

    #[test]
    fn empty_measurement_batch_uses_null_pointers() {
        let mut observed = false;
        let mut host = MobileMeasurementHost {
            prepared: std::collections::HashMap::new(),
            callback: observe_empty_batch,
            data: std::ptr::from_mut(&mut observed).cast(),
        };
        let mut responses = Vec::new();

        host.measure_batch(SurfaceId::new(1).unwrap(), &[], &mut responses)
            .unwrap();

        assert!(observed);
        assert!(responses.is_empty());
    }
}
