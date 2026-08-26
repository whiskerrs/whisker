use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;

use base64::Engine as _;
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use whisker::prelude::*;
use whisker::runtime::RuntimeWakeHandle;
use whisker::{ElementRegistry, RuntimeInstance, SurfaceRuntime};
use whisker_engine::{FrameSink, MeasurementProvider};
use whisker_host_conformance::{
    BackgroundBoxFixture, BackgroundImageFixture, BackgroundLayerFixture,
    BackgroundPaintLayerFixture, BackgroundSizeFixture, BackgroundSizeKeywordFixture,
    BorderFixture, BorderStyleFixture, ClipPathFixture, ClipReferenceBoxFixture, ClipShapeFixture,
    ColorFixture, Command, ConicGradientFixture, CornerRadiusFixture, FillRuleFixture,
    ImageRepeatFixture, LengthPercentageFixture, LinearGradientFixture, Manifest,
    OverflowClipFixture, PathCommandFixture, PixelSampleFixture, RadialGradientFixture,
    ResourceSourceFixture, ResourceStateFixture, SCHEMA_VERSION, Scenario, ScenarioSide,
    SceneNodeFixture, VisibilityFixture,
};
use whisker_protocol::{
    AvailableSpace, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode,
    BorderLineStyle, BoxClip, BoxPaint, ClipShape, FillRule, FrameHeader, FrameMode, FramePacket,
    GradientStop, ImageRepeat, LayoutGeometry, LayoutRect, MeasureConstraints, MeasureFontFamily,
    MeasureFontStyle, MeasureLineHeight, MeasureTextDirection, MeasureTextOverflow,
    MeasureTextWordBreak, MeasureTextWrap, MeasurementKey, MeasurementMetrics, MeasurementPayload,
    MeasurementRequest, MeasurementResponse, NodeId, Operation, OverflowClip, PaintBox, PaintColor,
    PaintCoordinate, PaintCornerRadius, PaintCorners, PaintEdges, PaintImage,
    PaintLengthPercentage, PaintPosition, PathCommand, ProtocolVersion, RadialGradientExtent,
    RadialGradientShape, ResourceCommand, ResourceDimensions, ResourceEvent, ResourceId,
    ResourceKind, ResourceRequest, ResourceSource, SurfaceId, TextContent, TextMeasurePayload,
    TextMeasureStyle, TextPaint, TextShadow, Transform, Visibility,
};
use whisker_style::StyleEnvironment;

use crate::measure::text::DomMeasurementProvider;
use crate::module_api::built_in_element_factories;
use crate::scene::frame_sink::DomFrameSink;
use crate::{WebResourceService, WebResourceState, WebResourceStore};

const MANIFEST: &str = include_str!("../../../../tests/host-conformance/manifest.json");
const RASTER_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAF0lEQVR4nAXBAQEAAACCIKb33EBkQpUOQdYIeRyCeLsAAAAASUVORK5CYII=";

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn padded_parent_preserves_child_border_box_coordinates() {
    let mut driver = Driver::new();
    let parent = NodeId::new(1).unwrap();
    let child = NodeId::new(2).unwrap();
    let registry = ElementRegistry::standard();
    let view = registry
        .registration_for_builtin(whisker::ElementTag::View)
        .unwrap()
        .element_type;
    let mut parent_paint = BoxPaint::default();
    let border_width = PaintLengthPercentage {
        length: 10.0,
        fraction: 0.0,
    };
    parent_paint.border_widths = PaintEdges {
        top: border_width,
        right: border_width,
        bottom: border_width,
        left: border_width,
    };
    parent_paint.border_styles = PaintEdges {
        top: BorderLineStyle::Solid,
        right: BorderLineStyle::Solid,
        bottom: BorderLineStyle::Solid,
        left: BorderLineStyle::Solid,
    };
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![
            Operation::CreateNode {
                node: parent,
                element_type: view,
            },
            Operation::CreateNode {
                node: child,
                element_type: view,
            },
            Operation::SetLayout {
                node: parent,
                geometry: LayoutGeometry {
                    border_box: LayoutRect {
                        x: 10.0,
                        y: 12.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    content_box: LayoutRect {
                        x: 20.0,
                        y: 20.0,
                        width: 60.0,
                        height: 60.0,
                    },
                },
            },
            Operation::SetBoxPaint {
                node: parent,
                paint: parent_paint,
            },
            Operation::SetLayout {
                node: child,
                geometry: LayoutGeometry {
                    border_box: LayoutRect {
                        x: 5.0,
                        y: 7.0,
                        width: 10.0,
                        height: 11.0,
                    },
                    content_box: LayoutRect {
                        width: 10.0,
                        height: 11.0,
                        ..LayoutRect::default()
                    },
                },
            },
            Operation::InsertChild {
                parent,
                child,
                index: 0,
            },
        ],
    };
    driver.sink.present(&packet).unwrap();

    let parent_bounds = driver.node(1).get_bounding_client_rect();
    let child_bounds = driver.node(2).get_bounding_client_rect();
    assert_eq!(child_bounds.left() - parent_bounds.left(), 5.0);
    assert_eq!(child_bounds.top() - parent_bounds.top(), 7.0);
    let parent_style = driver
        .node(1)
        .dyn_into::<web_sys::HtmlElement>()
        .unwrap()
        .style();
    assert_style(&parent_style, "padding", "10px");
}

#[wasm_bindgen_test]
fn missing_background_resource_rejects_before_dom_mutation() {
    let mut driver = Driver::new();
    driver
        .sink
        .present(&packet(
            1,
            [0.0, 0.0, 20.0, 20.0],
            &ColorFixture::Named {
                value: "red".into(),
            },
            None,
        ))
        .unwrap();
    let before = driver.root.inner_html();
    let node = NodeId::new(1).unwrap();
    let view = ElementRegistry::standard()
        .registration_for_builtin(whisker::ElementTag::View)
        .unwrap()
        .element_type;
    let missing = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 2,
            frame_id: 2,
            base_revision: 0,
            target_revision: 2,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![
            Operation::CreateNode {
                node,
                element_type: view,
            },
            Operation::SetLayout {
                node,
                geometry: LayoutRect {
                    width: 20.0,
                    height: 20.0,
                    ..LayoutRect::default()
                }
                .into(),
            },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![BackgroundLayer {
                    image: PaintImage::Resource(ResourceId::new(99).unwrap()),
                    position: PaintPosition::default(),
                    size: BackgroundSize::Auto,
                    repeat_x: ImageRepeat::Repeat,
                    repeat_y: ImageRepeat::Repeat,
                    origin: PaintBox::Padding,
                    clip: PaintBox::Border,
                    attachment: BackgroundAttachment::Scroll,
                    blend_mode: BlendMode::Normal,
                }],
            },
        ],
    };
    let error = driver.sink.present(&missing).unwrap_err();
    assert!(
        error.to_string().contains("resource 99 is not registered"),
        "unexpected rejection: {error}"
    );
    assert_eq!(driver.root.inner_html(), before);
}

#[wasm_bindgen_test]
async fn stale_resource_completion_cannot_replace_or_release_current_generation() {
    let store = WebResourceStore::new();
    let service = WebResourceService::new(store.clone());
    let resource = ResourceId::new(1).unwrap();
    let mut first = Box::pin(
        service.handle(ResourceCommand::Load(ResourceRequest {
            resource,
            generation: 1,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::Bytes {
                media_type: "image/png".into(),
                data: base64::engine::general_purpose::STANDARD
                    .decode(RASTER_PNG_BASE64)
                    .unwrap(),
            },
        })),
    );
    std::future::poll_fn(|context| match first.as_mut().poll(context) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(result) => panic!("first browser decode completed synchronously: {result:?}"),
    })
    .await;

    let current = service
        .handle(ResourceCommand::Load(ResourceRequest {
            resource,
            generation: 2,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::Url(format!("data:image/png;base64,{RASTER_PNG_BASE64}")),
        }))
        .await
        .unwrap();
    assert!(matches!(
        current,
        Some(ResourceEvent::Ready { generation: 2, .. })
    ));
    let current_url = store.url(resource).unwrap();

    assert_eq!(first.await.unwrap(), None);
    assert_eq!(service.event(resource, 1), None);
    assert_eq!(store.url(resource).as_deref(), Some(current_url.as_str()));
    service
        .handle(ResourceCommand::Release {
            resource,
            generation: 1,
        })
        .await
        .unwrap();
    assert_eq!(store.url(resource).as_deref(), Some(current_url.as_str()));
    assert!(matches!(
        service.state(resource, 2),
        Some(WebResourceState::Ready { .. })
    ));
}

#[wasm_bindgen_test]
async fn typed_resource_commands_and_events_cross_the_web_runtime_boundary() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 16.0),
    );
    let mut runtime = RuntimeInstance::new(
        surface,
        RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }),
    );
    runtime.mount(|| render! { view() }).unwrap();
    let store = WebResourceStore::new();
    let service = WebResourceService::new(store);
    let resource = ResourceId::new(8).unwrap();
    runtime
        .surface()
        .enqueue_resource_command(ResourceCommand::Load(ResourceRequest {
            resource,
            generation: 1,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::Url(format!("data:image/png;base64,{RASTER_PNG_BASE64}")),
        }))
        .unwrap();

    let commands = runtime.surface().take_resource_commands();
    assert_eq!(commands.len(), 1);
    for command in commands {
        service.handle(command).await.unwrap();
    }
    let events = service.take_events();
    assert_eq!(events.len(), 1);
    let expected = events[0].clone();
    let wakes_before_event = wakes.load(Ordering::SeqCst);
    for event in events {
        assert_eq!(
            runtime.dispatch_resource_event(&event).unwrap(),
            whisker::ResourceEventApply::Applied
        );
    }
    assert_eq!(
        runtime.surface().resource_event(resource, 1),
        Some(expected)
    );
    assert!(wakes.load(Ordering::SeqCst) > wakes_before_event);
}

struct Driver {
    root: web_sys::Element,
    sink: DomFrameSink,
    resources: WebResourceStore,
    resource_service: WebResourceService,
    resource_urls: HashMap<u64, String>,
    resource_dimensions: HashMap<u64, [f32; 2]>,
    resource_lifecycle: bool,
    measurements: DomMeasurementProvider,
    measurement_results: HashMap<u64, MeasurementMetrics>,
    expected_box: Option<ExpectedBox>,
    expected_scene: Option<Vec<SceneNodeFixture>>,
}

#[derive(Clone, Debug)]
struct ExpectedBox {
    rect: [f32; 4],
    background: ColorFixture,
    border: Option<BorderFixture>,
}

impl Driver {
    fn new() -> Self {
        let document = web_sys::window().unwrap().document().unwrap();
        let root = document.create_element("div").unwrap();
        root.set_attribute("data-whisker-conformance-root", "")
            .unwrap();
        document.body().unwrap().append_child(&root).unwrap();
        let surface = SurfaceId::new(1).unwrap();
        let elements = ElementRegistry::standard();
        let resources = WebResourceStore::new();
        let resource_service = WebResourceService::new(resources.clone());
        let measurements = DomMeasurementProvider::new(document.clone());
        let sink = DomFrameSink::new_with_resources(
            document,
            root.clone(),
            surface,
            elements.registrations(),
            &built_in_element_factories(),
            resources.clone(),
        )
        .unwrap();
        Self {
            root,
            sink,
            resources,
            resource_service,
            resource_urls: HashMap::new(),
            resource_dimensions: HashMap::new(),
            resource_lifecycle: false,
            measurements,
            measurement_results: HashMap::new(),
            expected_box: None,
            expected_scene: None,
        }
    }

