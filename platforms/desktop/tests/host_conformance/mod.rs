use whisker::css::BorderStyle;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{set_root, with_installed_renderer};
use whisker::{SurfaceRuntime, standard_element_registrations};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::{FrameSink, MeasurementProvider};
use whisker_host_conformance::{
    BorderFixture, BorderStyleFixture, ColorFixture, Command, Host, LoadedCase,
    PixelRelationFixture, PixelRelationKind, PixelSampleFixture, PointerEventFixture, Scenario,
    ScenarioSide, load_required,
};
use whisker_protocol::{
    AvailableSpace, BorderLineStyle, BoxPaint, ElementTypeId, FrameHeader, FrameMode, FramePacket,
    InputEvent, InputEventKind, InputPoint, LayoutGeometry, LayoutRect, MeasureConstraints,
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWrap, MeasurementKey, MeasurementPayload, MeasurementRequest,
    MeasurementResponse, NodeId, Operation, PaintColor, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintLengthPercentage, PointerId, PointerInput, PointerKind, ProtocolVersion,
    SurfaceId, TextMeasurePayload, TextMeasureStyle, WhiskerValue,
};
use whisker_style::{PropertyOrigin, StyleEnvironment, StyleProperty};

use crate::element::{DesktopElementRegistry, built_in_element_factories};
use crate::gpu::render_box_primitives_offscreen;
use crate::paint::box_paint::{BoxPrimitive, BoxPrimitiveKind, lower_box};
use crate::paint::color::srgba;
use crate::scene::{DesktopScene, PaintCommand};
use crate::text::NativeTextHost;

const CAPABILITIES: &str = include_str!("../../../../tests/host-conformance/capabilities.json");

const fn pointer_event_protocol(value: PointerEventFixture) -> InputEventKind {
    match value {
        PointerEventFixture::Down => InputEventKind::PointerDown,
        PointerEventFixture::Move => InputEventKind::PointerMove,
        PointerEventFixture::Up => InputEventKind::PointerUp,
        PointerEventFixture::Cancel => InputEventKind::PointerCancel,
    }
}

