//! Rust-facing Host measurement boundary and layout driver.

use std::{error::Error, fmt};

use whisker_layout::LayoutSize;
use whisker_protocol::{
    MeasurementBatchError, MeasurementRequest, MeasurementResponse, NodeId, SurfaceId,
    validate_measurement_batch,
};

use crate::{LayoutProgress, SurfaceEngine, SurfaceError};

/// Host capability required to resolve one batch of intrinsic measurements.
///
/// Android, UIKit, DOM, and native Desktop bindings implement this information flow at
/// their generated boundary. The trait itself contains no platform types and
/// is also the conformance seam used by Rust-only tests.
pub trait MeasurementHost {
    /// Backend or binding failure returned before a batch can be accepted.
    type Error;

    /// Measures every request and appends exactly one correlated response for each.
    ///
    /// The output vector is empty on entry. Returning `Ok` commits no state by
    /// itself: Rust validates the complete response set transactionally before
    /// applying any result.
    fn measure_batch(
        &mut self,
        surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error>;
}

/// Limit for synchronous Host batches attempted by one layout drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostLayoutOptions {
    /// Maximum number of immediate request/response rounds.
    pub max_immediate_batches: usize,
}

impl Default for HostLayoutOptions {
    fn default() -> Self {
        Self {
            max_immediate_batches: 32,
        }
    }
}

/// Failure while driving Taffy through the Rust-to-Host measurement boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum HostLayoutError<HostError> {
    /// Retained surface or response semantics were invalid.
    Surface(SurfaceError),
    /// The Host binding failed without producing an accepted batch.
    Host(HostError),
    /// The Host returned a structurally malformed response set.
    InvalidBatch(MeasurementBatchError),
    /// Immediate measurements did not converge within the configured guard.
    BatchLimitExceeded {
        /// Configured maximum number of Host calls.
        limit: usize,
    },
}

impl<HostError: fmt::Debug> fmt::Display for HostLayoutError<HostError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker Host layout error: {self:?}")
    }
}

impl<HostError: Error + 'static> Error for HostLayoutError<HostError> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Surface(error) => Some(error),
            Self::Host(error) => Some(error),
            Self::InvalidBatch(_) | Self::BatchLimitExceeded { .. } => None,
        }
    }
}

impl<HostError> From<SurfaceError> for HostLayoutError<HostError> {
    fn from(error: SurfaceError) -> Self {
        Self::Surface(error)
    }
}

impl SurfaceEngine {
    /// Runs layout and all immediately answerable Host measurement batches.
    ///
    /// A synchronous Text provider normally makes this return
    /// [`LayoutProgress::Complete`] in one call. If a provider returns
    /// `Pending`, this returns blocked or provisional progress; a later
    /// Host-to-Rust [`MeasurementReady`](whisker_protocol::MeasurementReady)
    /// event is applied with [`SurfaceEngine::apply_measurement_ready`] before
    /// driving layout again.
    pub fn drive_layout_with_host<Host: MeasurementHost>(
        &mut self,
        root: NodeId,
        viewport: LayoutSize,
        environment_epoch: u64,
        host: &mut Host,
        options: HostLayoutOptions,
    ) -> Result<LayoutProgress, HostLayoutError<Host::Error>> {
        let mut batches = 0;
        loop {
            let progress =
                self.compute_layout_with_measurements(root, viewport, environment_epoch)?;
            if progress.requests().is_empty() {
                return Ok(progress);
            }
            if batches >= options.max_immediate_batches {
                return Err(HostLayoutError::BatchLimitExceeded {
                    limit: options.max_immediate_batches,
                });
            }

            let requests = progress.requests().to_vec();
            let mut responses = Vec::with_capacity(requests.len());
            host.measure_batch(self.surface(), &requests, &mut responses)
                .map_err(HostLayoutError::Host)?;
            validate_measurement_batch(&requests, &responses)
                .map_err(HostLayoutError::InvalidBatch)?;
            self.apply_measurement_responses(&responses)?;
            batches += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use whisker_protocol::{
        ApplyResult, ElementTypeId, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
        MeasureTextDirection, MeasureTextOverflow, MeasureTextWrap, MeasuredSize,
        MeasurementMetrics, MeasurementPayload, MeasurementPayloadError, MeasurementRequestId,
        MeasurementSpec, Operation, PendingMeasurePolicy, PreparedContentId,
        ReplacedContentMeasurePayload, TextMeasurePayload, TextMeasureStyle,
    };
    use whisker_style::{ComputedLayoutStyle, SpecifiedStyle, StyleEnvironment, resolve_style};

    use super::*;
    use crate::{FrameSink, PlainTextInput, RecordingRenderer, SceneError};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestHostError {
        Failed,
    }

    impl fmt::Display for TestHostError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("test Host failed")
        }
    }