    async fn execute(&mut self, side: &ScenarioSide) {
        for command in &side.commands {
            match command {
                Command::AttachSurface {
                    width,
                    height,
                    scale,
                } => {
                    assert!(*width > 0.0 && *height > 0.0 && *scale > 0.0);
                    set_style(&self.root, "position", "relative");
                    set_style(&self.root, "width", &format!("{width}px"));
                    set_style(&self.root, "height", &format!("{height}px"));
                }
                Command::RegisterRasterResource {
                    id,
                    width,
                    height,
                    pixels,
                } => self.register_raster_resource(*id, *width, *height, pixels),
                Command::LoadRasterResource {
                    id,
                    generation,
                    source,
                } => {
                    self.resource_lifecycle = true;
                    let source = match source {
                        ResourceSourceFixture::Url { value } => ResourceSource::Url(value.clone()),
                        ResourceSourceFixture::Bytes { media_type, base64 } => {
                            ResourceSource::Bytes {
                                media_type: media_type.clone(),
                                data: base64::engine::general_purpose::STANDARD
                                    .decode(base64)
                                    .unwrap(),
                            }
                        }
                    };
                    self.resource_service
                        .handle(ResourceCommand::Load(ResourceRequest {
                            resource: ResourceId::new(*id).unwrap(),
                            generation: *generation,
                            kind: ResourceKind::RasterImage,
                            source,
                        }))
                        .await
                        .unwrap();
                }
                Command::ReleaseRasterResource { id, generation } => {
                    self.resource_service
                        .handle(ResourceCommand::Release {
                            resource: ResourceId::new(*id).unwrap(),
                            generation: *generation,
                        })
                        .await
                        .unwrap();
                }
                Command::CheckpointResource {
                    id,
                    generation,
                    state,
                    width,
                    height,
                } => self.assert_resource_state(*id, *generation, *state, *width, *height),
                Command::PresentBox {
                    revision,
                    rect,
                    background,
                    border,
                } => {
                    let packet = packet(*revision, *rect, background, border.as_ref());
                    self.sink.present(&packet).unwrap();
                    self.expected_box = Some(ExpectedBox {
                        rect: *rect,
                        background: background.clone(),
                        border: border.clone(),
                    });
                    self.expected_scene = None;
                }
                Command::PresentScene { revision, nodes } => {
                    self.sink.present(&scene_packet(*revision, nodes)).unwrap();
                    self.expected_scene = Some(nodes.clone());
                    self.expected_box = None;
                }
                Command::Checkpoint { name, samples, .. } => {
                    if self.expected_scene.is_some() {
                        let expected_checkpoint = if name.starts_with("paint.transform.") {
                            name.as_str()
                        } else if name == "paint.text.shadow-single" {
                            "paint.text.shadow-single"
                        } else if name == "paint.text.decoration-lynx" {
                            "paint.text.decoration-lynx"
                        } else if name == "paint.text.align-lynx" {
                            "paint.text.align-lynx"
                        } else if name == "paint.text.indent-lynx" {
                            "paint.text.indent-lynx"
                        } else if name == "paint.text.wrap-overflow-lynx" {
                            "paint.text.wrap-overflow-lynx"
                        } else if name == "paint.text.font-features-lynx" {
                            "paint.text.font-features-lynx"
                        } else if name == "paint.text.basic-style-lynx" {
                            "paint.text.basic-style-lynx"
                        } else if name == "interaction.pointer.lynx" {
                            "interaction.pointer.lynx"
                        } else if name == "paint.visual-effects.image-rendering-pixelated" {
                            "paint.visual-effects.image-rendering-pixelated"
                        } else if self.resource_lifecycle {
                            "paint.background-layers.resource-lifecycle"
                        } else {
                            self
                            .expected_scene
                            .as_ref()
                            .and_then(|nodes| {
                                nodes
                                    .iter()
                                    .flat_map(|node| &node.background_layers)
                                    .find_map(|layer| {
                                        let round_auto_axis = matches!(
                                            layer.geometry.size,
                                            BackgroundSizeFixture::ExplicitAxes {
                                                width: Some(_),
                                                height: None,
                                            } if layer.geometry.repeat_x == ImageRepeatFixture::Round
                                                && layer.geometry.repeat_y != ImageRepeatFixture::Round
                                        ) || matches!(
                                            layer.geometry.size,
                                            BackgroundSizeFixture::ExplicitAxes {
                                                width: None,
                                                height: Some(_),
                                            } if layer.geometry.repeat_y == ImageRepeatFixture::Round
                                                && layer.geometry.repeat_x != ImageRepeatFixture::Round
                                        );
                                        if round_auto_axis {
                                            return Some(
                                                "paint.background-layers.round-auto-aspect-ratio",
                                            );
                                        }
                                        match layer.geometry.size {
                                            BackgroundSizeFixture::Keyword(
                                                BackgroundSizeKeywordFixture::Cover,
                                            ) => Some("paint.background-layers.size-cover"),
                                            BackgroundSizeFixture::Keyword(
                                                BackgroundSizeKeywordFixture::Contain,
                                            ) => Some("paint.background-layers.size-contain"),
                                            BackgroundSizeFixture::Keyword(
                                                BackgroundSizeKeywordFixture::Auto,
                                            )
                                            | BackgroundSizeFixture::ExplicitAxes { .. } => {
                                                Some("paint.background-layers.intrinsic-auto")
                                            }
                                            BackgroundSizeFixture::ExplicitPair(_) => None,
                                        }
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| node.backdrop_blur.is_some())
                                            .then_some("paint.visual-effects.backdrop-blur")
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.image_rendering
                                                    == whisker_host_conformance::ImageRenderingFixture::Pixelated
                                            })
                                            .then_some(
                                                "paint.visual-effects.image-rendering-pixelated",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes.iter().find_map(|node| {
                                            node.clip_path
                                                .as_ref()
                                                .map(|clip| match &clip.shape {
                                                    ClipShapeFixture::Inset { .. } => {
                                                        "paint.visual-effects.clip-path-inset"
                                                    }
                                                    ClipShapeFixture::Circle { .. } => {
                                                        "paint.visual-effects.clip-path-circle"
                                                    }
                                                    ClipShapeFixture::Ellipse { .. } => {
                                                        "paint.visual-effects.clip-path-ellipse"
                                                    }
                                                    ClipShapeFixture::Path { fill_rule, .. } => {
                                                        match fill_rule {
                                                            FillRuleFixture::NonZero => "paint.visual-effects.clip-path-path-nonzero",
                                                            FillRuleFixture::EvenOdd => "paint.visual-effects.clip-path-path-evenodd",
                                                        }
                                                    }
                                                })
                                        })
                                    })
                                    .or_else(|| {
                                        nodes.iter().find_map(|node| {
                                            node.box_shadows.first().map(|shadow| {
                                                if node.box_shadows.len() > 1 {
                                                    "paint.visual-effects.box-shadow-multiple"
                                                } else if shadow.inset {
                                                    "paint.visual-effects.box-shadow-inset"
                                                } else if shadow.blur_radius != 0.0 {
                                                    "paint.visual-effects.box-shadow-blur"
                                                } else if shadow.spread_radius != 0.0 {
                                                    "paint.visual-effects.box-shadow-spread"
                                                } else {
                                                    "paint.visual-effects.box-shadow-offset"
                                                }
                                            })
                                        })
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layers.iter().any(|layer| {
                                                    matches!(
                                                        layer.image,
                                                        BackgroundImageFixture::Resource(_)
                                                    )
                                                })
                                            })
                                            .then_some("paint.background-layers.resource-image")
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| !node.background_layers.is_empty())
                                            .then_some("paint.background-layers.stacking")
                                    })
                                    .or_else(|| background_repeat_checkpoint(nodes))
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer.clip
                                                    == BackgroundBoxFixture::Content
                                            })
                                            .then_some(
                                                "paint.background-layers.clip-content-box",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer.origin
                                                    == BackgroundBoxFixture::Content
                                            })
                                            .then_some(
                                                "paint.background-layers.origin-content-box",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer.clip
                                                    == BackgroundBoxFixture::Padding
                                            })
                                            .then_some(
                                                "paint.background-layers.clip-padding-box",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer.clip
                                                    == BackgroundBoxFixture::BorderArea
                                            })
                                            .then_some(
                                                "paint.background-layers.clip-border-area",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer.origin
                                                    == BackgroundBoxFixture::Border
                                            })
                                            .then_some("paint.background-layers.origin-border-box")
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer.position
                                                    != [LengthPercentageFixture::default(); 2]
                                            })
                                            .then_some(
                                                "paint.background-layers.position-length-percentage",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes
                                            .iter()
                                            .any(|node| {
                                                node.background_layer
                                                    != BackgroundLayerFixture::default()
                                            })
                                            .then_some(
                                                "paint.background-layers.explicit-size-no-repeat",
                                            )
                                    })
                                    .or_else(|| {
                                        nodes.iter().find_map(|node| {
                                            node.linear_gradient
                                                .as_ref()
                                                .map(|_| "paint.background-layers.linear-gradient")
                                                .or_else(|| {
                                                    node.radial_gradient.as_ref().map(|_| {
                                                        "paint.background-layers.radial-gradient"
                                                    })
                                                })
                                                .or_else(|| {
                                                    node.conic_gradient.as_ref().map(|_| {
                                                        "paint.background-layers.conic-gradient"
                                                    })
                                                })
                                        })
                                    })
                            })
                            .unwrap_or("paint.box")
                        };
                        assert_eq!(name, expected_checkpoint);
                        self.assert_scene_is_projected(
                            if name.starts_with("paint.visual-effects.box-shadow-") {
                                &[]
                            } else {
                                samples
                            },
                        );
                    } else {
                        assert_eq!(name, "paint.box");
                        self.assert_box_is_projected();
                    }
                }
                Command::MeasureText {
                    key,
                    text,
                    font_families,
                    font_size,
                    font_weight,
                    font_style,
                    line_height,
                    letter_spacing,
                    available_width,
                } => self.measure_text(
                    *key,
                    text,
                    font_families,
                    *font_size,
                    *font_weight,
                    *font_style,
                    *line_height,
                    *letter_spacing,
                    *available_width,
                ),
                Command::CheckpointMeasurement {
                    key,
                    min_width,
                    max_width,
                    min_height,
                    max_height,
                    prepared_content,
                } => self.assert_measurement(
                    *key,
                    [*min_width, *max_width],
                    [*min_height, *max_height],
                    *prepared_content,
                ),
                Command::EmitPointer { .. } | Command::CheckpointInput { .. } => {
                    panic!("non-paint command reached the Web paint runner")
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn measure_text(
        &mut self,
        key: u64,
        text: &str,
        font_families: &[String],
        font_size: f32,
        font_weight: u16,
        font_style: whisker_host_conformance::FontStyleFixture,
        line_height: f32,
        letter_spacing: f32,
        available_width: f32,
    ) {
        let key_id = MeasurementKey::new(key).expect("fixture measurement key is non-zero");
        let element_type = ElementRegistry::standard()
            .registration_for_builtin(whisker::ElementTag::Text)
            .expect("standard Text registration")
            .element_type;
        let request = MeasurementRequest {
            key: key_id,
            node: NodeId::new(key).expect("fixture measurement node is non-zero"),
            element_type,
            environment_epoch: 1,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [
                    AvailableSpace::Definite(available_width),
                    AvailableSpace::MaxContent,
                ],
            },
            payload: MeasurementPayload::Text(TextMeasurePayload {
                text: text.to_owned(),
                style: TextMeasureStyle {
                    font_families: font_families
                        .iter()
                        .map(|family| {
                            if family == "system" {
                                MeasureFontFamily::System
                            } else {
                                MeasureFontFamily::Named(family.clone())
                            }
                        })
                        .collect(),
                    font_size,
                    font_weight,
                    font_style: fixture_measure_font_style(font_style),
                    line_height: MeasureLineHeight::LogicalPixels(line_height),
                    letter_spacing,
                    ..TextMeasureStyle::default()
                },
                locale: None,
                direction: MeasureTextDirection::Auto,
                alignment: whisker_protocol::MeasureTextAlignment::Start,
                indent: Default::default(),
                wrap: MeasureTextWrap::Wrap,
                word_break: MeasureTextWordBreak::Normal,
                max_lines: None,
                overflow: MeasureTextOverflow::Clip,
            }),
        };
        let mut responses = Vec::new();
        self.measurements
            .measure_batch(SurfaceId::new(1).unwrap(), &[request], &mut responses)
            .unwrap();
        let response = responses
            .pop()
            .expect("DOM measurement provider returned one response");
        assert!(responses.is_empty());
        let MeasurementResponse::Ready {
            key: response_key,
            environment_epoch,
            metrics,
        } = response
        else {
            panic!("DOM text measurement must complete synchronously")
        };
        assert_eq!(response_key, key_id);
        assert_eq!(environment_epoch, 1);
        assert!(metrics.is_valid());
        self.measurement_results.insert(key, metrics);
    }

    fn assert_measurement(
        &self,
        key: u64,
        width_range: [f32; 2],
        height_range: [f32; 2],
        prepared_content: Option<bool>,
    ) {
        let metrics = self
            .measurement_results
            .get(&key)
            .unwrap_or_else(|| panic!("missing DOM measurement result for key {key}"));
        assert!(
            (width_range[0]..=width_range[1]).contains(&metrics.size.width),
            "measured width {} is outside {width_range:?}",
            metrics.size.width
        );
        assert!(
            (height_range[0]..=height_range[1]).contains(&metrics.size.height),
            "measured height {} is outside {height_range:?}",
            metrics.size.height
        );
        if let Some(expected) = prepared_content {
            assert_eq!(metrics.prepared_content.is_some(), expected);
        }
    }

    fn register_raster_resource(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        pixels: &[ColorFixture],
    ) {
        let document = web_sys::window().unwrap().document().unwrap();
        let canvas = document
            .create_element("canvas")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();
        canvas.set_width(width);
        canvas.set_height(height);
        let context = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<web_sys::CanvasRenderingContext2d>()
            .unwrap();
        let rgba = pixels
            .iter()
            .flat_map(fixture_color_rgba)
            .collect::<Vec<_>>();
        let image = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(rgba.as_slice()),
            width,
            height,
        )
        .unwrap();
        context.put_image_data(&image, 0.0, 0.0).unwrap();
        let url = canvas.to_data_url_with_type("image/png").unwrap();
        self.resources
            .register_url(ResourceId::new(id).unwrap(), url.clone())
            .unwrap();
        self.resource_urls.insert(id, url);
        self.resource_dimensions
            .insert(id, [width as f32, height as f32]);
    }

    fn assert_resource_state(
        &mut self,
        id: u64,
        generation: u64,
        expected: ResourceStateFixture,
        width: Option<u32>,
        height: Option<u32>,
    ) {
        let resource = ResourceId::new(id).unwrap();
        let state = self
            .resource_service
            .state(resource, generation)
            .unwrap_or_else(|| panic!("missing Web resource state {id}:{generation}"));
        match expected {
            ResourceStateFixture::Ready => {
                let dimensions = ResourceDimensions {
                    width: width.unwrap() as f32,
                    height: height.unwrap() as f32,
                    scale: 1.0,
                };
                assert_eq!(state, WebResourceState::Ready { dimensions });
                assert_eq!(
                    self.resource_service.event(resource, generation),
                    Some(ResourceEvent::Ready {
                        resource,
                        generation,
                        dimensions: Some(dimensions),
                    })
                );
                self.resource_urls
                    .insert(id, self.resources.url(resource).unwrap());
                self.resource_dimensions
                    .insert(id, [dimensions.width, dimensions.height]);
            }
            ResourceStateFixture::Failed => {
                let WebResourceState::Failed { code, diagnostic } = state else {
                    panic!("resource {id}:{generation} was not failed: {state:?}");
                };
                assert!(diagnostic.is_some());
                assert_eq!(
                    self.resource_service.event(resource, generation),
                    Some(ResourceEvent::Failed {
                        resource,
                        generation,
                        code,
                        diagnostic,
                    })
                );
            }
            ResourceStateFixture::Released => {
                assert_eq!(state, WebResourceState::Released);
            }
        }
    }

    fn assert_box_is_projected(&self) {
        let expected = self
            .expected_box
            .as_ref()
            .expect("paint checkpoint must follow present_box");
        let node = self
            .root
            .query_selector("[data-whisker-node='1']")
            .unwrap()
            .expect("fixture box DOM node");
        let html = node.dyn_ref::<web_sys::HtmlElement>().unwrap();
        let style = html.style();

        assert_style(&style, "position", "absolute");
        assert_style(&style, "box-sizing", "border-box");
        assert_style(&style, "left", &fixture_px(expected.rect[0]));
        assert_style(&style, "top", &fixture_px(expected.rect[1]));
        assert_style(&style, "width", &fixture_px(expected.rect[2]));
        assert_style(&style, "height", &fixture_px(expected.rect[3]));
        assert_style(
            &style,
            "background-color",
            &fixture_color_css(&expected.background),
        );

        assert_border_is_projected(&style, expected.border.as_ref());
    }

    fn assert_scene_is_projected(&self, samples: &[PixelSampleFixture]) {
        let expected = self
            .expected_scene
            .as_ref()
            .expect("paint checkpoint must follow present_scene");
        for fixture_node in expected {
            let node = self.node(fixture_node.id);
            let html = node.dyn_ref::<web_sys::HtmlElement>().unwrap();
            let style = html.style();
            assert_style(&style, "left", &fixture_px(fixture_node.rect[0]));
            assert_style(&style, "top", &fixture_px(fixture_node.rect[1]));
            assert_style(&style, "width", &fixture_px(fixture_node.rect[2]));
            assert_style(&style, "height", &fixture_px(fixture_node.rect[3]));
            if fixture_node.content_box.is_some() {
                let padding = fixture_padding(fixture_node);
                assert_style(&style, "padding-top", &fixture_px(padding[0]));
                assert_style(&style, "padding-right", &fixture_px(padding[1]));
                assert_style(&style, "padding-bottom", &fixture_px(padding[2]));
                assert_style(&style, "padding-left", &fixture_px(padding[3]));
            }
            assert_style(
                &style,
                "background-color",
                &fixture_color_css(&fixture_node.background),
            );
            assert_border_is_projected(&style, fixture_node.border.as_ref());
            assert_style(
                &style,
                "cursor",
                match fixture_node.cursor {
                    whisker_host_conformance::CursorFixture::Auto => "auto",
                    whisker_host_conformance::CursorFixture::Pointer => "pointer",
                    whisker_host_conformance::CursorFixture::Text => "text",
                    whisker_host_conformance::CursorFixture::Grab => "grab",
                    whisker_host_conformance::CursorFixture::None => "none",
                },
            );
            assert_style(
                &style,
                "pointer-events",
                match fixture_node.pointer_events {
                    whisker_host_conformance::PointerEventsFixture::Auto => "auto",
                    whisker_host_conformance::PointerEventsFixture::None => "none",
                },
            );
            if let Some(text) = &fixture_node.text {
                let text_node = node
                    .query_selector("[data-whisker-text]")
                    .unwrap()
                    .expect("text element has a native text projection");
                let text_style = text_node.dyn_ref::<web_sys::HtmlElement>().unwrap().style();
                assert_eq!(
                    text_node.text_content().as_deref(),
                    Some(text.value.as_str())
                );
                assert_style(&text_style, "font-size", &fixture_px(text.font_size));
                assert_style(
                    &text_style,
                    "font-family",
                    &fixture_font_families_css(&text.font_families),
                );
                assert_style(
                    &text_style,
                    "font-style",
                    match text.font_style {
                        whisker_host_conformance::FontStyleFixture::Normal => "normal",
                        whisker_host_conformance::FontStyleFixture::Italic => "italic",
                        whisker_host_conformance::FontStyleFixture::Oblique => "oblique",
                    },
                );
                assert_style(
                    &text_style,
                    "line-height",
                    &text.line_height.map_or_else(|| "normal".into(), fixture_px),
                );
                assert_style(
                    &text_style,
                    "letter-spacing",
                    &fixture_px(text.letter_spacing),
                );
                assert_style(
                    &text_style,
                    "font-feature-settings",
                    &fixture_font_settings(&text.font_features, |value| value.value.to_string()),
                );
                assert_style(
                    &text_style,
                    "font-variation-settings",
                    &fixture_font_settings(&text.font_variations, |value| value.value.to_string()),
                );
                assert_style(
                    &text_style,
                    "font-optical-sizing",
                    match text.font_optical_sizing {
                        whisker_host_conformance::FontOpticalSizingFixture::Auto => "auto",
                        whisker_host_conformance::FontOpticalSizingFixture::None => "none",
                    },
                );
                assert_style(
                    &text_style,
                    "text-align",
                    match text.alignment {
                        whisker_host_conformance::TextAlignmentFixture::Start => "start",
                        whisker_host_conformance::TextAlignmentFixture::End => "end",
                        whisker_host_conformance::TextAlignmentFixture::Left => "left",
                        whisker_host_conformance::TextAlignmentFixture::Right => "right",
                        whisker_host_conformance::TextAlignmentFixture::Center => "center",
                    },
                );
                if text.indent.logical_pixels != 0.0 || text.indent.percentage != 0.0 {
                    assert_style(
                        &text_style,
                        "text-indent",
                        &format!(
                            "calc({}px + {}%)",
                            text.indent.logical_pixels, text.indent.percentage
                        ),
                    );
                }
                assert_style(
                    &text_style,
                    "white-space",
                    match text.white_space {
                        whisker_host_conformance::WhiteSpaceFixture::Normal => "normal",
                        whisker_host_conformance::WhiteSpaceFixture::NoWrap => "nowrap",
                    },
                );
                assert_style(
                    &text_style,
                    "word-break",
                    match text.word_break {
                        whisker_host_conformance::WordBreakFixture::Normal => "normal",
                        whisker_host_conformance::WordBreakFixture::BreakAll => "break-all",
                        whisker_host_conformance::WordBreakFixture::KeepAll => "keep-all",
                    },
                );
                assert_style(
                    &text_style,
                    "text-overflow",
                    match text.overflow {
                        whisker_host_conformance::TextOverflowFixture::Clip => "clip",
                        whisker_host_conformance::TextOverflowFixture::Ellipsis => "ellipsis",
                    },
                );
                assert_style(&text_style, "font-weight", &text.font_weight.to_string());
                assert_style(&text_style, "color", &fixture_color_css(&text.color));
                let expected_shadow = text.shadow.as_ref().map_or_else(
                    || "none".to_string(),
                    |shadow| {
                        format!(
                            "{}px {}px {}px {}",
                            shadow.offset[0],
                            shadow.offset[1],
                            shadow.blur_radius,
                            fixture_color_css(&shadow.color),
                        )
                    },
                );
                assert_style(&text_style, "text-shadow", &expected_shadow);
                let expected_decoration = text.decoration.as_ref();
                assert_style(
                    &text_style,
                    "text-decoration-line",
                    expected_decoration.map_or("none", |decoration| match decoration.line {
                        whisker_host_conformance::TextDecorationLineFixture::Underline => {
                            "underline"
                        }
                        whisker_host_conformance::TextDecorationLineFixture::LineThrough => {
                            "line-through"
                        }
                    }),
                );
                if let Some(decoration) = expected_decoration {
                    assert_style(
                        &text_style,
                        "text-decoration-style",
                        match decoration.style {
                            whisker_host_conformance::TextDecorationStyleFixture::Solid => "solid",
                            whisker_host_conformance::TextDecorationStyleFixture::Double => {
                                "double"
                            }
                            whisker_host_conformance::TextDecorationStyleFixture::Dotted => {
                                "dotted"
                            }
                            whisker_host_conformance::TextDecorationStyleFixture::Dashed => {
                                "dashed"
                            }
                            whisker_host_conformance::TextDecorationStyleFixture::Wavy => "wavy",
                        },
                    );
                    assert_style(
                        &text_style,
                        "text-decoration-color",
                        &fixture_color_css(&decoration.color),
                    );
                }
            }
            let mut expected_shadows = fixture_node
                .box_shadows
                .iter()
                .map(|shadow| {
                    format!(
                        "{} {}px {}px {}px {}px{}",
                        fixture_color_css(&shadow.color),
                        shadow.offset[0],
                        shadow.offset[1],
                        shadow.blur_radius,
                        shadow.spread_radius,
                        if shadow.inset { " inset" } else { "" },
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            if expected_shadows.is_empty()
                && (fixture_node.clip_path.is_some()
                    || fixture_node.backdrop_blur.is_some()
                    || fixture_node.image_rendering
                        != whisker_host_conformance::ImageRenderingFixture::Auto)
            {
                expected_shadows = "none".into();
            }
            assert_eq!(
                style.get_property_value("box-shadow").unwrap(),
                expected_shadows
            );
            let mut expected_clip_path = fixture_node
                .clip_path
                .as_ref()
                .map(fixture_clip_path_css)
                .unwrap_or_default();
            if expected_clip_path.is_empty()
                && (!fixture_node.box_shadows.is_empty()
                    || fixture_node.backdrop_blur.is_some()
                    || fixture_node.image_rendering
                        != whisker_host_conformance::ImageRenderingFixture::Auto)
            {
                expected_clip_path = "none".into();
            }
            assert_eq!(
                style.get_property_value("clip-path").unwrap(),
                expected_clip_path
            );
            if let Some(radius) = fixture_node.backdrop_blur {
                assert_style(&style, "backdrop-filter", &format!("blur({radius}px)"));
            }
            if fixture_node.image_rendering != whisker_host_conformance::ImageRenderingFixture::Auto
            {
                assert_style(
                    &style,
                    "image-rendering",
                    match fixture_node.image_rendering {
                        whisker_host_conformance::ImageRenderingFixture::Pixelated => "pixelated",
                        whisker_host_conformance::ImageRenderingFixture::Auto
                        | whisker_host_conformance::ImageRenderingFixture::CrispEdges => "auto",
                    },
                );
            }
            assert_style(
                &style,
                "overflow-x",
                fixture_overflow_css(fixture_node.clip.horizontal),
            );
            assert_style(
                &style,
                "overflow-y",
                fixture_overflow_css(fixture_node.clip.vertical),
            );
            if let Some(transform) = fixture_node.transform {
                assert_style(&style, "transform-origin", "0 0");
                assert_style(&style, "transform", &fixture_transform_css(transform));
            } else {
                assert_eq!(style.get_property_value("transform").unwrap(), "");
                assert_eq!(style.get_property_value("transform-origin").unwrap(), "");
            }
            if let Some(opacity) = fixture_node.opacity {
                assert_style(&style, "opacity", &opacity.to_string());
            } else {
                assert_eq!(style.get_property_value("opacity").unwrap(), "");
            }
            if let Some(visibility) = fixture_node.visibility {
                assert_style(&style, "visibility", fixture_visibility_css(visibility));
            } else {
                assert_eq!(style.get_property_value("visibility").unwrap(), "");
            }
            if let Some(z_order) = fixture_node.z_order {
                assert_style(&style, "z-index", &z_order.to_string());
            } else {
                assert_eq!(style.get_property_value("z-index").unwrap(), "");
            }
            if !fixture_node.background_layers.is_empty() {
                assert_background_layers_are_projected(
                    &style,
                    &fixture_node.background_layers,
                    &self.resource_urls,
                );
            } else if let Some(gradient) = &fixture_node.linear_gradient {
                assert_style(
                    &style,
                    "background-image",
                    &fixture_linear_gradient_css(gradient),
                );
                assert_background_layer_is_projected(&style, fixture_node.background_layer);
            } else if let Some(gradient) = &fixture_node.radial_gradient {
                assert_style(
                    &style,
                    "background-image",
                    &fixture_radial_gradient_css(gradient),
                );
                assert_background_layer_is_projected(&style, fixture_node.background_layer);
            } else if let Some(gradient) = &fixture_node.conic_gradient {
                assert_style(
                    &style,
                    "background-image",
                    &fixture_conic_gradient_css(gradient),
                );
                assert_background_layer_is_projected(&style, fixture_node.background_layer);
            } else {
                assert_eq!(style.get_property_value("background-image").unwrap(), "");
            }

            let actual_parent = node.parent_element().unwrap();
            match fixture_node.parent {
                Some(parent) => {
                    let parent = parent.to_string();
                    assert_eq!(
                        actual_parent.get_attribute("data-whisker-node").as_deref(),
                        Some(parent.as_str()),
                        "fixture node {} was attached to the wrong DOM parent",
                        fixture_node.id
                    );
                }
                None => assert!(
                    actual_parent.has_attribute("data-whisker-conformance-root"),
                    "fixture root node {} was not attached to the Host surface",
                    fixture_node.id
                ),
            }
        }

        let document = web_sys::window().unwrap().document().unwrap();
        let bounds = self.root.get_bounding_client_rect();
        for sample in samples {
            let hit_nodes = document
                .elements_from_point(
                    (bounds.left() + f64::from(sample.point[0])) as f32,
                    (bounds.top() + f64::from(sample.point[1])) as f32,
                )
                .iter()
                .filter_map(|value| {
                    value
                        .dyn_into::<web_sys::Element>()
                        .ok()?
                        .get_attribute("data-whisker-node")
                })
                .collect::<Vec<_>>();
            let opaque_hit = hit_nodes.iter().find(|id| {
                expected.iter().any(|node| {
                    node.id.to_string() == id.as_str()
                        && fixture_node_paints_at(
                            node,
                            sample.point,
                            &sample.color,
                            &self.resource_dimensions,
                        )
                })
            });
            match &sample.color {
                ColorFixture::Named { value } if value == "transparent" => assert!(
                    opaque_hit.is_none(),
                    "transparent sample unexpectedly hit opaque scene stack {hit_nodes:?}"
                ),
                ColorFixture::Srgba { .. } => {
                    let expected_node = expected
                        .iter()
                        .find(|node| {
                            node.background == sample.color
                                && fixture_rect_contains(node.rect, sample.point)
                        })
                        .or_else(|| {
                            expected.iter().find(|node| {
                                node.background_layers.iter().any(|layer| {
                                    matches!(layer.image, BackgroundImageFixture::Resource(_))
                                }) && fixture_node_paints_at(
                                    node,
                                    sample.point,
                                    &sample.color,
                                    &self.resource_dimensions,
                                )
                            })
                        })
                        .or_else(|| expected.iter().rev().find(|node| node.opacity.is_some()))
                        .or_else(|| {
                            expected.iter().find(|node| {
                                !node.background_layers.is_empty()
                                    || node.linear_gradient.is_some()
                                    || node.radial_gradient.is_some()
                                    || node.conic_gradient.is_some()
                            })
                        })
                        .expect("sRGBA sample requires an opacity or gradient source node");
                    let expected_node_id = expected_node.id.to_string();
                    if expected_node.pointer_events
                        == whisker_host_conformance::PointerEventsFixture::None
                    {
                        assert!(
                            !hit_nodes.iter().any(|id| id == &expected_node_id),
                            "pointer-events:none node {} remained in the DOM hit-test stack",
                            expected_node.id
                        );
                    } else {
                        assert_eq!(
                            opaque_hit.map(String::as_str),
                            Some(expected_node_id.as_str()),
                            "sRGBA sample at {:?} did not hit its expected source node",
                            sample.point
                        );
                    }
                }
                expected_color => {
                    let expected_node = expected
                        .iter()
                        .find(|node| node.background == *expected_color)
                        .or_else(|| {
                            expected.iter().find(|node| {
                                node.background_layers.iter().any(|layer| {
                                    matches!(layer.image, BackgroundImageFixture::Resource(_))
                                }) && fixture_node_paints_at(
                                    node,
                                    sample.point,
                                    &sample.color,
                                    &self.resource_dimensions,
                                )
                            })
                        })
                        .or_else(|| {
                            expected.iter().find(|node| {
                                !node.background_layers.is_empty()
                                    || node.linear_gradient.is_some()
                                    || node.radial_gradient.is_some()
                                    || node.conic_gradient.is_some()
                            })
                        })
                        .unwrap_or_else(|| {
                            panic!("sample color {expected_color:?} has no matching scene node")
                        })
                        .id
                        .to_string();
                    assert_eq!(
                        opaque_hit.map(String::as_str),
                        Some(expected_node.as_str()),
                        "paint sample at {:?} did not hit its expected scene node",
                        sample.point
                    );
                }
            }
        }
    }

    fn node(&self, id: u64) -> web_sys::Element {
        self.root
            .query_selector(&format!("[data-whisker-node='{id}']"))
            .unwrap()
            .unwrap_or_else(|| panic!("fixture DOM node {id}"))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.root.remove();
    }
}

#[wasm_bindgen_test]
async fn every_shared_paint_fixture_reaches_the_production_dom_sink() {
    let manifest: Manifest = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(manifest.schema, SCHEMA_VERSION);
    let mut count = 0;
    for entry in manifest.cases {
        let json = fixture(&entry.fixture.to_string_lossy());
        let scenario: Scenario = serde_json::from_str(json).unwrap();
        assert_eq!(scenario.schema, SCHEMA_VERSION);
        assert_eq!(scenario.id, entry.id);
        if !scenario.test.commands.iter().any(|command| {
            matches!(
                command,
                Command::PresentBox { .. }
                    | Command::PresentScene { .. }
                    | Command::MeasureText { .. }
            )
        }) {
            continue;
        }
        let mut driver = Driver::new();
        driver.execute(&scenario.test).await;
        if let Some(reference) = &scenario.reference {
            let mut driver = Driver::new();
            driver.execute(reference).await;
        }
        count += 1;
    }
    assert!(count > 0);
}

fn fixture(path: &str) -> &'static str {
    match path {
        "core/resource-raster-lifecycle.json" => {
            include_str!("../../../../tests/host-conformance/core/resource-raster-lifecycle.json")
        }
        "core/image-rendering-pixelated.json" => {
            include_str!("../../../../tests/host-conformance/core/image-rendering-pixelated.json")
        }
        "core/background-layer-geometry-symmetry.json" => include_str!(
            "../../../../tests/host-conformance/core/background-layer-geometry-symmetry.json"
        ),
        "wpt/css/CSS2/backgrounds/background-color-129.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/backgrounds/background-color-129.json"
        ),
        "wpt/css/css-images/linear-gradient-1.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-images/linear-gradient-1.json"
        ),
        "wpt/css/css-images/radial-gradient-container-relative-units-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-images/radial-gradient-container-relative-units-001.json"
        ),
        "wpt/css/css-images/conic-gradient-angle.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-images/conic-gradient-angle.json"
        ),
        "wpt/css/css-backgrounds/background-size-009.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-size-009.json"
        ),
        "wpt/css/css-backgrounds/background-size-intrinsic-auto.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-size-intrinsic-auto.json"
        ),
        "wpt/css/css-backgrounds/background-size-auto-round-aspect-ratio.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-size-auto-round-aspect-ratio.json"
        ),
        "wpt/css/css-backgrounds/background-size-contain-001-intrinsic.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-size-contain-001-intrinsic.json"
        ),
        "wpt/css/css-backgrounds/background-size-cover-001-intrinsic.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-size-cover-001-intrinsic.json"
        ),
        "wpt/css/css-backgrounds/background-position-three-four-values.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-position-three-four-values.json"
        ),
        "wpt/css/css-backgrounds/css3-background-origin-border-box.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/css3-background-origin-border-box.json"
        ),
        "wpt/css/css-backgrounds/clip-padding-box-with-size.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/clip-padding-box-with-size.json"
        ),
        "wpt/css/css-backgrounds/background-repeat/background-repeat-repeat-x.json" => {
            include_str!(
                "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat/background-repeat-repeat-x.json"
            )
        }
        "wpt/css/css-backgrounds/background-repeat/background-repeat-repeat-y.json" => {
            include_str!(
                "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat/background-repeat-repeat-y.json"
            )
        }
        "wpt/css/css-backgrounds/background-repeat-space.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat-space.json"
        ),
        "wpt/css/css-backgrounds/background-repeat-space-single.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat-space-single.json"
        ),
        "wpt/css/css-backgrounds/background-repeat-round-x.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat-round-x.json"
        ),
        "wpt/css/css-backgrounds/background-repeat-round-y.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat-round-y.json"
        ),
        "wpt/css/css-backgrounds/background-repeat-round-position.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-repeat-round-position.json"
        ),
        "wpt/css/css-backgrounds/background-origin-content-box.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-origin-content-box.json"
        ),
        "wpt/css/css-backgrounds/background-clip-content-box.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-clip-content-box.json"
        ),
        "wpt/css/css-backgrounds/background-clip-border-area-background-geometry.json" => {
            include_str!(
                "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-clip-border-area-background-geometry.json"
            )
        }
        "wpt/css/css-backgrounds/background-layer-stacking.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-layer-stacking.json"
        ),
        "wpt/css/css-backgrounds/background-resource-image.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-resource-image.json"
        ),
        "wpt/css/css-backgrounds/border-radius-sum-of-radii-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/border-radius-sum-of-radii-001.json"
        ),
        "wpt/css/css-backgrounds/border-radius-004.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/border-radius-004.json"
        ),
        "wpt/css/css-backgrounds/border-radius-overflow-hidden.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/border-radius-overflow-hidden.json"
        ),
        "wpt/css/css-backgrounds/border-radius-clipping-with-transform-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/border-radius-clipping-with-transform-001.json"
        ),
        "wpt/css/css-overflow/clip-002.json" => {
            include_str!("../../../../tests/host-conformance/wpt/css/css-overflow/clip-002.json")
        }
        "wpt/css/css-overflow/clip-002-vertical.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-overflow/clip-002-vertical.json"
        ),
        "wpt/css/css-transforms/transform-matrix-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-transforms/transform-matrix-001.json"
        ),
        "core/transform-local-origin.json" => {
            include_str!("../../../../tests/host-conformance/core/transform-local-origin.json")
        }
        "core/transform-parent-composition.json" => include_str!(
            "../../../../tests/host-conformance/core/transform-parent-composition.json"
        ),
        "core/transform-projective-plane.json" => {
            include_str!("../../../../tests/host-conformance/core/transform-projective-plane.json")
        }
        "core/transform-perspective-current-node.json" => include_str!(
            "../../../../tests/host-conformance/core/transform-perspective-current-node.json"
        ),
        "core/transform-motion-path-line.json" => {
            include_str!("../../../../tests/host-conformance/core/transform-motion-path-line.json")
        }
        "core/transform-motion-path-curves.json" => include_str!(
            "../../../../tests/host-conformance/core/transform-motion-path-curves.json"
        ),
        "core/transform-motion-path-ellipses.json" => include_str!(
            "../../../../tests/host-conformance/core/transform-motion-path-ellipses.json"
        ),
        "core/transform-motion-path-inset.json" => {
            include_str!("../../../../tests/host-conformance/core/transform-motion-path-inset.json")
        }
        "core/transform-motion-path-arcs.json" => {
            include_str!("../../../../tests/host-conformance/core/transform-motion-path-arcs.json")
        }
        "wpt/css/css-color/t32-opacity-basic-0.6-a.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-color/t32-opacity-basic-0.6-a.json"
        ),
        "wpt/css/CSS2/visufx/visibility-004.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/visufx/visibility-004.json"
        ),
        "wpt/css/CSS2/zindex/z-index-003.json" => {
            include_str!("../../../../tests/host-conformance/wpt/css/CSS2/zindex/z-index-003.json")
        }
        "wpt/css/CSS2/borders/border-top-003.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-003.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-001.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-002.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-002.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-blur-definition-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-blur-definition-001.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-inset-without-border-radius.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-inset-without-border-radius.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-inset-spread.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-inset-spread.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-inset-blur-definition.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-inset-blur-definition.json"
        ),
        "wpt/css/css-backgrounds/box-shadow-multiple-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/box-shadow-multiple-001.json"
        ),
        "wpt/css/css-masking/clip-path-inset-round-rendering.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-masking/clip-path-inset-round-rendering.json"
        ),
        "wpt/css/css-masking/clip-path-circle-002.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-masking/clip-path-circle-002.json"
        ),
        "wpt/css/css-masking/clip-path-ellipse-002.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-masking/clip-path-ellipse-002.json"
        ),
        "wpt/css/css-masking/clip-path-path-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-masking/clip-path-path-001.json"
        ),
        "wpt/css/css-masking/clip-path-path-002.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-masking/clip-path-path-002.json"
        ),
        "wpt/css/CSS2/borders/border-right-003.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-right-003.json"
        ),
        "wpt/css/CSS2/borders/border-bottom-003.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-bottom-003.json"
        ),
        "wpt/css/CSS2/borders/border-left-003.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-left-003.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-003.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-003.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-004.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-004.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-006.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-006.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-007.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-007.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-008.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-008.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-009.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-009.json"
        ),
        "wpt/css/CSS2/borders/border-top-style-010.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/CSS2/borders/border-top-style-010.json"
        ),
        "core/text-measure-basic.json" => {
            include_str!("../../../../tests/host-conformance/core/text-measure-basic.json")
        }
        "core/pointer-input-basic.json" => {
            include_str!("../../../../tests/host-conformance/core/pointer-input-basic.json")
        }
        "core/pointer-style-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/pointer-style-lynx.json")
        }
        "core/pointer-cursor-fidelity.json" => {
            include_str!("../../../../tests/host-conformance/core/pointer-cursor-fidelity.json")
        }
        "core/backdrop-filter-blur.json" => {
            include_str!("../../../../tests/host-conformance/core/backdrop-filter-blur.json")
        }
        "core/text-shadow-single.json" => {
            include_str!("../../../../tests/host-conformance/core/text-shadow-single.json")
        }
        "core/text-decoration-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/text-decoration-lynx.json")
        }
        "core/text-align-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/text-align-lynx.json")
        }
        "core/text-indent-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/text-indent-lynx.json")
        }
        "core/text-wrap-overflow-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/text-wrap-overflow-lynx.json")
        }
        "core/text-font-features-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/text-font-features-lynx.json")
        }
        "core/text-basic-style-lynx.json" => {
            include_str!("../../../../tests/host-conformance/core/text-basic-style-lynx.json")
        }
        _ => panic!("manifest fixture is not embedded in the Web test: {path}"),
    }
}

