use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use whisker::ElementRegistry;
use whisker_engine::FrameSink;
use whisker_host_conformance::{
    BackgroundBoxFixture, BackgroundLayerFixture, BorderFixture, BorderStyleFixture, ColorFixture,
    Command, ConicGradientFixture, CornerRadiusFixture, ImageRepeatFixture,
    LengthPercentageFixture, LinearGradientFixture, Manifest, OverflowClipFixture,
    PixelSampleFixture, RadialGradientFixture, SCHEMA_VERSION, Scenario, ScenarioSide,
    SceneNodeFixture, VisibilityFixture,
};
use whisker_protocol::{
    BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, BorderLineStyle, BoxClip,
    BoxPaint, FrameHeader, FrameMode, FramePacket, GradientStop, ImageRepeat, LayoutGeometry,
    LayoutRect, NodeId, Operation, OverflowClip, PaintBox, PaintColor, PaintCoordinate,
    PaintCornerRadius, PaintCorners, PaintEdges, PaintImage, PaintLengthPercentage, PaintPosition,
    ProtocolVersion, RadialGradientExtent, RadialGradientShape, SurfaceId, Transform, Visibility,
};

use crate::module_api::built_in_element_factories;
use crate::scene::frame_sink::DomFrameSink;

const MANIFEST: &str = include_str!("../../../../tests/host-conformance/manifest.json");

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Driver {
    root: web_sys::Element,
    sink: DomFrameSink,
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
        let sink = DomFrameSink::new(
            document,
            root.clone(),
            surface,
            elements.registrations(),
            &built_in_element_factories(),
        )
        .unwrap();
        Self {
            root,
            sink,
            expected_box: None,
            expected_scene: None,
        }
    }

    fn execute(&mut self, side: &ScenarioSide) {
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
                        let expected_checkpoint = self
                            .expected_scene
                            .as_ref()
                            .and_then(|nodes| {
                                nodes
                                    .iter()
                                    .any(|node| {
                                        node.background_layer.position
                                            != [LengthPercentageFixture::default(); 2]
                                    })
                                    .then_some("paint.background-layers.position-length-percentage")
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
                            .unwrap_or("paint.box");
                        assert_eq!(name, expected_checkpoint);
                        self.assert_scene_is_projected(samples);
                    } else {
                        assert_eq!(name, "paint.box");
                        self.assert_box_is_projected();
                    }
                }
                Command::MeasureText { .. }
                | Command::CheckpointMeasurement { .. }
                | Command::EmitPointer { .. }
                | Command::CheckpointInput { .. } => {
                    panic!("non-paint command reached the Web paint runner")
                }
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
            assert_style(
                &style,
                "background-color",
                &fixture_color_css(&fixture_node.background),
            );
            assert_border_is_projected(&style, fixture_node.border.as_ref());
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
            if let Some(gradient) = &fixture_node.linear_gradient {
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
                    node.id.to_string() == id.as_str() && fixture_node_paints_at(node, sample.point)
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
                        .rev()
                        .find(|node| node.opacity.is_some())
                        .or_else(|| {
                            expected.iter().find(|node| {
                                node.linear_gradient.is_some()
                                    || node.radial_gradient.is_some()
                                    || node.conic_gradient.is_some()
                            })
                        })
                        .expect("sRGBA sample requires an opacity or gradient source node")
                        .id
                        .to_string();
                    assert_eq!(
                        opaque_hit.map(String::as_str),
                        Some(expected_node.as_str()),
                        "composited sample did not hit the opacity source node"
                    );
                }
                expected_color => {
                    let expected_node = expected
                        .iter()
                        .find(|node| node.background == *expected_color)
                        .or_else(|| {
                            expected.iter().find(|node| {
                                node.linear_gradient.is_some()
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
                        "paint sample did not hit the expected transformed scene node"
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
fn every_shared_paint_fixture_reaches_the_production_dom_sink() {
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
                Command::PresentBox { .. } | Command::PresentScene { .. }
            )
        }) {
            continue;
        }
        Driver::new().execute(&scenario.test);
        if let Some(reference) = &scenario.reference {
            Driver::new().execute(reference);
        }
        count += 1;
    }
    assert!(count > 0);
}

fn fixture(path: &str) -> &'static str {
    match path {
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
        "wpt/css/css-backgrounds/background-position-three-four-values.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/background-position-three-four-values.json"
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
        _ => panic!("manifest fixture is not embedded in the Web test: {path}"),
    }
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
                    content_box: LayoutRect::default(),
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
    let view = ElementRegistry::standard()
        .registration_for_builtin(whisker::ElementTag::View)
        .unwrap()
        .element_type;
    let mut operations = Vec::with_capacity(nodes.len() * 5);
    for fixture_node in nodes {
        let node = fixture_node_id(fixture_node.id);
        operations.extend([
            Operation::CreateNode {
                node,
                element_type: view,
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
                    content_box: LayoutRect::default(),
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
        if let Some(transform) = fixture_node.transform {
            operations.push(Operation::SetTransform {
                node,
                transform: Transform(transform),
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
        if let Some(gradient) = &fixture_node.linear_gradient {
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
    layer.size = geometry
        .size
        .map_or(BackgroundSize::Auto, |size| BackgroundSize::Explicit {
            width: Some(paint_length_percentage(size[0])),
            height: Some(paint_length_percentage(size[1])),
        });
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

fn fixture_color_css(value: &ColorFixture) -> String {
    match value {
        ColorFixture::Named { value } => value.clone(),
        ColorFixture::Srgba {
            red,
            green,
            blue,
            alpha,
        } => format!("rgba({red}, {green}, {blue}, {alpha})"),
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
    layer.size.map_or_else(
        || "auto".into(),
        |size| {
            format!(
                "{} {}",
                fixture_length_percentage(size[0]),
                fixture_length_percentage(size[1])
            )
        },
    )
}

fn fixture_background_repeat(layer: BackgroundLayerFixture) -> &'static str {
    match (layer.repeat_x, layer.repeat_y) {
        (ImageRepeatFixture::Repeat, ImageRepeatFixture::Repeat) => "repeat",
        (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::NoRepeat) => "no-repeat",
        (ImageRepeatFixture::Repeat, ImageRepeatFixture::NoRepeat) => "repeat no-repeat",
        (ImageRepeatFixture::NoRepeat, ImageRepeatFixture::Repeat) => "no-repeat repeat",
        (ImageRepeatFixture::Space, _) | (_, ImageRepeatFixture::Space) => "space",
        (ImageRepeatFixture::Round, _) | (_, ImageRepeatFixture::Round) => "round",
    }
}

fn fixture_background_box(value: BackgroundBoxFixture) -> &'static str {
    match value {
        BackgroundBoxFixture::Border => "border-box",
        BackgroundBoxFixture::Padding => "padding-box",
        BackgroundBoxFixture::Content => "content-box",
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

fn fixture_node_paints_at(node: &SceneNodeFixture, point: [f32; 2]) -> bool {
    if !fixture_color_is_transparent(&node.background) {
        return true;
    }
    if node.linear_gradient.is_none()
        && node.radial_gradient.is_none()
        && node.conic_gradient.is_none()
    {
        return false;
    }
    let layer = node.background_layer;
    let size = layer.size.map_or([node.rect[2], node.rect[3]], |size| {
        [
            size[0].length + size[0].fraction * node.rect[2],
            size[1].length + size[1].fraction * node.rect[3],
        ]
    });
    let position = [
        node.rect[0]
            + layer.position[0].length
            + layer.position[0].fraction * (node.rect[2] - size[0]),
        node.rect[1]
            + layer.position[1].length
            + layer.position[1].fraction * (node.rect[3] - size[1]),
    ];
    let paints_x = layer.repeat_x != ImageRepeatFixture::NoRepeat
        || (position[0]..position[0] + size[0]).contains(&point[0]);
    let paints_y = layer.repeat_y != ImageRepeatFixture::NoRepeat
        || (position[1]..position[1] + size[1]).contains(&point[1]);
    paints_x && paints_y
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
