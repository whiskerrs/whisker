use serde::Deserialize;
use whisker::SurfaceRuntime;
use whisker::css::BorderStyle;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{set_root, with_installed_renderer};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::{FrameSink, MeasurementHost};
use whisker_protocol::{
    AvailableSpace, BorderLineStyle, BoxPaint, ElementTypeId, FrameHeader, FrameMode, FramePacket,
    InputEvent, InputEventKind, InputPoint, LayoutGeometry, LayoutRect, MeasureConstraints,
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWrap, MeasurementKey, MeasurementPayload, MeasurementRequest,
    MeasurementResponse, NodeId, Operation, PaintColor, PaintCorners, PaintEdges,
    PaintLengthPercentage, PointerId, PointerInput, PointerKind, ProtocolValue, ProtocolVersion,
    SurfaceId, TextMeasurePayload, TextMeasureStyle,
};
use whisker_style::StyleEnvironment;

use crate::gpu::render_box_primitives_offscreen;
use crate::paint::box_paint::{BoxPrimitive, BoxPrimitiveKind, lower_box};
use crate::scene::{DesktopScene, PaintCommand};
use crate::text::NativeTextHost;

const BACKGROUND_COLOR_129: &str = include_str!(
    "../../../../tests/host-conformance/wpt/css/CSS2/backgrounds/background-color-129.json"
);
const BORDER_RADIUS_SUM_001: &str = include_str!(
    "../../../../tests/host-conformance/wpt/css/css-backgrounds/border-radius-sum-of-radii-001.json"
);
const TEXT_MEASURE_BASIC: &str =
    include_str!("../../../../tests/host-conformance/core/text-measure-basic.json");
const POINTER_INPUT_BASIC: &str =
    include_str!("../../../../tests/host-conformance/core/pointer-input-basic.json");
const MANIFEST: &str = include_str!("../../../../tests/host-conformance/manifest.json");

#[derive(Debug, Deserialize)]
struct Manifest {
    schema: u32,
    wpt_revision: String,
    cases: Vec<ManifestCase>,
}

#[derive(Debug, Deserialize)]
struct ManifestCase {
    id: String,
    feature: String,
    fixture: String,
    required_hosts: Vec<String>,
    checkpoints: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    schema: u32,
    id: String,
    upstream: Upstream,
    test: ScenarioSide,
    reference: ScenarioSide,
}

#[derive(Debug, Deserialize)]
struct Upstream {
    repository: String,
    revision: String,
    path: String,
    reference_path: Option<String>,
    license: String,
    assertion: String,
    adaptation: String,
}

#[derive(Debug, Deserialize)]
struct ScenarioSide {
    commands: Vec<Command>,
}