fn background_repeat_checkpoint(nodes: &[SceneNodeFixture]) -> Option<&'static str> {
    nodes.iter().find_map(|node| {
        match (
            node.background_layer.repeat_x,
            node.background_layer.repeat_y,
        ) {
            (ImageRepeatFixture::Round, ImageRepeatFixture::Round)
                if node.background_layer.position != [LengthPercentageFixture::default(); 2] =>
            {
                Some("paint.background-layers.repeat-round-position")
            }
            (ImageRepeatFixture::Round, ImageRepeatFixture::NoRepeat) => {
                Some("paint.background-layers.repeat-round-x")
            }
            (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::Round) => {
                Some("paint.background-layers.repeat-round-y")
            }
            (ImageRepeatFixture::Repeat, ImageRepeatFixture::NoRepeat) => {
                Some("paint.background-layers.repeat-x")
            }
            (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::Repeat) => {
                Some("paint.background-layers.repeat-y")
            }
            (ImageRepeatFixture::Space, ImageRepeatFixture::Space)
                if node.background_layer.position != [LengthPercentageFixture::default(); 2] =>
            {
                Some("paint.background-layers.repeat-space-single")
            }
            (ImageRepeatFixture::Space, ImageRepeatFixture::Space) => {
                Some("paint.background-layers.repeat-space")
            }
            _ => None,
        }
    })
}