    impl Error for TestHostError {}

    #[derive(Clone, Copy)]
    enum Reply {
        Ready(MeasuredSize),
        ReadyPreparedText,
        InvalidMetrics,
        Pending,
        Missing,
        Fail,
    }

    struct TestHost {
        reply: Reply,
        calls: Vec<(SurfaceId, Vec<MeasurementRequest>)>,
    }

    impl TestHost {
        fn new(reply: Reply) -> Self {
            Self {
                reply,
                calls: Vec::new(),
            }
        }
    }

    impl MeasurementHost for TestHost {
        type Error = TestHostError;

        fn measure_batch(
            &mut self,
            surface: SurfaceId,
            requests: &[MeasurementRequest],
            responses: &mut Vec<MeasurementResponse>,
        ) -> Result<(), Self::Error> {
            assert!(responses.is_empty());
            self.calls.push((surface, requests.to_vec()));
            match self.reply {
                Reply::Ready(size) => {
                    responses.extend(requests.iter().map(|request| MeasurementResponse::Ready {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        metrics: MeasurementMetrics::from_size(size),
                    }))
                }
                Reply::ReadyPreparedText => responses.extend(requests.iter().map(|request| {
                    let MeasurementPayload::Text(payload) = &request.payload else {
                        panic!("plain-text Host received a non-text request");
                    };
                    let width = payload.text.chars().count() as f32 * 9.0;
                    MeasurementResponse::Ready {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        metrics: MeasurementMetrics {
                            size: MeasuredSize::new(width, 21.0),
                            first_baseline: Some(15.0),
                            last_baseline: Some(15.0),
                            overflow: None,
                            prepared_content: Some(
                                PreparedContentId::new(request.key.get())
                                    .expect("measurement keys are non-zero"),
                            ),
                        },
                    }
                })),
                Reply::InvalidMetrics => {
                    responses.extend(requests.iter().map(|request| MeasurementResponse::Ready {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        metrics: MeasurementMetrics::from_size(MeasuredSize::new(-1.0, 0.0)),
                    }))
                }
                Reply::Pending => responses.extend(requests.iter().map(|request| {
                    MeasurementResponse::Pending {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        request_id: MeasurementRequestId::new(request.key.get())
                            .expect("non-zero request key"),
                        provisional: None,
                    }
                })),
                Reply::Missing => {}
                Reply::Fail => return Err(TestHostError::Failed),
            }
            Ok(())
        }
    }

    fn id<T>(constructor: impl FnOnce(u64) -> Option<T>) -> T {
        constructor(1).expect("non-zero test id")
    }

    fn text_spec() -> MeasurementSpec {
        MeasurementSpec {
            content_hash: 1,
            style_hash: 2,
            payload: MeasurementPayload::Text(TextMeasurePayload {
                text: "measure me".into(),
                style: TextMeasureStyle {
                    font_families: vec![MeasureFontFamily::Named("Inter".into())],
                    font_size: 16.0,
                    font_weight: 500,
                    font_style: MeasureFontStyle::Normal,
                    line_height: MeasureLineHeight::LogicalPixels(20.0),
                    letter_spacing: 0.5,
                },
                locale: Some("en-US".into()),
                direction: MeasureTextDirection::LeftToRight,
                wrap: MeasureTextWrap::Wrap,
                max_lines: Some(2),
                overflow: MeasureTextOverflow::Ellipsis,
            }),
            pending_policy: PendingMeasurePolicy::Block,
        }
    }

    fn surface_with_text() -> (SurfaceEngine, NodeId) {
        let mut surface = SurfaceEngine::new(id(SurfaceId::new));
        let root = surface
            .create_node(
                ElementTypeId::new(1).expect("non-zero element type"),
                ComputedLayoutStyle::default(),
            )
            .expect("create text node");
        surface
            .set_measurement(root, Some(text_spec()))
            .expect("register text measurement");
        (surface, root)
    }

    #[test]
    fn synchronous_host_completes_text_layout_before_returning() {
        let (mut surface, root) = surface_with_text();
        let mut host = TestHost::new(Reply::Ready(MeasuredSize::new(48.0, 20.0)));
        let progress = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(100.0, 100.0),
                7,
                &mut host,
                HostLayoutOptions::default(),
            )
            .expect("synchronous layout");