fn color_protocol(value: &ColorFixture) -> PaintColor {
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

const fn border_style_protocol(value: BorderStyleFixture) -> BorderLineStyle {
    match value {
        BorderStyleFixture::None => BorderLineStyle::None,
        BorderStyleFixture::Hidden => BorderLineStyle::Hidden,
        BorderStyleFixture::Solid => BorderLineStyle::Solid,
        BorderStyleFixture::Dashed => BorderLineStyle::Dashed,
        BorderStyleFixture::Dotted => BorderLineStyle::Dotted,
        BorderStyleFixture::Double => BorderLineStyle::Double,
        BorderStyleFixture::Groove => BorderLineStyle::Groove,
        BorderStyleFixture::Ridge => BorderLineStyle::Ridge,
        BorderStyleFixture::Inset => BorderLineStyle::Inset,
        BorderStyleFixture::Outset => BorderLineStyle::Outset,
    }
}

struct Driver {
    surface: Option<SurfaceId>,
    scene: Option<DesktopScene>,
    logical_size: [f32; 2],
    scale: f32,
    text: NativeTextHost,
    measurement_responses: Vec<MeasurementResponse>,
    input: RecordingInputSink,
}

#[derive(Default)]
struct RecordingInputSink {
    events: Vec<InputEvent>,
}

impl RecordingInputSink {
    fn dispatch(&mut self, event: InputEvent) {
        event
            .validate()
            .expect("Host scenario emits valid normalized input");
        self.events.push(event);
    }
}

struct Checkpoint {
    logical_size: [u32; 2],
    primitives: Vec<BoxPrimitive>,
    samples: Vec<PixelSampleFixture>,
    relations: Vec<PixelRelationFixture>,
}

fn standard_element_type(name: &str) -> ElementTypeId {
    standard_element_registrations()
        .into_iter()
        .find(|registration| registration.name == name)
        .expect("standard element registration")
        .element_type
}

fn desktop_scene(surface: SurfaceId) -> DesktopScene {
    DesktopScene::new(
        surface,
        DesktopElementRegistry::bind(
            &standard_element_registrations(),
            &built_in_element_factories(),
        )
        .unwrap(),
    )
}

impl Driver {
    fn new() -> Self {
        Self {
            surface: None,
            scene: None,
            logical_size: [0.0; 2],
            scale: 1.0,
            text: NativeTextHost::new(
                DesktopElementRegistry::bind(
                    &standard_element_registrations(),
                    &built_in_element_factories(),
                )
                .unwrap(),
            ),
            measurement_responses: Vec::new(),
            input: RecordingInputSink::default(),
        }
    }

    fn execute(mut self, side: &ScenarioSide) -> Vec<Checkpoint> {
        let mut checkpoints = Vec::new();
        for command in &side.commands {
            match command {
                Command::AttachSurface {
                    width,
                    height,
                    scale,
                } => {
                    assert!(*width > 0.0 && *height > 0.0 && *scale > 0.0);
                    let surface = SurfaceId::new(1).unwrap();
                    self.surface = Some(surface);
                    self.scene = Some(desktop_scene(surface));
                    self.logical_size = [*width, *height];
                    self.scale = *scale;
                }
                Command::PresentBox {
                    revision,
                    rect,
                    background,
                    border,
                } => self.present_box(*revision, *rect, background, border.as_ref()),
                Command::Checkpoint {
                    name,
                    samples,
                    relations,
                } => {
                    assert_eq!(name, "paint.box", "unsupported Desktop checkpoint");
                    checkpoints.push(Checkpoint {
                        logical_size: [
                            self.logical_size[0].round() as u32,
                            self.logical_size[1].round() as u32,
                        ],
                        primitives: self.box_primitives(),
                        samples: samples.clone(),
                        relations: relations.clone(),
                    });
                }
                Command::MeasureText {
                    key,
                    text,
                    font_size,
                    line_height,
                    available_width,
                } => self.measure_text(*key, text, *font_size, *line_height, *available_width),
                Command::CheckpointMeasurement {
                    key,
                    min_width,
                    max_width,
                    min_height,
                    max_height,
                    prepared_content,
                } => self.check_measurement(
                    *key,
                    [*min_width, *max_width],
                    [*min_height, *max_height],
                    *prepared_content,
                ),
                Command::EmitPointer {
                    event,
                    pointer_id,
                    timestamp_ms,
                    x,
                    y,
                    buttons,
                    changed_button,
                } => self.emit_pointer(
                    *event,
                    *pointer_id,
                    *timestamp_ms,
                    [*x, *y],
                    *buttons,
                    *changed_button,
                ),
                Command::CheckpointInput {
                    event,
                    pointer_id,
                    x,
                    y,
                } => self.check_input(*event, *pointer_id, [*x, *y]),
            }
        }
        checkpoints
    }

    fn present_box(
        &mut self,
        revision: u64,
        rect: [f32; 4],
        background: &ColorFixture,
        border: Option<&BorderFixture>,
    ) {
        let surface = self.surface.expect("attach_surface precedes present_box");
        assert!(self.scale.is_finite() && self.scale > 0.0);
        assert!(rect[0] >= 0.0 && rect[1] >= 0.0);
        assert!(rect[0] + rect[2] <= self.logical_size[0]);
        assert!(rect[1] + rect[3] <= self.logical_size[1]);
        let node = NodeId::new(1).unwrap();
        let expected_paint = box_paint(background, border);
        let packet = FramePacket {
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
                    element_type: standard_element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::SetLayout {
                    node,
                    geometry: LayoutGeometry {
                        border_box: layout_rect(rect),
                        content_box: LayoutRect::default(),
                    },
                },
                Operation::SetBoxPaint {
                    node,
                    paint: expected_paint.clone(),
                },
            ],
        };
        let scene = self.scene.as_mut().expect("attached Desktop scene");
        scene
            .present(&packet)
            .expect("canonical Host scenario packet is valid");
        match scene.paint_commands().as_slice() {
            [
                PaintCommand::Box {
                    rect: actual_rect,
                    paint: actual_paint,
                    ..
                },
            ] => {
                assert_eq!(*actual_rect, layout_rect(rect));
                assert_eq!(*actual_paint, &expected_paint);
            }
            _ => panic!("one projected Desktop box command"),
        }
    }

    fn box_primitives(&self) -> Vec<BoxPrimitive> {
        let scene = self.scene.as_ref().expect("checkpoint follows attach");
        let mut primitives = Vec::new();
        for command in scene.paint_commands() {
            if let PaintCommand::Box {
                rect,
                paint,
                opacity,
                ..
            } = command
            {
                lower_box(rect, paint, opacity, |primitive| {
                    primitives.push(primitive);
                });
            }
        }
        primitives
    }

    fn measure_text(
        &mut self,
        key: u64,
        value: &str,
        font_size: f32,
        line_height: f32,
        available_width: f32,
    ) {
        let surface = self.surface.expect("attach_surface precedes measure_text");
        let request = MeasurementRequest {
            key: MeasurementKey::new(key).expect("scenario measurement key is non-zero"),
            node: NodeId::new(key).expect("scenario node key is non-zero"),
            element_type: standard_element_type(whisker::TEXT_ELEMENT_NAME),
            environment_epoch: 1,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [
                    AvailableSpace::Definite(available_width),
                    AvailableSpace::MaxContent,
                ],
            },
            payload: MeasurementPayload::Text(TextMeasurePayload {
                text: value.into(),
                style: TextMeasureStyle {
                    font_families: vec![MeasureFontFamily::System],
                    font_size,
                    font_weight: 400,
                    font_style: MeasureFontStyle::Normal,
                    line_height: MeasureLineHeight::LogicalPixels(line_height),
                    letter_spacing: 0.0,
                    ..TextMeasureStyle::default()
                },
                locale: None,
                direction: MeasureTextDirection::Auto,
                wrap: MeasureTextWrap::Wrap,
                max_lines: None,
                overflow: MeasureTextOverflow::Clip,
            }),
        };
        self.text
            .measure_batch(surface, &[request], &mut self.measurement_responses)
            .expect("Desktop text measurement is infallible");
    }

    fn check_measurement(
        &self,
        key: u64,
        width: [f32; 2],
        height: [f32; 2],
        expects_prepared_content: bool,
    ) {
        let response = self
            .measurement_responses
            .iter()
            .find(|response| response.key().get() == key)
            .expect("measurement checkpoint key was observed");
        let MeasurementResponse::Ready { metrics, .. } = response else {
            panic!("Desktop text measurement is synchronously ready");
        };
        assert!((width[0]..=width[1]).contains(&metrics.size.width));
        assert!((height[0]..=height[1]).contains(&metrics.size.height));
        assert_eq!(metrics.prepared_content.is_some(), expects_prepared_content);
    }

    fn emit_pointer(
        &mut self,
        kind: PointerEventFixture,
        pointer_id: u64,
        timestamp_ms: f64,
        position: [f32; 2],
        buttons: u32,
        changed_button: i16,
    ) {
        let surface = self.surface.expect("attach_surface precedes emit_pointer");
        self.input.dispatch(InputEvent {
            surface,
            timestamp_ms,
            kind: pointer_event_protocol(kind),
            pointer: Some(PointerInput {
                id: PointerId::new(pointer_id).expect("scenario pointer id is non-zero"),
                kind: PointerKind::Mouse,
                position: InputPoint {
                    x: position[0],
                    y: position[1],
                },
                buttons,
                changed_button,
            }),
            target: None,
            detail: WhiskerValue::Null,
        });
    }

    fn check_input(&self, kind: PointerEventFixture, pointer_id: u64, position: [f32; 2]) {
        let event = self
            .input
            .events
            .last()
            .expect("input checkpoint follows event");
        assert_eq!(event.kind, pointer_event_protocol(kind));
        assert_eq!(event.surface, self.surface.expect("attached surface"));
        assert_eq!(event.target, None);
        let pointer = event.pointer.expect("pointer checkpoint has pointer data");
        assert_eq!(pointer.id.get(), pointer_id);
        assert_eq!(pointer.kind, PointerKind::Mouse);
        assert_close(pointer.position.x, position[0], "pointer x");
        assert_close(pointer.position.y, position[1], "pointer y");
    }
}