fn packet(
    revision: u64,
    rect: [f32; 4],
    background: &ColorFixture,
    border: Option<&whisker_host_conformance::BorderFixture>,
) -> FramePacket {
    let surface = SurfaceId::new(1).unwrap();
    let node = NodeId::new(1).unwrap();
    let view = ElementRegistry::standard()
        .registration_for_builtin(whisker::ElementTag::View)
        .unwrap()
        .element_type;
    FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface,
            scene_epoch: 1,
            frame_id: revision,
            base_revision: 0,
            target_revision: revision,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![
            Operation::CreateNode {
                node,
                element_type: view,
            },
            Operation::SetLayout {
                node,
                geometry: LayoutGeometry {
                    border_box: LayoutRect {
                        x: rect[0],
                        y: rect[1],
                        width: rect[2],
                        height: rect[3],
                    },
                    content_box: LayoutRect {
                        width: rect[2],
                        height: rect[3],
                        ..LayoutRect::default()
                    },
                },
            },
            Operation::SetBoxPaint {
                node,
                paint: box_paint(background, border),
            },
        ],
    }
}

fn scene_packet(revision: u64, nodes: &[SceneNodeFixture]) -> FramePacket {
    let surface = SurfaceId::new(1).unwrap();
    let registry = ElementRegistry::standard();
    let mut operations = Vec::with_capacity(nodes.len() * 5);
    for fixture_node in nodes {
        let node = fixture_node_id(fixture_node.id);
        operations.extend([
            Operation::CreateNode {
                node,
                element_type: registry
                    .registration_for_builtin(if fixture_node.text.is_some() {
                        whisker::ElementTag::Text
                    } else {
                        whisker::ElementTag::View
                    })
                    .unwrap()
                    .element_type,
            },
            Operation::SetLayout {
                node,
                geometry: LayoutGeometry {
                    border_box: LayoutRect {
                        x: fixture_node.rect[0],
                        y: fixture_node.rect[1],
                        width: fixture_node.rect[2],
                        height: fixture_node.rect[3],
                    },
                    content_box: {
                        let content_box = fixture_node.resolved_content_box();
                        LayoutRect {
                            x: content_box[0],
                            y: content_box[1],
                            width: content_box[2],
                            height: content_box[3],
                        }
                    },
                },
            },
            Operation::SetBoxPaint {
                node,
                paint: box_paint(&fixture_node.background, fixture_node.border.as_ref()),
            },
            Operation::SetClip {
                node,
                clip: BoxClip {
                    horizontal: overflow_clip(fixture_node.clip.horizontal),
                    vertical: overflow_clip(fixture_node.clip.vertical),
                },
            },
        ]);
        if let Some(text) = &fixture_node.text {
            operations.push(Operation::SetText {
                node,
                content: fixture_text_content(text),
            });
        }
        if let Some(transform) = fixture_node.transform {
            operations.push(Operation::SetTransform {
                node,
                transform: Transform(transform),
            });
        }
        if !fixture_node.box_shadows.is_empty()
            || fixture_node.clip_path.is_some()
            || fixture_node.backdrop_blur.is_some()
            || fixture_node.image_rendering != whisker_host_conformance::ImageRenderingFixture::Auto
        {
            operations.push(Operation::SetVisualEffects {
                node,
                effects: whisker_protocol::VisualEffects {
                    box_shadows: fixture_node
                        .box_shadows
                        .iter()
                        .map(|shadow| whisker_protocol::BoxShadow {
                            offset_x: shadow.offset[0],
                            offset_y: shadow.offset[1],
                            blur_radius: shadow.blur_radius,
                            spread_radius: shadow.spread_radius,
                            color: color(&shadow.color),
                            inset: shadow.inset,
                        })
                        .collect(),
                    clip_path: fixture_node.clip_path.as_ref().map(clip_path_protocol),
                    backdrop_blur: fixture_node.backdrop_blur,
                    image_rendering: match fixture_node.image_rendering {
                        whisker_host_conformance::ImageRenderingFixture::Auto => {
                            whisker_protocol::ImageRendering::Auto
                        }
                        whisker_host_conformance::ImageRenderingFixture::Pixelated => {
                            whisker_protocol::ImageRendering::Pixelated
                        }
                        whisker_host_conformance::ImageRenderingFixture::CrispEdges => {
                            whisker_protocol::ImageRendering::CrispEdges
                        }
                    },
                    ..Default::default()
                },
            });
        }
        if let Some(opacity) = fixture_node.opacity {
            operations.push(Operation::SetOpacity { node, opacity });
        }
        if let Some(visibility) = fixture_node.visibility {
            operations.push(Operation::SetVisibility {
                node,
                visibility: protocol_visibility(visibility),
            });
        }
        if let Some(z_order) = fixture_node.z_order {
            operations.push(Operation::SetZOrder { node, z_order });
        }
        operations.push(Operation::SetCursor {
            node,
            cursor: whisker_protocol::Cursor {
                resources: Vec::new(),
                fallback: match fixture_node.cursor {
                    whisker_host_conformance::CursorFixture::Auto => {
                        whisker_protocol::CursorKeyword::Auto
                    }
                    whisker_host_conformance::CursorFixture::Pointer => {
                        whisker_protocol::CursorKeyword::Pointer
                    }
                    whisker_host_conformance::CursorFixture::Text => {
                        whisker_protocol::CursorKeyword::Text
                    }
                    whisker_host_conformance::CursorFixture::Grab => {
                        whisker_protocol::CursorKeyword::Grab
                    }
                    whisker_host_conformance::CursorFixture::None => {
                        whisker_protocol::CursorKeyword::None
                    }
                },
            },
        });
        operations.push(Operation::SetHitTest {
            node,
            behavior: match fixture_node.pointer_events {
                whisker_host_conformance::PointerEventsFixture::Auto => {
                    whisker_protocol::HitTestBehavior::Auto
                }
                whisker_host_conformance::PointerEventsFixture::None => {
                    whisker_protocol::HitTestBehavior::None
                }
            },
        });
        if !fixture_node.background_layers.is_empty() {
            operations.push(Operation::SetBackgroundLayers {
                node,
                layers: fixture_node
                    .background_layers
                    .iter()
                    .map(background_paint_layer)
                    .collect(),
            });
        } else if let Some(gradient) = &fixture_node.linear_gradient {
            operations.push(Operation::SetBackgroundLayers {
                node,
                layers: vec![with_background_geometry(
                    linear_gradient_layer(gradient),
                    fixture_node.background_layer,
                )],
            });
        } else if let Some(gradient) = &fixture_node.radial_gradient {
            operations.push(Operation::SetBackgroundLayers {
                node,
                layers: vec![with_background_geometry(
                    radial_gradient_layer(gradient),
                    fixture_node.background_layer,
                )],
            });
        } else if let Some(gradient) = &fixture_node.conic_gradient {
            operations.push(Operation::SetBackgroundLayers {
                node,
                layers: vec![with_background_geometry(
                    conic_gradient_layer(gradient),
                    fixture_node.background_layer,
                )],
            });
        }
    }
    for (node_index, fixture_node) in nodes.iter().enumerate() {
        if let Some(parent) = fixture_node.parent {
            let index = nodes[..node_index]
                .iter()
                .filter(|candidate| candidate.parent == Some(parent))
                .count() as u32;
            operations.push(Operation::InsertChild {
                parent: fixture_node_id(parent),
                child: fixture_node_id(fixture_node.id),
                index,
            });
        }
    }
    FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface,
            scene_epoch: 1,
            frame_id: revision,
            base_revision: 0,
            target_revision: revision,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations,
    }
}