        assert!(progress.has_layout());
        assert!(progress.requests().is_empty());
        assert_eq!(host.calls.len(), 1);
        assert_eq!(host.calls[0].0, surface.surface());
        assert_eq!(
            host.calls[0].1[0].payload.kind(),
            whisker_protocol::MeasurementKind::Text
        );
        assert_eq!(
            surface.last_measurement(root).map(|metrics| metrics.size),
            Some(MeasuredSize::new(48.0, 20.0))
        );
    }

    #[test]
    fn plain_text_reaches_final_snapshot_and_incremental_frame_with_mock_host() {
        let resolved = resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
            .expect("default computed style");
        let mut surface = SurfaceEngine::new(id(SurfaceId::new));
        let root = surface
            .create_node(
                ElementTypeId::new(1).expect("text element type"),
                resolved.computed().layout().clone(),
            )
            .expect("create text node");
        let first_input = PlainTextInput::new("hello");
        let missing = NodeId::new(99).expect("missing node");
        assert_eq!(
            surface.set_plain_text(missing, &first_input, resolved.computed().inherited_text(),),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );
        let mut invalid = first_input.clone();
        invalid.locale = Some(String::new());
        assert_eq!(
            surface.set_plain_text(root, &invalid, resolved.computed().inherited_text()),
            Err(SurfaceError::Measurement(
                crate::MeasurementError::InvalidPayload {
                    node: root,
                    error: MeasurementPayloadError::InvalidLocale,
                }
            ))
        );
        assert!(
            surface
                .set_plain_text(root, &first_input, resolved.computed().inherited_text())
                .expect("lower first text")
        );
        assert!(
            !surface
                .set_plain_text(root, &first_input, resolved.computed().inherited_text())
                .expect("equal unmeasured text is idle")
        );

        let mut host = TestHost::new(Reply::ReadyPreparedText);
        let progress = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(200.0, 100.0),
                3,
                &mut host,
                HostLayoutOptions::default(),
            )
            .expect("measure and finalize first text layout");
        assert!(progress.has_layout());
        assert_eq!(host.calls.len(), 1);
        assert_eq!(
            surface.last_measurement(root),
            Some(&MeasurementMetrics {
                size: MeasuredSize::new(45.0, 21.0),
                first_baseline: Some(15.0),
                last_baseline: Some(15.0),
                overflow: None,
                prepared_content: PreparedContentId::new(1),
            })
        );

        let first_packet = surface
            .prepare_frame(3)
            .expect("prepare snapshot")
            .expect("snapshot has text work")
            .clone();
        assert_eq!(
            surface.set_plain_text(root, &first_input, resolved.computed().inherited_text()),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert!(matches!(
            first_packet
                .operations
                .iter()
                .find(|operation| matches!(operation, Operation::SetLayout { .. })),
            Some(Operation::SetLayout { node, rect })
                if *node == root && *rect == whisker_protocol::LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 45.0,
                    height: 21.0,
                }
        ));
        assert!(matches!(
            first_packet
                .operations
                .iter()
                .find(|operation| matches!(operation, Operation::SetText { .. })),
            Some(Operation::SetText { node, content })
                if *node == root
                    && content.payload.text == "hello"
                    && content.prepared_content == PreparedContentId::new(1)
        ));

        let mut renderer = RecordingRenderer::new(surface.surface());
        assert_eq!(
            renderer.present(&first_packet),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
        surface.accept_pending(1).expect("accept snapshot");
        assert_eq!(renderer.projection().node_count(), 1);

        let second_input = PlainTextInput::new("hello world");
        assert!(
            surface
                .set_plain_text(root, &second_input, resolved.computed().inherited_text())
                .expect("lower changed text")
        );
        surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(200.0, 100.0),
                3,
                &mut host,
                HostLayoutOptions::default(),
            )
            .expect("measure and finalize changed text layout");
        assert_eq!(host.calls.len(), 2);
        let delta = surface
            .prepare_frame(3)
            .expect("prepare delta")
            .expect("changed text has work")
            .clone();
        assert!(matches!(
            delta.operations.as_slice(),
            [Operation::SetText { node: text_node, content }, Operation::SetLayout { node: layout_node, rect }]
                if *text_node == root
                    && *layout_node == root
                    && content.payload.text == "hello world"
                    && content.prepared_content == PreparedContentId::new(2)
                    && rect.width == 99.0
                    && rect.height == 21.0
        ));
        assert_eq!(
            renderer.present(&delta),
            Ok(ApplyResult::Accepted { revision: 2 })
        );
        surface.accept_pending(2).expect("accept delta");

        assert!(
            !surface
                .set_plain_text(root, &second_input, resolved.computed().inherited_text())
                .expect("equal text is idle")
        );
        surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(200.0, 100.0),
                3,
                &mut host,
                HostLayoutOptions::default(),
            )
            .expect("equal text reuses retained layout");
        assert_eq!(host.calls.len(), 2);
        assert_eq!(surface.prepare_frame(3).expect("idle prepare"), None);
    }

    #[test]
    #[should_panic(expected = "plain-text Host received a non-text request")]
    fn plain_text_test_host_rejects_non_text_requests() {
        let mut surface = SurfaceEngine::new(id(SurfaceId::new));
        let root = surface
            .create_node(
                ElementTypeId::new(1).expect("element type"),
                ComputedLayoutStyle::default(),
            )
            .expect("create node");
        surface
            .set_measurement(
                root,
                Some(MeasurementSpec {
                    content_hash: 1,
                    style_hash: 1,
                    payload: MeasurementPayload::ReplacedContent(
                        ReplacedContentMeasurePayload::default(),
                    ),
                    pending_policy: PendingMeasurePolicy::Block,
                }),
            )
            .expect("register replaced measurement");
        let mut host = TestHost::new(Reply::ReadyPreparedText);
        let _ = surface.drive_layout_with_host(
            root,
            LayoutSize::new(100.0, 100.0),
            1,
            &mut host,
            HostLayoutOptions::default(),
        );
    }

    #[test]
    fn pending_host_returns_control_to_the_event_boundary() {
        let (mut surface, root) = surface_with_text();
        let mut host = TestHost::new(Reply::Pending);
        let progress = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(100.0, 100.0),
                1,
                &mut host,
                HostLayoutOptions::default(),
            )
            .expect("pending layout is valid");
        assert!(matches!(
            progress,
            LayoutProgress::Blocked {
                ref requests,
                pending: 1
            } if requests.is_empty()
        ));
        assert_eq!(host.calls.len(), 1);
    }

    #[test]
    fn host_and_batch_failures_are_transactional_and_diagnostic() {
        let (mut surface, root) = surface_with_text();
        let mut failed = TestHost::new(Reply::Fail);
        let error = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(100.0, 100.0),
                1,
                &mut failed,
                HostLayoutOptions::default(),
            )
            .expect_err("Host failure");
        assert_eq!(error, HostLayoutError::Host(TestHostError::Failed));
        assert!(error.source().is_some());
        assert!(error.to_string().contains("Host"));
        assert_eq!(TestHostError::Failed.to_string(), "test Host failed");

        let mut missing = TestHost::new(Reply::Missing);
        let error = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(100.0, 100.0),
                1,
                &mut missing,
                HostLayoutOptions::default(),
            )
            .expect_err("missing response");
        assert_eq!(
            error,
            HostLayoutError::InvalidBatch(MeasurementBatchError::MissingResponseKey {
                key: whisker_protocol::MeasurementKey::new(1).expect("first request key"),
            })
        );
        assert!(error.source().is_none());

        let mut invalid_metrics = TestHost::new(Reply::InvalidMetrics);
        let error = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(100.0, 100.0),
                1,
                &mut invalid_metrics,
                HostLayoutOptions::default(),
            )
            .expect_err("invalid metrics");
        assert_eq!(
            error,
            HostLayoutError::Surface(SurfaceError::Measurement(
                crate::MeasurementError::InvalidMetrics {
                    key: whisker_protocol::MeasurementKey::new(1).expect("first request key"),
                }
            ))
        );
    }

    #[test]
    fn surface_failure_and_batch_limit_do_not_call_host() {
        let mut empty = SurfaceEngine::new(id(SurfaceId::new));
        let mut host = TestHost::new(Reply::Ready(MeasuredSize::default()));
        let error = empty
            .drive_layout_with_host(
                id(NodeId::new),
                LayoutSize::new(1.0, 1.0),
                1,
                &mut host,
                HostLayoutOptions::default(),
            )
            .expect_err("unknown root");
        assert_eq!(
            error,
            HostLayoutError::Surface(SurfaceError::Layout(
                whisker_layout::LayoutError::UnknownNode(id(NodeId::new))
            ))
        );
        assert!(error.source().is_some());

        let (mut surface, root) = surface_with_text();
        let error = surface
            .drive_layout_with_host(
                root,
                LayoutSize::new(100.0, 100.0),
                1,
                &mut host,
                HostLayoutOptions {
                    max_immediate_batches: 0,
                },
            )
            .expect_err("zero batch guard");
        assert_eq!(error, HostLayoutError::BatchLimitExceeded { limit: 0 });
        assert!(error.source().is_none());
        assert!(host.calls.is_empty());
    }
}