#[derive(Debug, Deserialize)]
struct CoreScenario {
    schema: u32,
    id: String,
    commands: Vec<Command>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Command {
    AttachSurface {
        width: f32,
        height: f32,
        scale: f32,
    },
    PresentBox {
        revision: u64,
        rect: [f32; 4],
        background: ColorFixture,
        #[serde(default)]
        border: Option<BorderFixture>,
    },
    Checkpoint {
        name: String,
    },
    MeasureText {
        key: u64,
        text: String,
        font_size: f32,
        line_height: f32,
        available_width: f32,
    },
    CheckpointMeasurement {
        key: u64,
        min_width: f32,
        max_width: f32,
        min_height: f32,
        max_height: f32,
        prepared_content: bool,
    },
    EmitPointer {
        event: PointerEventFixture,
        pointer_id: u64,
        timestamp_ms: f64,
        x: f32,
        y: f32,
        buttons: u32,
        changed_button: i16,
    },
    CheckpointInput {
        event: PointerEventFixture,
        pointer_id: u64,
        x: f32,
        y: f32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PointerEventFixture {
    Down,
    Move,
    Up,
    Cancel,
}

impl PointerEventFixture {
    const fn protocol(self) -> InputEventKind {
        match self {
            Self::Down => InputEventKind::PointerDown,
            Self::Move => InputEventKind::PointerMove,
            Self::Up => InputEventKind::PointerUp,
            Self::Cancel => InputEventKind::PointerCancel,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ColorFixture {
    Named {
        value: String,
    },
    Srgba {
        red: u8,
        green: u8,
        blue: u8,
        alpha: f32,
    },
}

impl ColorFixture {
    fn protocol(&self) -> PaintColor {
        match self {
            Self::Named { value } => PaintColor::Named(value.clone()),
            Self::Srgba {
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
}

#[derive(Debug, Deserialize)]
struct BorderFixture {
    /// Top, right, bottom, left widths.
    widths: [f32; 4],
    /// Top, right, bottom, left colors.
    colors: [ColorFixture; 4],
    /// Top, right, bottom, left styles.
    styles: [BorderStyleFixture; 4],
    /// Top-left, top-right, bottom-right, bottom-left radii.
    radii: [f32; 4],
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BorderStyleFixture {
    None,
    Hidden,
    Solid,
}

impl BorderStyleFixture {
    const fn protocol(self) -> BorderLineStyle {
        match self {
            Self::None => BorderLineStyle::None,
            Self::Hidden => BorderLineStyle::Hidden,
            Self::Solid => BorderLineStyle::Solid,
        }
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
}

impl Driver {
    fn new() -> Self {
        Self {
            surface: None,
            scene: None,
            logical_size: [0.0; 2],
            scale: 1.0,
            text: NativeTextHost::new(),
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
                    self.scene = Some(DesktopScene::new(surface));
                    self.logical_size = [*width, *height];
                    self.scale = *scale;
                }
                Command::PresentBox {
                    revision,
                    rect,
                    background,
                    border,
                } => self.present_box(*revision, *rect, background, border.as_ref()),
                Command::Checkpoint { name } => {
                    assert_eq!(name, "paint.box", "unsupported Desktop checkpoint");
                    checkpoints.push(Checkpoint {
                        logical_size: [
                            self.logical_size[0].round() as u32,
                            self.logical_size[1].round() as u32,
                        ],
                        primitives: self.box_primitives(),
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
                    element_type: ElementTypeId::new(1).unwrap(),
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
                    paint: box_paint(background, border),
                },
            ],
        };
        self.scene
            .as_mut()
            .expect("attached Desktop scene")
            .present(&packet)
            .expect("canonical Host scenario packet is valid");
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
            element_type: ElementTypeId::new(1).unwrap(),
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
            kind: kind.protocol(),
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
            detail: ProtocolValue::Null,
        });
    }

    fn check_input(&self, kind: PointerEventFixture, pointer_id: u64, position: [f32; 2]) {
        let event = self
            .input
            .events
            .last()
            .expect("input checkpoint follows event");
        assert_eq!(event.kind, kind.protocol());
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
            background_color: background.protocol(),
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
                top_left: zero,
                top_right: zero,
                bottom_right: zero,
                bottom_left: zero,
            },
        };
    };
    let lengths = border.widths.map(|length| PaintLengthPercentage {
        length,
        fraction: 0.0,
    });
    let radii = border.radii.map(|length| PaintLengthPercentage {
        length,
        fraction: 0.0,
    });
    BoxPaint {
        background_color: background.protocol(),
        border_widths: PaintEdges {
            top: lengths[0],
            right: lengths[1],
            bottom: lengths[2],
            left: lengths[3],
        },
        border_colors: PaintEdges {
            top: border.colors[0].protocol(),
            right: border.colors[1].protocol(),
            bottom: border.colors[2].protocol(),
            left: border.colors[3].protocol(),
        },
        border_styles: PaintEdges {
            top: border.styles[0].protocol(),
            right: border.styles[1].protocol(),
            bottom: border.styles[2].protocol(),
            left: border.styles[3].protocol(),
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

fn assert_primitive_shape_eq(test: &BoxPrimitive, reference: &BoxPrimitive) {
    assert_eq!(test.kind, reference.kind);
    for (name, left, right) in [
        ("outer rect x", test.outer_rect.x, reference.outer_rect.x),
        ("outer rect y", test.outer_rect.y, reference.outer_rect.y),
        (
            "outer rect width",
            test.outer_rect.width,
            reference.outer_rect.width,
        ),
        (
            "outer rect height",
            test.outer_rect.height,
            reference.outer_rect.height,
        ),
        ("inner rect x", test.inner_rect.x, reference.inner_rect.x),
        ("inner rect y", test.inner_rect.y, reference.inner_rect.y),
        (
            "inner rect width",
            test.inner_rect.width,
            reference.inner_rect.width,
        ),
        (
            "inner rect height",
            test.inner_rect.height,
            reference.inner_rect.height,
        ),
    ] {
        assert_close(left, right, name);
    }
    for (index, (left, right)) in test
        .outer_radii_x
        .iter()
        .zip(reference.outer_radii_x.iter())
        .enumerate()
    {
        assert_close(*left, *right, &format!("outer radius x {index}"));
    }
    for (index, (left, right)) in test
        .outer_radii_y
        .iter()
        .zip(reference.outer_radii_y.iter())
        .enumerate()
    {
        assert_close(*left, *right, &format!("outer radius y {index}"));
    }
    for (index, (left, right)) in test
        .inner_radii_x
        .iter()
        .zip(reference.inner_radii_x.iter())
        .enumerate()
    {
        assert_close(*left, *right, &format!("inner radius x {index}"));
    }
    for (index, (left, right)) in test
        .inner_radii_y
        .iter()
        .zip(reference.inner_radii_y.iter())
        .enumerate()
    {
        assert_close(*left, *right, &format!("inner radius y {index}"));
    }
    for (index, (left, right)) in test
        .border_widths
        .iter()
        .zip(reference.border_widths.iter())
        .enumerate()
    {
        assert_close(*left, *right, &format!("border width {index}"));
    }
    for (index, (left, right)) in test.color.iter().zip(reference.color.iter()).enumerate() {
        assert_close(*left, *right, &format!("fill color {index}"));
    }
}

fn run_reftest(json: &str) {
    let scenario: Scenario = serde_json::from_str(json).expect("valid Host scenario JSON");
    assert_eq!(scenario.schema, 1);
    assert!(scenario.id.starts_with("wpt."));
    assert_eq!(
        scenario.upstream.repository,
        "https://github.com/web-platform-tests/wpt"
    );
    assert_eq!(
        scenario.upstream.revision,
        "db80bd24a77f1b5f8ba40a5b320dec3720a37c8d"
    );
    assert!(!scenario.upstream.path.is_empty());
    assert_eq!(scenario.upstream.license, "BSD-3-Clause");
    assert!(!scenario.upstream.assertion.is_empty());
    assert!(!scenario.upstream.adaptation.is_empty());
    if let Some(reference) = &scenario.upstream.reference_path {
        assert!(!reference.is_empty());
    }

    let test = Driver::new().execute(&scenario.test);
    let reference = Driver::new().execute(&scenario.reference);
    assert_eq!(test.len(), 1, "one test checkpoint");
    assert_eq!(reference.len(), 1, "one reference checkpoint");
    assert_eq!(test[0].logical_size, reference[0].logical_size);
    assert_eq!(test[0].primitives.len(), reference[0].primitives.len());
    for (test, reference) in test[0].primitives.iter().zip(&reference[0].primitives) {
        assert_primitive_shape_eq(test, reference);
    }

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

#[test]
fn wpt_background_color_129_matches_reference() {
    run_reftest(BACKGROUND_COLOR_129);
}

#[test]
fn wpt_border_radius_sum_of_radii_001_matches_reference() {
    run_reftest(BORDER_RADIUS_SUM_001);
}

#[test]
fn core_text_measurement_runs_without_runtime_instance() {
    let scenario: CoreScenario =
        serde_json::from_str(TEXT_MEASURE_BASIC).expect("valid core Host scenario JSON");
    assert_eq!(scenario.schema, 1);
    assert_eq!(scenario.id, "host.measure.text.basic");
    Driver::new().execute(&ScenarioSide {
        commands: scenario.commands,
    });
}

#[test]
fn core_pointer_input_reaches_mock_runtime_sink() {
    let scenario: CoreScenario =
        serde_json::from_str(POINTER_INPUT_BASIC).expect("valid core Host scenario JSON");
    assert_eq!(scenario.schema, 1);
    assert_eq!(scenario.id, "host.input.pointer.basic");
    Driver::new().execute(&ScenarioSide {
        commands: scenario.commands,
    });
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

    let mut measurement = NativeTextHost::new();
    let mut scene = DesktopScene::new(surface_id);
    let frame = surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut measurement,
            &mut scene,
            whisker_engine::HostLayoutOptions::default(),
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
fn manifest_assigns_every_seed_case_to_desktop() {
    let manifest: Manifest = serde_json::from_str(MANIFEST).expect("valid Host manifest JSON");
    assert_eq!(manifest.schema, 1);
    assert_eq!(
        manifest.wpt_revision,
        "db80bd24a77f1b5f8ba40a5b320dec3720a37c8d"
    );
    let expected = [
        "wpt.css2.backgrounds.background-color-129",
        "wpt.css-backgrounds.border-radius-sum-of-radii-001.test1",
        "host.measure.text.basic",
        "host.input.pointer.basic",
    ];
    assert_eq!(manifest.cases.len(), expected.len());
    for (case, expected) in manifest.cases.iter().zip(expected) {
        assert_eq!(case.id, expected);
        assert!(!case.feature.is_empty());
        assert!(case.fixture.ends_with(".json"));
        assert!(case.required_hosts.iter().any(|host| host == "desktop"));
        assert!(!case.checkpoints.is_empty());
    }
}