fn layout_rect([x, y, width, height]: [f32; 4]) -> LayoutRect {
    LayoutRect {
        x,
        y,
        width,
        height,
    }
}

fn box_paint(background: &ColorFixture, border: Option<&BorderFixture>) -> BoxPaint {
    let zero = PaintLengthPercentage::default();
    let Some(border) = border else {
        return BoxPaint {
            background_color: color_protocol(background),
            border_widths: PaintEdges {
                top: zero,
                right: zero,
                bottom: zero,
                left: zero,
            },
            border_colors: PaintEdges {
                top: PaintColor::default(),
                right: PaintColor::default(),
                bottom: PaintColor::default(),
                left: PaintColor::default(),
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::None,
                right: BorderLineStyle::None,
                bottom: BorderLineStyle::None,
                left: BorderLineStyle::None,
            },
            border_radii: PaintCorners {
                top_left: PaintCornerRadius::circular(zero),
                top_right: PaintCornerRadius::circular(zero),
                bottom_right: PaintCornerRadius::circular(zero),
                bottom_left: PaintCornerRadius::circular(zero),
            },
        };
    };
    let lengths = border.widths.map(|length| PaintLengthPercentage {
        length,
        fraction: 0.0,
    });
    let radii = border.radii.map(|length| {
        PaintCornerRadius::circular(PaintLengthPercentage {
            length,
            fraction: 0.0,
        })
    });
    BoxPaint {
        background_color: color_protocol(background),
        border_widths: PaintEdges {
            top: lengths[0],
            right: lengths[1],
            bottom: lengths[2],
            left: lengths[3],
        },
        border_colors: PaintEdges {
            top: color_protocol(&border.colors[0]),
            right: color_protocol(&border.colors[1]),
            bottom: color_protocol(&border.colors[2]),
            left: color_protocol(&border.colors[3]),
        },
        border_styles: PaintEdges {
            top: border_style_protocol(border.styles[0]),
            right: border_style_protocol(border.styles[1]),
            bottom: border_style_protocol(border.styles[2]),
            left: border_style_protocol(border.styles[3]),
        },
        border_radii: PaintCorners {
            top_left: radii[0],
            top_right: radii[1],
            bottom_right: radii[2],
            bottom_left: radii[3],
        },
    }
}