fn fixture_node_id(id: u64) -> NodeId {
    NodeId::new(id).expect("fixture node ids are validated as non-zero")
}

fn overflow_clip(value: OverflowClipFixture) -> OverflowClip {
    match value {
        OverflowClipFixture::Visible => OverflowClip::Visible,
        OverflowClipFixture::Hidden => OverflowClip::Hidden,
    }
}

fn linear_gradient_layer(gradient: &LinearGradientFixture) -> BackgroundLayer {
    BackgroundLayer {
        image: PaintImage::LinearGradient {
            angle_degrees: gradient.angle_degrees,
            repeating: gradient.repeating,
            stops: gradient
                .stops
                .iter()
                .map(|stop| GradientStop {
                    color: color(&stop.color),
                    position: Some(PaintCoordinate {
                        length: 0.0,
                        fraction: stop.position,
                    }),
                })
                .collect(),
        },
        position: PaintPosition::default(),
        size: BackgroundSize::Auto,
        repeat_x: ImageRepeat::Repeat,
        repeat_y: ImageRepeat::Repeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    }
}

fn background_paint_layer(layer: &BackgroundPaintLayerFixture) -> BackgroundLayer {
    let image_layer = match &layer.image {
        BackgroundImageFixture::Resource(id) => BackgroundLayer {
            image: PaintImage::Resource(ResourceId::new(*id).unwrap()),
            position: PaintPosition::default(),
            size: BackgroundSize::Auto,
            repeat_x: ImageRepeat::Repeat,
            repeat_y: ImageRepeat::Repeat,
            origin: PaintBox::Padding,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
        },
        BackgroundImageFixture::LinearGradient(gradient) => linear_gradient_layer(gradient),
        BackgroundImageFixture::RadialGradient(gradient) => radial_gradient_layer(gradient),
        BackgroundImageFixture::ConicGradient(gradient) => conic_gradient_layer(gradient),
    };
    with_background_geometry(image_layer, layer.geometry)
}

fn radial_gradient_layer(gradient: &RadialGradientFixture) -> BackgroundLayer {
    BackgroundLayer {
        image: PaintImage::RadialGradient {
            shape: RadialGradientShape::Ellipse,
            extent: RadialGradientExtent::Explicit,
            center: PaintPosition {
                x: PaintCoordinate {
                    length: gradient.center[0],
                    fraction: 0.0,
                },
                y: PaintCoordinate {
                    length: gradient.center[1],
                    fraction: 0.0,
                },
            },
            radii: Some((
                PaintLengthPercentage {
                    length: gradient.radii[0],
                    fraction: 0.0,
                },
                PaintLengthPercentage {
                    length: gradient.radii[1],
                    fraction: 0.0,
                },
            )),
            repeating: false,
            stops: gradient
                .stops
                .iter()
                .map(|stop| GradientStop {
                    color: color(&stop.color),
                    position: Some(PaintCoordinate {
                        length: 0.0,
                        fraction: stop.position,
                    }),
                })
                .collect(),
        },
        position: PaintPosition::default(),
        size: BackgroundSize::Auto,
        repeat_x: ImageRepeat::Repeat,
        repeat_y: ImageRepeat::Repeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    }
}

