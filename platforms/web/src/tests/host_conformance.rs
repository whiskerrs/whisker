use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use whisker::ElementRegistry;
use whisker_engine::FrameSink;
use whisker_host_conformance::{
    BorderFixture, BorderStyleFixture, ColorFixture, Command, Manifest, SCHEMA_VERSION, Scenario,
    ScenarioSide,
};
use whisker_protocol::{
    BorderLineStyle, BoxPaint, FrameHeader, FrameMode, FramePacket, LayoutGeometry, LayoutRect,
    NodeId, Operation, PaintColor, PaintCornerRadius, PaintCorners, PaintEdges,
    PaintLengthPercentage, ProtocolVersion, SurfaceId,
};

use crate::module_api::built_in_element_factories;
use crate::scene::frame_sink::DomFrameSink;

const MANIFEST: &str = include_str!("../../../../tests/host-conformance/manifest.json");

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Driver {
    root: web_sys::Element,
    sink: DomFrameSink,
    expected_box: Option<ExpectedBox>,
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
                }
                Command::Checkpoint { name, .. } => {
                    assert_eq!(name, "paint.box");
                    self.assert_box_is_projected();
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

        let (widths, styles, radii) = expected.border.as_ref().map_or(
            ([0.0; 4], [BorderStyleFixture::None; 4], [0.0; 4]),
            |border| (border.widths, border.styles, border.radii),
        );
        for (index, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
            assert_style(
                &style,
                &format!("border-{side}-width"),
                &fixture_px(widths[index]),
            );
            assert_style(
                &style,
                &format!("border-{side}-color"),
                &expected.border.as_ref().map_or_else(
                    || "rgba(0, 0, 0, 1)".to_owned(),
                    |border| fixture_color_css(&border.colors[index]),
                ),
            );
            assert_style(
                &style,
                &format!("border-{side}-style"),
                fixture_border_style_css(styles[index]),
            );
        }
        for (index, corner) in ["top-left", "top-right", "bottom-right", "bottom-left"]
            .iter()
            .enumerate()
        {
            let radius = fixture_px(radii[index]);
            assert_style(
                &style,
                &format!("border-{corner}-radius"),
                &format!("{radius} {radius}"),
            );
        }
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
        if !scenario
            .test
            .commands
            .iter()
            .any(|command| matches!(command, Command::PresentBox { .. }))
        {
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
        "wpt/css/css-backgrounds/border-radius-sum-of-radii-001.json" => include_str!(
            "../../../../tests/host-conformance/wpt/css/css-backgrounds/border-radius-sum-of-radii-001.json"
        ),
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
    let radii = border.radii.map(|length| {
        PaintCornerRadius::circular(PaintLengthPercentage {
            length,
            fraction: 0.0,
        })
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