fn assert_close(left: f32, right: f32, context: &str) {
    assert!(
        (left - right).abs() <= 0.001,
        "{context}: {left} != {right}"
    );
}

fn run_reftest(scenario: &Scenario) {
    let reference_side = scenario
        .reference
        .as_ref()
        .expect("reftest scenario has reference commands");
    let test = Driver::new().execute(&scenario.test);
    let reference = Driver::new().execute(reference_side);
    assert_eq!(test.len(), 1, "one test checkpoint");
    assert_eq!(reference.len(), 1, "one reference checkpoint");
    assert_eq!(test[0].logical_size, reference[0].logical_size);

    let test_pixels = pollster::block_on(render_box_primitives_offscreen(
        &test[0].primitives,
        test[0].logical_size,
    ))
    .expect("Desktop test pixel checkpoint");
    let reference_pixels = pollster::block_on(render_box_primitives_offscreen(
        &reference[0].primitives,
        reference[0].logical_size,
    ))
    .expect("Desktop reference pixel checkpoint");
    assert_eq!(test_pixels.len(), reference_pixels.len());
    assert!(test_pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    let largest_difference = test_pixels
        .iter()
        .zip(&reference_pixels)
        .map(|(test, reference)| test.abs_diff(*reference))
        .max()
        .unwrap_or(0);
    assert!(
        largest_difference <= 1,
        "{} pixel checkpoint differs by {largest_difference}",
        scenario.id
    );
}

fn run_pixel_assertions(scenario: &Scenario) {
    let checkpoints = Driver::new().execute(&scenario.test);
    for checkpoint in checkpoints
        .iter()
        .filter(|checkpoint| !checkpoint.samples.is_empty() || !checkpoint.relations.is_empty())
    {
        let pixels = pollster::block_on(render_box_primitives_offscreen(
            &checkpoint.primitives,
            checkpoint.logical_size,
        ))
        .expect("Desktop pixel-sample checkpoint");
        let [width, height] = checkpoint.logical_size;
        for sample in &checkpoint.samples {
            let x = sample.point[0].floor() as u32;
            let y = sample.point[1].floor() as u32;
            assert!(
                x < width && y < height,
                "{} sample is outside the surface",
                scenario.id
            );
            let offset = ((y * width + x) * 4) as usize;
            let actual: [u8; 4] = pixels[offset..offset + 4].try_into().unwrap();
            let expected = srgba(&color_protocol(&sample.color), 1.0)
                .map(|channel| (channel * 255.0).round() as u8);
            let difference = actual
                .into_iter()
                .zip(expected)
                .map(|(actual, expected)| actual.abs_diff(expected))
                .max()
                .unwrap_or(0);
            assert!(
                difference <= sample.tolerance,
                "{} sample ({x}, {y}) differs by {difference}: {actual:?} != {expected:?}",
                scenario.id
            );
        }
        for relation in &checkpoint.relations {
            let first = pixel_at(
                &pixels,
                checkpoint.logical_size,
                relation.first,
                &scenario.id,
            );
            let second = pixel_at(
                &pixels,
                checkpoint.logical_size,
                relation.second,
                &scenario.id,
            );
            let first_luminance = luminance(first);
            let second_luminance = luminance(second);
            let minimum = u32::from(relation.minimum_difference);
            let matches = match relation.relation {
                PixelRelationKind::Lighter => first_luminance >= second_luminance + minimum,
                PixelRelationKind::Darker => first_luminance + minimum <= second_luminance,
            };
            assert!(
                matches,
                "{} relation {:?}: {first:?} ({first_luminance}) vs {second:?} ({second_luminance})",
                scenario.id, relation.relation
            );
        }
    }
}

fn pixel_at(pixels: &[u8], size: [u32; 2], point: [f32; 2], id: &str) -> [u8; 4] {
    let [width, height] = size;
    let x = point[0].floor() as u32;
    let y = point[1].floor() as u32;
    assert!(
        x < width && y < height,
        "{id} sample is outside the surface"
    );
    let offset = ((y * width + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn luminance([red, green, blue, _]: [u8; 4]) -> u32 {
    (u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114) / 1000
}

#[test]
fn every_manifest_case_required_by_desktop_executes() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/host-conformance");
    let (_, cases) = load_required(&root, Host::Desktop).expect("load shared Desktop fixtures");
    assert!(!cases.is_empty());
    for LoadedCase { manifest, scenario } in cases {
        if scenario.reference.is_some() {
            assert!(
                manifest.checkpoints.iter().any(|value| value == "pixel"),
                "{} reftest must require a pixel checkpoint",
                scenario.id
            );
            run_reftest(&scenario);
        } else {
            run_pixel_assertions(&scenario);
        }
    }
}

#[test]
fn render_taffy_protocol_and_desktop_box_paint_compose() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface_id = SurfaceId::new(17).expect("test surface");
    let surface = SurfaceRuntime::new(surface_id, StyleEnvironment::new(100.0, 100.0, 1.0, 14.0));
    let _root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(100))
                    .height(px(100))
                    .background_color(Color::rgb(0, 255, 255))
                    .border_top_width(px(10))
                    .border_right_width(px(10))
                    .border_bottom_width(px(10))
                    .border_left_width(px(10))
                    .border_top_color(Color::rgb(0, 0, 0))
                    .border_right_color(Color::rgb(0, 0, 0))
                    .border_bottom_color(Color::rgb(0, 0, 0))
                    .border_left_color(Color::rgb(0, 0, 0))
                    .border_top_style(BorderStyle::Solid)
                    .border_right_style(BorderStyle::Solid)
                    .border_bottom_style(BorderStyle::Solid)
                    .border_left_style(BorderStyle::Solid)
                    .border_top_left_radius(px(60))
                    .border_top_right_radius(px(150))
                    .border_bottom_right_radius(px(30))
                    .border_bottom_left_radius(px(30)))
            }
        });
        set_root(root);
        root
    });

    let registrations = surface.element_registrations();
    let mut measurement = NativeTextHost::new(
        DesktopElementRegistry::bind(&registrations, &built_in_element_factories()).unwrap(),
    );
    let mut scene = DesktopScene::new(
        surface_id,
        DesktopElementRegistry::bind(&registrations, &built_in_element_factories()).unwrap(),
    );
    let frame = surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut measurement,
            &mut scene,
            whisker_engine::LayoutOptions::default(),
        )
        .expect("render!, Taffy, protocol, and Desktop scene compose");
    assert!(frame.layout.has_layout());
    assert!(frame.presentation.is_some());

    let mut primitives = Vec::new();
    for command in scene.paint_commands() {
        if let PaintCommand::Box {
            rect,
            paint,
            opacity,
            ..
        } = command
        {
            assert_close(rect.width, 100.0, "Taffy border-box width");
            assert_close(rect.height, 100.0, "Taffy border-box height");
            lower_box(rect, paint, opacity, |primitive| {
                primitives.push(primitive);
            });
        }
    }
    assert_eq!(primitives.len(), 2);
    let border = primitives
        .iter()
        .find(|primitive| primitive.kind == BoxPrimitiveKind::Border)
        .expect("solid border primitive");
    assert_close(
        border.outer_radii_x[0],
        100.0 * 60.0 / 210.0,
        "normalized top-left radius",
    );
    assert_close(
        border.outer_radii_x[1],
        100.0 * 150.0 / 210.0,
        "normalized top-right radius",
    );

    let pixels = pollster::block_on(render_box_primitives_offscreen(&primitives, [100, 100]))
        .expect("production Desktop box pipeline renders offscreen");
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));

    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn capability_checklist_covers_each_standard_registry_property_once() {
    let checklist: serde_json::Value =
        serde_json::from_str(CAPABILITIES).expect("valid capability checklist JSON");
    assert_eq!(checklist["schema"], 1);
    assert_eq!(checklist["target"]["feature_count"], 175);
    assert_eq!(checklist["target"]["property_count"], 174);
    let statuses = checklist["statuses"]
        .as_array()
        .expect("status vocabulary")
        .iter()
        .map(|status| status.as_str().expect("string status"))
        .collect::<std::collections::BTreeSet<_>>();
    let mut actual = std::collections::BTreeSet::new();
    let mut features = std::collections::BTreeSet::new();
    for capability in checklist["capabilities"]
        .as_array()
        .expect("capability entries")
    {
        for property in capability["properties"]
            .as_array()
            .expect("capability properties")
        {
            assert!(
                actual.insert(property.as_str().expect("property name")),
                "duplicate property in capability checklist: {property}"
            );
        }
        if let Some(capability_features) = capability["features"].as_array() {
            for feature in capability_features {
                assert!(
                    features.insert(feature.as_str().expect("feature name")),
                    "duplicate non-property feature in capability checklist: {feature}"
                );
            }
        }
        for host in ["desktop", "web", "android", "ios"] {
            let status = capability["hosts"][host]
                .as_str()
                .expect("Host capability status");
            assert!(statuses.contains(status), "unknown status {status}");
        }
    }

    let expected = StyleProperty::ALL
        .iter()
        .filter(|property| property.metadata().origin == PropertyOrigin::Css)
        .map(|property| property.css_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 174);
    assert_eq!(
        features,
        std::collections::BTreeSet::from(["custom-properties"])
    );
    assert_eq!(actual.len() + features.len(), 175);
}