fn conic_gradient_layer(gradient: &ConicGradientFixture) -> BackgroundLayer {
    BackgroundLayer {
        image: PaintImage::ConicGradient {
            from_degrees: gradient.from_degrees,
            center: PaintPosition {
                x: PaintCoordinate {
                    length: gradient.center[0],
                    fraction: 0.0,
                },
                y: PaintCoordinate {
                    length: gradient.center[1],
                    fraction: 0.0,
                },
            },
            repeating: false,
            stops: gradient
                .stops
                .iter()
                .map(|stop| GradientStop {
                    color: color(&stop.color),
                    position: Some(PaintCoordinate {
                        length: 0.0,
                        fraction: stop.position,
                    }),
                })
                .collect(),
        },
        position: PaintPosition::default(),
        size: BackgroundSize::Auto,
        repeat_x: ImageRepeat::Repeat,
        repeat_y: ImageRepeat::Repeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    }
}

fn with_background_geometry(
    mut layer: BackgroundLayer,
    geometry: BackgroundLayerFixture,
) -> BackgroundLayer {
    layer.position = PaintPosition {
        x: paint_coordinate(geometry.position[0]),
        y: paint_coordinate(geometry.position[1]),
    };
    layer.size = match geometry.size {
        BackgroundSizeFixture::ExplicitPair(size) => BackgroundSize::Explicit {
            width: Some(paint_length_percentage(size[0])),
            height: Some(paint_length_percentage(size[1])),
        },
        BackgroundSizeFixture::ExplicitAxes { width, height } => BackgroundSize::Explicit {
            width: width.map(paint_length_percentage),
            height: height.map(paint_length_percentage),
        },
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Auto) => BackgroundSize::Auto,
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Cover) => {
            BackgroundSize::Cover
        }
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Contain) => {
            BackgroundSize::Contain
        }
    };
    layer.repeat_x = image_repeat(geometry.repeat_x);
    layer.repeat_y = image_repeat(geometry.repeat_y);
    layer.origin = background_box(geometry.origin);
    layer.clip = background_box(geometry.clip);
    layer
}

fn paint_coordinate(value: LengthPercentageFixture) -> PaintCoordinate {
    PaintCoordinate {
        length: value.length,
        fraction: value.fraction,
    }
}

fn clip_path_protocol(value: &ClipPathFixture) -> (PaintBox, ClipShape) {
    let reference_box = match value.reference_box {
        ClipReferenceBoxFixture::Border => PaintBox::Border,
        ClipReferenceBoxFixture::Padding => PaintBox::Padding,
        ClipReferenceBoxFixture::Content => PaintBox::Content,
    };
    let shape = match &value.shape {
        ClipShapeFixture::Inset { edges, radii } => {
            let radius = |value: CornerRadiusFixture| PaintCornerRadius {
                horizontal: PaintLengthPercentage {
                    length: value.horizontal(),
                    fraction: 0.0,
                },
                vertical: PaintLengthPercentage {
                    length: value.vertical(),
                    fraction: 0.0,
                },
            };
            ClipShape::Inset {
                edges: PaintEdges {
                    top: paint_coordinate(edges[0]),
                    right: paint_coordinate(edges[1]),
                    bottom: paint_coordinate(edges[2]),
                    left: paint_coordinate(edges[3]),
                },
                radii: PaintCorners {
                    top_left: radius(radii[0]),
                    top_right: radius(radii[1]),
                    bottom_right: radius(radii[2]),
                    bottom_left: radius(radii[3]),
                },
            }
        }
        ClipShapeFixture::Circle { radius, center } => ClipShape::Circle {
            radius: paint_length_percentage(*radius),
            center: whisker_protocol::PaintPosition {
                x: paint_coordinate(center[0]),
                y: paint_coordinate(center[1]),
            },
        },
        ClipShapeFixture::Ellipse { radii, center } => ClipShape::Ellipse {
            radius_x: paint_length_percentage(radii[0]),
            radius_y: paint_length_percentage(radii[1]),
            center: whisker_protocol::PaintPosition {
                x: paint_coordinate(center[0]),
                y: paint_coordinate(center[1]),
            },
        },
        ClipShapeFixture::Path {
            fill_rule,
            commands,
        } => ClipShape::Path {
            fill_rule: match fill_rule {
                FillRuleFixture::NonZero => FillRule::NonZero,
                FillRuleFixture::EvenOdd => FillRule::EvenOdd,
            },
            commands: commands.iter().map(path_command_protocol).collect(),
        },
    };
    (reference_box, shape)
}

fn path_command_protocol(value: &PathCommandFixture) -> PathCommand {
    let position = |point: &[LengthPercentageFixture; 2]| PaintPosition {
        x: paint_coordinate(point[0]),
        y: paint_coordinate(point[1]),
    };
    match value {
        PathCommandFixture::MoveTo { point } => PathCommand::MoveTo(position(point)),
        PathCommandFixture::LineTo { point } => PathCommand::LineTo(position(point)),
        PathCommandFixture::QuadraticTo { control, end } => PathCommand::QuadraticTo {
            control: position(control),
            end: position(end),
        },
        PathCommandFixture::CubicTo {
            control_1,
            control_2,
            end,
        } => PathCommand::CubicTo {
            control_1: position(control_1),
            control_2: position(control_2),
            end: position(end),
        },
        PathCommandFixture::Close => PathCommand::Close,
    }
}

fn paint_length_percentage(value: LengthPercentageFixture) -> PaintLengthPercentage {
    PaintLengthPercentage {
        length: value.length,
        fraction: value.fraction,
    }
}

fn image_repeat(value: ImageRepeatFixture) -> ImageRepeat {
    match value {
        ImageRepeatFixture::Repeat => ImageRepeat::Repeat,
        ImageRepeatFixture::NoRepeat => ImageRepeat::NoRepeat,
        ImageRepeatFixture::Space => ImageRepeat::Space,
        ImageRepeatFixture::Round => ImageRepeat::Round,
    }
}

fn background_box(value: BackgroundBoxFixture) -> PaintBox {
    match value {
        BackgroundBoxFixture::Border => PaintBox::Border,
        BackgroundBoxFixture::Padding => PaintBox::Padding,
        BackgroundBoxFixture::Content => PaintBox::Content,
        BackgroundBoxFixture::BorderArea => PaintBox::BorderArea,
    }
}

fn box_paint(
    background: &ColorFixture,
    border: Option<&whisker_host_conformance::BorderFixture>,
) -> BoxPaint {
    let zero = PaintLengthPercentage::default();
    let Some(border) = border else {
        return BoxPaint {
            background_color: color(background),
            border_widths: edges(zero),
            border_colors: edges(PaintColor::default()),
            border_styles: edges(BorderLineStyle::None),
            border_radii: corners(PaintCornerRadius::circular(zero)),
        };
    };
    let widths = border.widths.map(|length| PaintLengthPercentage {
        length,
        fraction: 0.0,
    });
    let radii = border.radii.map(|radius| PaintCornerRadius {
        horizontal: PaintLengthPercentage {
            length: radius.horizontal(),
            fraction: 0.0,
        },
        vertical: PaintLengthPercentage {
            length: radius.vertical(),
            fraction: 0.0,
        },
    });
    BoxPaint {
        background_color: color(background),
        border_widths: PaintEdges {
            top: widths[0],
            right: widths[1],
            bottom: widths[2],
            left: widths[3],
        },
        border_colors: PaintEdges {
            top: color(&border.colors[0]),
            right: color(&border.colors[1]),
            bottom: color(&border.colors[2]),
            left: color(&border.colors[3]),
        },
        border_styles: PaintEdges {
            top: style(border.styles[0]),
            right: style(border.styles[1]),
            bottom: style(border.styles[2]),
            left: style(border.styles[3]),
        },
        border_radii: PaintCorners {
            top_left: radii[0],
            top_right: radii[1],
            bottom_right: radii[2],
            bottom_left: radii[3],
        },
    }
}

fn color(value: &ColorFixture) -> PaintColor {
    match value {
        ColorFixture::Named { value } => PaintColor::Named(value.clone()),
        ColorFixture::Srgba {
            red,
            green,
            blue,
            alpha,
        } => PaintColor::Srgba {
            red: *red,
            green: *green,
            blue: *blue,
            alpha: *alpha,
        },
    }
}

fn fixture_alignment(
    value: whisker_host_conformance::TextAlignmentFixture,
) -> whisker_protocol::MeasureTextAlignment {
    use whisker_host_conformance::TextAlignmentFixture as Fixture;
    match value {
        Fixture::Start => whisker_protocol::MeasureTextAlignment::Start,
        Fixture::End => whisker_protocol::MeasureTextAlignment::End,
        Fixture::Left => whisker_protocol::MeasureTextAlignment::Left,
        Fixture::Right => whisker_protocol::MeasureTextAlignment::Right,
        Fixture::Center => whisker_protocol::MeasureTextAlignment::Center,
    }
}

fn fixture_measure_font_style(
    value: whisker_host_conformance::FontStyleFixture,
) -> MeasureFontStyle {
    match value {
        whisker_host_conformance::FontStyleFixture::Normal => MeasureFontStyle::Normal,
        whisker_host_conformance::FontStyleFixture::Italic => MeasureFontStyle::Italic,
        whisker_host_conformance::FontStyleFixture::Oblique => MeasureFontStyle::Oblique,
    }
}

fn fixture_text_content(text: &whisker_host_conformance::TextFixture) -> TextContent {
    use whisker_host_conformance::{TextOverflowFixture, WhiteSpaceFixture, WordBreakFixture};
    TextContent {
        payload: TextMeasurePayload {
            text: text.value.clone(),
            style: TextMeasureStyle {
                font_families: text
                    .font_families
                    .iter()
                    .map(|family| {
                        if family == "system" {
                            MeasureFontFamily::System
                        } else {
                            MeasureFontFamily::Named(family.clone())
                        }
                    })
                    .collect(),
                font_size: text.font_size,
                font_weight: text.font_weight,
                font_style: fixture_measure_font_style(text.font_style),
                line_height: text
                    .line_height
                    .map_or(MeasureLineHeight::Normal, MeasureLineHeight::LogicalPixels),
                letter_spacing: text.letter_spacing,
                features: text
                    .font_features
                    .iter()
                    .map(|feature| whisker_protocol::FontFeature {
                        tag: fixture_font_tag(&feature.tag),
                        value: feature.value,
                    })
                    .collect(),
                variations: text
                    .font_variations
                    .iter()
                    .map(|variation| whisker_protocol::FontVariation {
                        tag: fixture_font_tag(&variation.tag),
                        value: variation.value,
                    })
                    .collect(),
                optical_sizing: match text.font_optical_sizing {
                    whisker_host_conformance::FontOpticalSizingFixture::Auto => {
                        whisker_protocol::FontOpticalSizing::Auto
                    }
                    whisker_host_conformance::FontOpticalSizingFixture::None => {
                        whisker_protocol::FontOpticalSizing::None
                    }
                },
                ..TextMeasureStyle::default()
            },
            locale: None,
            direction: MeasureTextDirection::Auto,
            alignment: fixture_alignment(text.alignment),
            indent: whisker_protocol::MeasureTextIndent {
                logical_pixels: text.indent.logical_pixels,
                percentage: text.indent.percentage,
            },
            wrap: match text.white_space {
                WhiteSpaceFixture::Normal => MeasureTextWrap::Wrap,
                WhiteSpaceFixture::NoWrap => MeasureTextWrap::NoWrap,
            },
            word_break: match text.word_break {
                WordBreakFixture::Normal => MeasureTextWordBreak::Normal,
                WordBreakFixture::BreakAll => MeasureTextWordBreak::BreakAll,
                WordBreakFixture::KeepAll => MeasureTextWordBreak::KeepAll,
            },
            max_lines: (text.max_lines > 0).then_some(text.max_lines),
            overflow: match text.overflow {
                TextOverflowFixture::Clip => MeasureTextOverflow::Clip,
                TextOverflowFixture::Ellipsis => MeasureTextOverflow::Ellipsis,
            },
        },
        paint: TextPaint {
            foreground: color(&text.color),
            decoration: text.decoration.as_ref().map_or_else(
                whisker_protocol::TextDecoration::default,
                |decoration| whisker_protocol::TextDecoration {
                    lines: whisker_protocol::TextDecorationLines {
                        underline: matches!(
                            decoration.line,
                            whisker_host_conformance::TextDecorationLineFixture::Underline
                        ),
                        overline: false,
                        line_through: matches!(
                            decoration.line,
                            whisker_host_conformance::TextDecorationLineFixture::LineThrough
                        ),
                    },
                    color: color(&decoration.color),
                    style: match decoration.style {
                        whisker_host_conformance::TextDecorationStyleFixture::Solid => {
                            whisker_protocol::TextDecorationStyle::Solid
                        }
                        whisker_host_conformance::TextDecorationStyleFixture::Double => {
                            whisker_protocol::TextDecorationStyle::Double
                        }
                        whisker_host_conformance::TextDecorationStyleFixture::Dotted => {
                            whisker_protocol::TextDecorationStyle::Dotted
                        }
                        whisker_host_conformance::TextDecorationStyleFixture::Dashed => {
                            whisker_protocol::TextDecorationStyle::Dashed
                        }
                        whisker_host_conformance::TextDecorationStyleFixture::Wavy => {
                            whisker_protocol::TextDecorationStyle::Wavy
                        }
                    },
                    thickness: whisker_protocol::TextDecorationThickness::Auto,
                },
            ),
            shadows: text
                .shadow
                .iter()
                .map(|shadow| TextShadow {
                    offset_x: shadow.offset[0],
                    offset_y: shadow.offset[1],
                    blur_radius: shadow.blur_radius,
                    color: color(&shadow.color),
                })
                .collect(),
            ..TextPaint::default()
        },
        prepared_content: None,
    }
}

fn fixture_font_tag(value: &str) -> whisker_protocol::FontTag {
    let bytes: [u8; 4] = value
        .as_bytes()
        .try_into()
        .expect("fixture schema validates four-byte ASCII tags");
    whisker_protocol::FontTag::new(bytes).expect("fixture schema validates printable tags")
}

fn fixture_font_families_css(values: &[String]) -> String {
    values
        .iter()
        .map(|family| {
            if family == "system" {
                "system-ui".to_owned()
            } else {
                format!("{family:?}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fixture_font_settings<T>(values: &[T], value: impl Fn(&T) -> String) -> String
where
    T: FixtureFontSetting,
{
    if values.is_empty() {
        return "normal".into();
    }
    values
        .iter()
        .map(|setting| format!("'{}' {}", setting.tag(), value(setting)))
        .collect::<Vec<_>>()
        .join(", ")
}

trait FixtureFontSetting {
    fn tag(&self) -> &str;
}

impl FixtureFontSetting for whisker_host_conformance::FontFeatureFixture {
    fn tag(&self) -> &str {
        &self.tag
    }
}

impl FixtureFontSetting for whisker_host_conformance::FontVariationFixture {
    fn tag(&self) -> &str {
        &self.tag
    }
}

fn style(value: whisker_host_conformance::BorderStyleFixture) -> BorderLineStyle {
    use whisker_host_conformance::BorderStyleFixture as Fixture;
    match value {
        Fixture::None => BorderLineStyle::None,
        Fixture::Hidden => BorderLineStyle::Hidden,
        Fixture::Solid => BorderLineStyle::Solid,
        Fixture::Dashed => BorderLineStyle::Dashed,
        Fixture::Dotted => BorderLineStyle::Dotted,
        Fixture::Double => BorderLineStyle::Double,
        Fixture::Groove => BorderLineStyle::Groove,
        Fixture::Ridge => BorderLineStyle::Ridge,
        Fixture::Inset => BorderLineStyle::Inset,
        Fixture::Outset => BorderLineStyle::Outset,
    }
}

fn edges<T: Clone>(value: T) -> PaintEdges<T> {
    PaintEdges {
        top: value.clone(),
        right: value.clone(),
        bottom: value.clone(),
        left: value,
    }
}

fn corners<T: Clone>(value: T) -> PaintCorners<T> {
    PaintCorners {
        top_left: value.clone(),
        top_right: value.clone(),
        bottom_right: value.clone(),
        bottom_left: value,
    }
}

fn set_style(element: &web_sys::Element, name: &str, value: &str) {
    element
        .dyn_ref::<web_sys::HtmlElement>()
        .unwrap()
        .style()
        .set_property(name, value)
        .unwrap();
}

fn assert_style(style: &web_sys::CssStyleDeclaration, property: &str, expected: &str) {
    let actual = style.get_property_value(property).unwrap();
    let document = web_sys::window().unwrap().document().unwrap();
    let probe = document.create_element("div").unwrap();
    let probe_style = probe.dyn_ref::<web_sys::HtmlElement>().unwrap().style();
    probe_style.set_property(property, expected).unwrap();
    let expected = probe_style.get_property_value(property).unwrap();
    assert_eq!(
        actual, expected,
        "production DOM projection for {property} did not match the fixture"
    );
}

fn assert_border_is_projected(
    style: &web_sys::CssStyleDeclaration,
    border: Option<&BorderFixture>,
) {
    let (widths, styles, radii) = border.map_or(
        (
            [0.0; 4],
            [BorderStyleFixture::None; 4],
            [CornerRadiusFixture::Circular(0.0); 4],
        ),
        |border| (border.widths, border.styles, border.radii),
    );
    for (index, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
        assert_style(
            style,
            &format!("border-{side}-width"),
            &fixture_px(widths[index]),
        );
        assert_style(
            style,
            &format!("border-{side}-color"),
            &border.map_or_else(
                || "rgba(0, 0, 0, 1)".to_owned(),
                |border| fixture_color_css(&border.colors[index]),
            ),
        );
        assert_style(
            style,
            &format!("border-{side}-style"),
            fixture_border_style_css(styles[index]),
        );
    }
    for (index, corner) in ["top-left", "top-right", "bottom-right", "bottom-left"]
        .iter()
        .enumerate()
    {
        let horizontal = fixture_px(radii[index].horizontal());
        let vertical = fixture_px(radii[index].vertical());
        assert_style(
            style,
            &format!("border-{corner}-radius"),
            &format!("{horizontal} {vertical}"),
        );
    }
}

fn fixture_px(value: f32) -> String {
    format!("{value}px")
}

fn fixture_clip_path_css(value: &ClipPathFixture) -> String {
    let reference_box = match value.reference_box {
        ClipReferenceBoxFixture::Border => "",
        ClipReferenceBoxFixture::Padding => "padding-box",
        ClipReferenceBoxFixture::Content => "content-box",
    };
    let coordinate = |value: LengthPercentageFixture| {
        if value.fraction == 0.0 {
            format!("{}px", value.length)
        } else if value.length == 0.0 {
            let percentage = (value.fraction * 1_000_000.0).round() / 10_000.0;
            format!("{percentage}%")
        } else {
            format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
        }
    };
    let inset = |edges: &[LengthPercentageFixture; 4], radii: &[CornerRadiusFixture; 4]| {
        let edges = edges.map(coordinate);
        let horizontal = radii.map(|radius| format!("{}px", radius.horizontal()));
        let vertical = radii.map(|radius| format!("{}px", radius.vertical()));
        let edges = css_four_value_shorthand(&edges);
        let horizontal = css_four_value_shorthand(&horizontal);
        let vertical = css_four_value_shorthand(&vertical);
        let radii = if horizontal == vertical {
            horizontal
        } else {
            format!("{horizontal} / {vertical}")
        };
        format!("inset({edges} round {radii})")
    };
    let shape = match &value.shape {
        ClipShapeFixture::Inset { edges, radii } => inset(edges, radii),
        ClipShapeFixture::Circle { radius, center } => format!(
            "circle({} at {} {})",
            coordinate(*radius),
            coordinate(center[0]),
            coordinate(center[1])
        ),
        ClipShapeFixture::Ellipse { radii, center } => format!(
            "ellipse({} {} at {} {})",
            coordinate(radii[0]),
            coordinate(radii[1]),
            coordinate(center[0]),
            coordinate(center[1])
        ),
        ClipShapeFixture::Path {
            fill_rule,
            commands,
        } => {
            let point = |value: &[LengthPercentageFixture; 2]| {
                format!("{} {}", value[0].length, value[1].length)
            };
            let commands = commands
                .iter()
                .map(|command| match command {
                    PathCommandFixture::MoveTo { point: value } => format!("M {}", point(value)),
                    PathCommandFixture::LineTo { point: value } => format!("L {}", point(value)),
                    PathCommandFixture::QuadraticTo { control, end } => {
                        format!("Q {} {}", point(control), point(end))
                    }
                    PathCommandFixture::CubicTo {
                        control_1,
                        control_2,
                        end,
                    } => format!("C {} {} {}", point(control_1), point(control_2), point(end)),
                    PathCommandFixture::Close => "Z".into(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            match fill_rule {
                FillRuleFixture::NonZero => format!("path(\"{commands}\")"),
                FillRuleFixture::EvenOdd => format!("path(evenodd, \"{commands}\")"),
            }
        }
    };
    let suffix = if reference_box.is_empty() {
        String::new()
    } else {
        format!(" {reference_box}")
    };
    format!("{shape}{suffix}")
}

fn css_four_value_shorthand(values: &[String; 4]) -> String {
    if values.iter().all(|value| value == &values[0]) {
        values[0].clone()
    } else if values[0] == values[2] && values[1] == values[3] {
        format!("{} {}", values[0], values[1])
    } else if values[1] == values[3] {
        format!("{} {} {}", values[0], values[1], values[2])
    } else {
        values.join(" ")
    }
}

fn fixture_color_css(value: &ColorFixture) -> String {
    match value {
        ColorFixture::Named { value } => value.clone(),
        ColorFixture::Srgba {
            red,
            green,
            blue,
            alpha,
        } if *alpha == 1.0 => format!("rgb({red}, {green}, {blue})"),
        ColorFixture::Srgba {
            red,
            green,
            blue,
            alpha,
        } => format!("rgba({red}, {green}, {blue}, {alpha})"),
    }
}

fn fixture_color_rgba(value: &ColorFixture) -> [u8; 4] {
    match value {
        ColorFixture::Srgba {
            red,
            green,
            blue,
            alpha,
        } => [
            *red,
            *green,
            *blue,
            (*alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        ],
        ColorFixture::Named { value } => match value.as_str() {
            "transparent" => [0, 0, 0, 0],
            "black" => [0, 0, 0, 255],
            "white" => [255, 255, 255, 255],
            "red" => [255, 0, 0, 255],
            "green" => [0, 128, 0, 255],
            "blue" => [0, 0, 255, 255],
            "yellow" => [255, 255, 0, 255],
            "gray" => [128, 128, 128, 255],
            name => panic!("raster fixture uses unsupported named color {name}"),
        },
    }
}

fn fixture_color_is_transparent(value: &ColorFixture) -> bool {
    match value {
        ColorFixture::Named { value } => value == "transparent",
        ColorFixture::Srgba { alpha, .. } => *alpha == 0.0,
    }
}

fn fixture_linear_gradient_css(gradient: &LinearGradientFixture) -> String {
    let stops = gradient
        .stops
        .iter()
        .map(|stop| {
            format!(
                "{} {}%",
                fixture_color_css(&stop.color),
                stop.position * 100.0
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("linear-gradient({}deg, {stops})", gradient.angle_degrees)
}

fn fixture_radial_gradient_css(gradient: &RadialGradientFixture) -> String {
    let stops = gradient
        .stops
        .iter()
        .map(|stop| {
            format!(
                "{} {}%",
                fixture_color_css(&stop.color),
                stop.position * 100.0
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "radial-gradient(ellipse {}px {}px at {}px {}px, {stops})",
        gradient.radii[0], gradient.radii[1], gradient.center[0], gradient.center[1]
    )
}

fn fixture_conic_gradient_css(gradient: &ConicGradientFixture) -> String {
    let stops = gradient
        .stops
        .iter()
        .map(|stop| format!("{} {}turn", fixture_color_css(&stop.color), stop.position))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "conic-gradient(from {}deg at {}px {}px, {stops})",
        gradient.from_degrees, gradient.center[0], gradient.center[1]
    )
}

fn assert_background_layers_are_projected(
    style: &web_sys::CssStyleDeclaration,
    layers: &[BackgroundPaintLayerFixture],
    resource_urls: &HashMap<u64, String>,
) {
    let images = layers
        .iter()
        .map(|layer| match &layer.image {
            BackgroundImageFixture::Resource(id) => {
                format!("url(\"{}\")", resource_urls.get(id).unwrap())
            }
            BackgroundImageFixture::LinearGradient(gradient) => {
                fixture_linear_gradient_css(gradient)
            }
            BackgroundImageFixture::RadialGradient(gradient) => {
                fixture_radial_gradient_css(gradient)
            }
            BackgroundImageFixture::ConicGradient(gradient) => fixture_conic_gradient_css(gradient),
        })
        .collect::<Vec<_>>()
        .join(", ");
    assert_style(style, "background-image", &images);
    assert_style(
        style,
        "background-position",
        &layers
            .iter()
            .map(|layer| fixture_background_position(layer.geometry))
            .collect::<Vec<_>>()
            .join(", "),
    );
    assert_style(
        style,
        "background-size",
        &layers
            .iter()
            .map(|layer| fixture_background_size(layer.geometry))
            .collect::<Vec<_>>()
            .join(", "),
    );
    assert_style(
        style,
        "background-repeat",
        &layers
            .iter()
            .map(|layer| fixture_background_repeat(layer.geometry))
            .collect::<Vec<_>>()
            .join(", "),
    );
    assert_style(
        style,
        "background-origin",
        &layers
            .iter()
            .map(|layer| fixture_background_box(layer.geometry.origin))
            .collect::<Vec<_>>()
            .join(", "),
    );
    assert_style(
        style,
        "background-clip",
        &layers
            .iter()
            .map(|layer| fixture_background_box(layer.geometry.clip))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let attachments = std::iter::repeat_n("scroll", layers.len())
        .collect::<Vec<_>>()
        .join(", ");
    let blend_modes = std::iter::repeat_n("normal", layers.len())
        .collect::<Vec<_>>()
        .join(", ");
    assert_style(style, "background-attachment", &attachments);
    assert_style(style, "background-blend-mode", &blend_modes);
}

fn assert_background_layer_is_projected(
    style: &web_sys::CssStyleDeclaration,
    layer: BackgroundLayerFixture,
) {
    assert_style(
        style,
        "background-position",
        &fixture_background_position(layer),
    );
    assert_style(style, "background-size", &fixture_background_size(layer));
    assert_style(style, "background-repeat", fixture_background_repeat(layer));
    assert_style(
        style,
        "background-origin",
        fixture_background_box(layer.origin),
    );
    assert_style(style, "background-clip", fixture_background_box(layer.clip));
    assert_style(style, "background-attachment", "scroll");
    assert_style(style, "background-blend-mode", "normal");
}

fn fixture_background_position(layer: BackgroundLayerFixture) -> String {
    if layer.position == [LengthPercentageFixture::default(); 2] {
        "0px 0px".into()
    } else {
        format!(
            "{} {}",
            fixture_coordinate(layer.position[0]),
            fixture_coordinate(layer.position[1])
        )
    }
}

fn fixture_background_size(layer: BackgroundLayerFixture) -> String {
    let axis = |value: Option<LengthPercentageFixture>| {
        value.map_or_else(|| "auto".into(), fixture_length_percentage)
    };
    match layer.size {
        BackgroundSizeFixture::ExplicitPair(size) => format!(
            "{} {}",
            fixture_length_percentage(size[0]),
            fixture_length_percentage(size[1])
        ),
        BackgroundSizeFixture::ExplicitAxes { width, height } => {
            format!("{} {}", axis(width), axis(height))
        }
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Auto) => "auto".into(),
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Cover) => "cover".into(),
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Contain) => "contain".into(),
    }
}

fn fixture_background_repeat(layer: BackgroundLayerFixture) -> &'static str {
    match (layer.repeat_x, layer.repeat_y) {
        (ImageRepeatFixture::Repeat, ImageRepeatFixture::Repeat) => "repeat",
        (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::NoRepeat) => "no-repeat",
        (ImageRepeatFixture::Repeat, ImageRepeatFixture::NoRepeat) => "repeat no-repeat",
        (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::Repeat) => "no-repeat repeat",
        (ImageRepeatFixture::Space, ImageRepeatFixture::Space) => "space",
        (ImageRepeatFixture::Space, ImageRepeatFixture::Repeat) => "space repeat",
        (ImageRepeatFixture::Space, ImageRepeatFixture::NoRepeat) => "space no-repeat",
        (ImageRepeatFixture::Repeat, ImageRepeatFixture::Space) => "repeat space",
        (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::Space) => "no-repeat space",
        (ImageRepeatFixture::Round, ImageRepeatFixture::Round) => "round",
        (ImageRepeatFixture::Round, ImageRepeatFixture::Repeat) => "round repeat",
        (ImageRepeatFixture::Round, ImageRepeatFixture::NoRepeat) => "round no-repeat",
        (ImageRepeatFixture::Round, ImageRepeatFixture::Space) => "round space",
        (ImageRepeatFixture::Repeat, ImageRepeatFixture::Round) => "repeat round",
        (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::Round) => "no-repeat round",
        (ImageRepeatFixture::Space, ImageRepeatFixture::Round) => "space round",
    }
}

fn fixture_background_box(value: BackgroundBoxFixture) -> &'static str {
    match value {
        BackgroundBoxFixture::Border => "border-box",
        BackgroundBoxFixture::Padding => "padding-box",
        BackgroundBoxFixture::Content => "content-box",
        BackgroundBoxFixture::BorderArea => "border-area",
    }
}

fn fixture_length_percentage(value: LengthPercentageFixture) -> String {
    if value.length == 0.0 {
        format!("{}%", value.fraction * 100.0)
    } else if value.fraction == 0.0 {
        format!("{}px", value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn fixture_coordinate(value: LengthPercentageFixture) -> String {
    fixture_length_percentage(value)
}

fn fixture_node_paints_at(
    node: &SceneNodeFixture,
    point: [f32; 2],
    sample_color: &ColorFixture,
    resource_dimensions: &HashMap<u64, [f32; 2]>,
) -> bool {
    let has_resource_layer = node
        .background_layers
        .iter()
        .any(|layer| matches!(layer.image, BackgroundImageFixture::Resource(_)));
    if !fixture_color_is_transparent(&node.background)
        && (!has_resource_layer || node.background == *sample_color)
    {
        return true;
    }
    if node.background_layers.is_empty()
        && node.linear_gradient.is_none()
        && node.radial_gradient.is_none()
        && node.conic_gradient.is_none()
    {
        return false;
    }
    if !node.background_layers.is_empty() {
        return node.background_layers.iter().any(|layer| {
            let intrinsic_size = match layer.image {
                BackgroundImageFixture::Resource(id) => resource_dimensions.get(&id).copied(),
                _ => None,
            };
            fixture_background_layer_paints_at(node, layer.geometry, intrinsic_size, point)
        });
    }
    fixture_background_layer_paints_at(node, node.background_layer, None, point)
}

fn fixture_background_layer_paints_at(
    node: &SceneNodeFixture,
    layer: BackgroundLayerFixture,
    intrinsic_size: Option<[f32; 2]>,
    point: [f32; 2],
) -> bool {
    let border_widths = node
        .border
        .as_ref()
        .map_or([0.0; 4], |border| border.widths);
    let positioning_area = fixture_background_area(node, border_widths, layer.origin);
    let clip_area = fixture_background_area(node, border_widths, layer.clip);
    if !fixture_rect_contains(clip_area, point) {
        return false;
    }
    if layer.clip == BackgroundBoxFixture::BorderArea
        && fixture_rect_contains(
            fixture_background_area(node, border_widths, BackgroundBoxFixture::Padding),
            point,
        )
    {
        return false;
    }
    let mut size = fixture_background_image_size(layer.size, positioning_area, intrinsic_size);
    if intrinsic_size.is_some() {
        match layer.size {
            BackgroundSizeFixture::ExplicitAxes {
                width: Some(_),
                height: None,
            } if layer.repeat_x == ImageRepeatFixture::Round
                && layer.repeat_y != ImageRepeatFixture::Round
                && size[0] > 0.0 =>
            {
                let rounded_width = fixture_round_tile_length(positioning_area[2], size[0]);
                size[1] *= rounded_width / size[0];
                size[0] = rounded_width;
            }
            BackgroundSizeFixture::ExplicitAxes {
                width: None,
                height: Some(_),
            } if layer.repeat_y == ImageRepeatFixture::Round
                && layer.repeat_x != ImageRepeatFixture::Round
                && size[1] > 0.0 =>
            {
                let rounded_height = fixture_round_tile_length(positioning_area[3], size[1]);
                size[0] *= rounded_height / size[1];
                size[1] = rounded_height;
            }
            _ => {}
        }
    }
    let position = [
        positioning_area[0]
            + layer.position[0].length
            + layer.position[0].fraction * (positioning_area[2] - size[0]),
        positioning_area[1]
            + layer.position[1].length
            + layer.position[1].fraction * (positioning_area[3] - size[1]),
    ];
    let paints_x = fixture_background_axis_paints_at(
        positioning_area[0],
        positioning_area[2],
        position[0],
        size[0],
        layer.repeat_x,
        point[0],
    );
    let paints_y = fixture_background_axis_paints_at(
        positioning_area[1],
        positioning_area[3],
        position[1],
        size[1],
        layer.repeat_y,
        point[1],
    );
    paints_x && paints_y
}

fn fixture_round_tile_length(area_length: f32, tile_length: f32) -> f32 {
    let tile_count = (area_length / tile_length).round().max(1.0);
    area_length / tile_count
}

fn fixture_background_image_size(
    size: BackgroundSizeFixture,
    positioning_area: [f32; 4],
    intrinsic_size: Option<[f32; 2]>,
) -> [f32; 2] {
    let area = [positioning_area[2], positioning_area[3]];
    let intrinsic = intrinsic_size.filter(|size| size[0] > 0.0 && size[1] > 0.0);
    let preserve_ratio = |resolved: Option<f32>, axis: usize| match (resolved, intrinsic) {
        (Some(value), _) => value,
        (None, Some(intrinsic)) => intrinsic[axis],
        (None, None) => area[axis],
    };
    match size {
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Auto) => {
            intrinsic.unwrap_or(area)
        }
        BackgroundSizeFixture::Keyword(
            keyword @ (BackgroundSizeKeywordFixture::Cover | BackgroundSizeKeywordFixture::Contain),
        ) => {
            let Some(intrinsic) = intrinsic else {
                return area;
            };
            let width_scale = area[0] / intrinsic[0];
            let height_scale = area[1] / intrinsic[1];
            let scale = match keyword {
                BackgroundSizeKeywordFixture::Cover => width_scale.max(height_scale),
                BackgroundSizeKeywordFixture::Contain => width_scale.min(height_scale),
                BackgroundSizeKeywordFixture::Auto => unreachable!(),
            };
            [intrinsic[0] * scale, intrinsic[1] * scale]
        }
        BackgroundSizeFixture::ExplicitPair(size) => [
            size[0].length + size[0].fraction * area[0],
            size[1].length + size[1].fraction * area[1],
        ],
        BackgroundSizeFixture::ExplicitAxes { width, height } => {
            let width = width.map(|value| value.length + value.fraction * area[0]);
            let height = height.map(|value| value.length + value.fraction * area[1]);
            match (width, height, intrinsic) {
                (Some(width), None, Some(intrinsic)) => {
                    [width, width * intrinsic[1] / intrinsic[0]]
                }
                (None, Some(height), Some(intrinsic)) => {
                    [height * intrinsic[0] / intrinsic[1], height]
                }
                (width, height, _) => [preserve_ratio(width, 0), preserve_ratio(height, 1)],
            }
        }
    }
}

fn fixture_background_axis_paints_at(
    area_start: f32,
    area_length: f32,
    tile_start: f32,
    tile_length: f32,
    repeat: ImageRepeatFixture,
    point: f32,
) -> bool {
    if tile_length <= 0.0 {
        return false;
    }
    match repeat {
        ImageRepeatFixture::NoRepeat => (tile_start..tile_start + tile_length).contains(&point),
        ImageRepeatFixture::Repeat | ImageRepeatFixture::Round => true,
        ImageRepeatFixture::Space => {
            let tile_count = (area_length / tile_length).floor() as usize;
            if tile_count < 2 {
                return (tile_start..tile_start + tile_length).contains(&point);
            }
            let offset = point - area_start;
            if !(0.0..area_length).contains(&offset) {
                return false;
            }
            let gap = (area_length - tile_count as f32 * tile_length)
                / (tile_count.saturating_sub(1)) as f32;
            let stride = tile_length + gap;
            offset % stride < tile_length
        }
    }
}

fn fixture_background_area(
    node: &SceneNodeFixture,
    border_widths: [f32; 4],
    background_box: BackgroundBoxFixture,
) -> [f32; 4] {
    match background_box {
        BackgroundBoxFixture::Border => node.rect,
        BackgroundBoxFixture::Padding => [
            node.rect[0] + border_widths[3],
            node.rect[1] + border_widths[0],
            node.rect[2] - border_widths[1] - border_widths[3],
            node.rect[3] - border_widths[0] - border_widths[2],
        ],
        BackgroundBoxFixture::Content => {
            let content_box = node.resolved_content_box();
            [
                node.rect[0] + content_box[0],
                node.rect[1] + content_box[1],
                content_box[2],
                content_box[3],
            ]
        }
        BackgroundBoxFixture::BorderArea => node.rect,
    }
}

fn fixture_padding(node: &SceneNodeFixture) -> [f32; 4] {
    let content_box = node.resolved_content_box();
    let border_widths = node.border.as_ref().map_or([0.0; 4], |border| {
        std::array::from_fn(|index| {
            if border.styles[index] != BorderStyleFixture::None {
                border.widths[index]
            } else {
                0.0
            }
        })
    });
    [
        (content_box[1] - border_widths[0]).max(0.0),
        (node.rect[2] - content_box[0] - content_box[2] - border_widths[1]).max(0.0),
        (node.rect[3] - content_box[1] - content_box[3] - border_widths[2]).max(0.0),
        (content_box[0] - border_widths[3]).max(0.0),
    ]
}

fn fixture_rect_contains(rect: [f32; 4], point: [f32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}

fn fixture_border_style_css(value: BorderStyleFixture) -> &'static str {
    match value {
        BorderStyleFixture::None => "none",
        BorderStyleFixture::Hidden => "hidden",
        BorderStyleFixture::Solid => "solid",
        BorderStyleFixture::Dashed => "dashed",
        BorderStyleFixture::Dotted => "dotted",
        BorderStyleFixture::Double => "double",
        BorderStyleFixture::Groove => "groove",
        BorderStyleFixture::Ridge => "ridge",
        BorderStyleFixture::Inset => "inset",
        BorderStyleFixture::Outset => "outset",
    }
}

fn fixture_overflow_css(value: OverflowClipFixture) -> &'static str {
    match value {
        OverflowClipFixture::Visible => "visible",
        OverflowClipFixture::Hidden => "clip",
    }
}

fn fixture_transform_css(transform: [f32; 16]) -> String {
    let values = transform
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("matrix3d({values})")
}

fn protocol_visibility(value: VisibilityFixture) -> Visibility {
    match value {
        VisibilityFixture::Visible => Visibility::Visible,
        VisibilityFixture::Hidden => Visibility::Hidden,
    }
}

fn fixture_visibility_css(value: VisibilityFixture) -> &'static str {
    match value {
        VisibilityFixture::Visible => "visible",
        VisibilityFixture::Hidden => "hidden",
    }
}
