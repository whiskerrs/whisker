use base64::Engine;
use whisker::css::BorderStyle;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{set_root, with_installed_renderer};
use whisker::{SurfaceRuntime, standard_element_registrations};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::{FrameSink, MeasurementProvider};
use whisker_host_conformance::{
    BackgroundBoxFixture, BackgroundImageFixture, BackgroundLayerFixture, BackgroundSizeFixture,
    BackgroundSizeKeywordFixture, BorderFixture, BorderStyleFixture, ClipPathFixture,
    ClipReferenceBoxFixture, ClipShapeFixture, ColorFixture, Command, ConicGradientFixture,
    CursorFixture, FillRuleFixture, Host, ImageRenderingFixture, ImageRepeatFixture,
    LinearGradientFixture, LoadedCase, OverflowClipFixture, PathCommandFixture,
    PixelRelationFixture, PixelRelationKind, PixelSampleFixture, PointerEventFixture,
    PointerEventsFixture, PointerKindFixture, RadialGradientFixture, ResourceSourceFixture,
    ResourceStateFixture, Scenario, ScenarioSide, SceneNodeFixture, VisibilityFixture,
    load_required,
};
use whisker_protocol::{
    AvailableSpace, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode,
    BorderLineStyle, BoxClip, BoxPaint, ClipShape, Cursor, CursorKeyword, ElementTypeId, FillRule,
    FrameHeader, FrameMode, FramePacket, GradientStop, HitTestBehavior, ImageRendering,
    ImageRepeat, InputEvent, InputEventKind, InputPoint, LayoutGeometry, LayoutRect,
    MeasureConstraints, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
    MeasureTextDirection, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap,
    MeasurementKey, MeasurementPayload, MeasurementRequest, MeasurementResponse, NodeId, Operation,
    OverflowClip, PaintBox, PaintColor, PaintCoordinate, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintImage, PaintLengthPercentage, PaintPosition, PathCommand, PointerKind,
    ProtocolVersion, RadialGradientExtent, RadialGradientShape, ResourceCommand, ResourceId,
    ResourceKind, ResourceRequest, ResourceSource, SurfaceId, TextContent, TextMeasurePayload,
    TextMeasureStyle, TextPaint, TextShadow, Transform, Visibility,
};
use whisker_style::{PropertyOrigin, StyleEnvironment, StyleProperty};

use crate::element::{DesktopElementRegistry, built_in_element_factories};
use crate::gpu::{
    BackdropCheckpoint, ClippedBoxPrimitive, RasterResource, background_gradient_draw,
    background_resource_draw, render_box_primitives_offscreen,
    render_clipped_box_primitives_offscreen,
    render_clipped_box_primitives_with_backdrops_offscreen,
};
use crate::input::{DesktopPointerAdapter, DesktopPointerEvent, DesktopPointerPhase};
use crate::paint::box_paint::{
    BoxPrimitiveKind, background_gradient_primitive, lower_box, resolve_box_geometry,
};
use crate::paint::color::srgba;
use crate::resource::{DesktopResourceService, DesktopResourceState, DesktopResourceUpdate};
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

const fn pointer_kind_protocol(value: PointerKindFixture) -> PointerKind {
    match value {
        PointerKindFixture::Mouse => PointerKind::Mouse,
        PointerKindFixture::Touch => PointerKind::Touch,
        PointerKindFixture::Pen => PointerKind::Pen,
        PointerKindFixture::Unknown => PointerKind::Unknown,
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

fn cursor_protocol(value: CursorFixture) -> Cursor {
    Cursor {
        resources: Vec::new(),
        fallback: match value {
            CursorFixture::Auto => CursorKeyword::Auto,
            CursorFixture::Pointer => CursorKeyword::Pointer,
            CursorFixture::Text => CursorKeyword::Text,
            CursorFixture::Grab => CursorKeyword::Grab,
            CursorFixture::None => CursorKeyword::None,
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
                font_style: match text.font_style {
                    whisker_host_conformance::FontStyleFixture::Normal => MeasureFontStyle::Normal,
                    whisker_host_conformance::FontStyleFixture::Italic => MeasureFontStyle::Italic,
                    whisker_host_conformance::FontStyleFixture::Oblique => {
                        MeasureFontStyle::Oblique
                    }
                },
                line_height: text
                    .line_height
                    .map_or(MeasureLineHeight::Normal, |height| {
                        MeasureLineHeight::LogicalPixels(height)
                    }),
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
            foreground: color_protocol(&text.color),
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
                    color: color_protocol(&decoration.color),
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
                    color: color_protocol(&shadow.color),
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

fn apply_background_geometry(
    mut layer: BackgroundLayer,
    value: &BackgroundLayerFixture,
) -> BackgroundLayer {
    let length = |value: whisker_host_conformance::LengthPercentageFixture| PaintLengthPercentage {
        length: value.length,
        fraction: value.fraction,
    };
    let coordinate = |value: whisker_host_conformance::LengthPercentageFixture| PaintCoordinate {
        length: value.length,
        fraction: value.fraction,
    };
    let repeat = |value| match value {
        ImageRepeatFixture::Repeat => ImageRepeat::Repeat,
        ImageRepeatFixture::NoRepeat => ImageRepeat::NoRepeat,
        ImageRepeatFixture::Space => ImageRepeat::Space,
        ImageRepeatFixture::Round => ImageRepeat::Round,
    };
    let paint_box = |value| match value {
        BackgroundBoxFixture::Border => PaintBox::Border,
        BackgroundBoxFixture::Padding => PaintBox::Padding,
        BackgroundBoxFixture::Content => PaintBox::Content,
        BackgroundBoxFixture::BorderArea => PaintBox::BorderArea,
    };
    layer.position = PaintPosition {
        x: coordinate(value.position[0]),
        y: coordinate(value.position[1]),
    };
    layer.size = match value.size {
        BackgroundSizeFixture::ExplicitPair(size) => BackgroundSize::Explicit {
            width: Some(length(size[0])),
            height: Some(length(size[1])),
        },
        BackgroundSizeFixture::ExplicitAxes { width, height } => BackgroundSize::Explicit {
            width: width.map(length),
            height: height.map(length),
        },
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Auto) => BackgroundSize::Auto,
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Cover) => {
            BackgroundSize::Cover
        }
        BackgroundSizeFixture::Keyword(BackgroundSizeKeywordFixture::Contain) => {
            BackgroundSize::Contain
        }
    };
    layer.repeat_x = repeat(value.repeat_x);
    layer.repeat_y = repeat(value.repeat_y);
    layer.origin = paint_box(value.origin);
    layer.clip = paint_box(value.clip);
    layer
}

fn linear_gradient_protocol(
    value: &LinearGradientFixture,
    geometry: &BackgroundLayerFixture,
) -> BackgroundLayer {
    apply_background_geometry(
        BackgroundLayer {
            image: PaintImage::LinearGradient {
                angle_degrees: value.angle_degrees,
                repeating: value.repeating,
                stops: value
                    .stops
                    .iter()
                    .map(|stop| GradientStop {
                        color: color_protocol(&stop.color),
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
        },
        geometry,
    )
}

fn radial_gradient_protocol(
    value: &RadialGradientFixture,
    geometry: &BackgroundLayerFixture,
) -> BackgroundLayer {
    apply_background_geometry(
        BackgroundLayer {
            image: PaintImage::RadialGradient {
                shape: RadialGradientShape::Ellipse,
                extent: RadialGradientExtent::Explicit,
                center: PaintPosition {
                    x: PaintCoordinate {
                        length: value.center[0],
                        fraction: 0.0,
                    },
                    y: PaintCoordinate {
                        length: value.center[1],
                        fraction: 0.0,
                    },
                },
                radii: Some((
                    PaintLengthPercentage {
                        length: value.radii[0],
                        fraction: 0.0,
                    },
                    PaintLengthPercentage {
                        length: value.radii[1],
                        fraction: 0.0,
                    },
                )),
                repeating: false,
                stops: value
                    .stops
                    .iter()
                    .map(|stop| GradientStop {
                        color: color_protocol(&stop.color),
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
        },
        geometry,
    )
}

fn conic_gradient_protocol(
    value: &ConicGradientFixture,
    geometry: &BackgroundLayerFixture,
) -> BackgroundLayer {
    apply_background_geometry(
        BackgroundLayer {
            image: PaintImage::ConicGradient {
                from_degrees: value.from_degrees,
                center: PaintPosition {
                    x: PaintCoordinate {
                        length: value.center[0],
                        fraction: 0.0,
                    },
                    y: PaintCoordinate {
                        length: value.center[1],
                        fraction: 0.0,
                    },
                },
                repeating: false,
                stops: value
                    .stops
                    .iter()
                    .map(|stop| GradientStop {
                        color: color_protocol(&stop.color),
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
        },
        geometry,
    )
}

fn background_image_protocol(
    image: &BackgroundImageFixture,
    geometry: &BackgroundLayerFixture,
) -> BackgroundLayer {
    match image {
        BackgroundImageFixture::LinearGradient(gradient) => {
            linear_gradient_protocol(gradient, geometry)
        }
        BackgroundImageFixture::RadialGradient(gradient) => {
            radial_gradient_protocol(gradient, geometry)
        }
        BackgroundImageFixture::ConicGradient(gradient) => {
            conic_gradient_protocol(gradient, geometry)
        }
        BackgroundImageFixture::Resource(resource) => apply_background_geometry(
            BackgroundLayer {
                image: PaintImage::Resource(
                    ResourceId::new(*resource).expect("validated fixture resource id"),
                ),
                position: PaintPosition::default(),
                size: BackgroundSize::Auto,
                repeat_x: ImageRepeat::Repeat,
                repeat_y: ImageRepeat::Repeat,
                origin: PaintBox::Padding,
                clip: PaintBox::Border,
                attachment: BackgroundAttachment::Scroll,
                blend_mode: BlendMode::Normal,
            },
            geometry,
        ),
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
    pointer: Option<DesktopPointerAdapter>,
    input: RecordingInputSink,
    raster_resources: std::collections::HashMap<ResourceId, RasterResource>,
    resource_service: DesktopResourceService,
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
    primitives: Vec<ClippedBoxPrimitive>,
    backdrops: Vec<BackdropCheckpoint>,
    raster_resources: std::collections::HashMap<ResourceId, RasterResource>,
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
            pointer: None,
            input: RecordingInputSink::default(),
            raster_resources: std::collections::HashMap::new(),
            resource_service: DesktopResourceService::new(std::path::PathBuf::new(), || {}),
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
                    self.pointer = Some(DesktopPointerAdapter::new(surface));
                    self.scene = Some(desktop_scene(surface));
                    self.logical_size = [*width, *height];
                    self.scale = *scale;
                }
                Command::RegisterRasterResource {
                    id,
                    width,
                    height,
                    pixels,
                } => {
                    assert_eq!(pixels.len(), (*width * *height) as usize);
                    let resource = ResourceId::new(*id).expect("validated fixture resource id");
                    let rgba = pixels
                        .iter()
                        .flat_map(|color| {
                            srgba(&color_protocol(color), 1.0)
                                .map(|channel| (channel * 255.0).round() as u8)
                        })
                        .collect();
                    let raster = RasterResource::new(*width, *height, rgba)
                        .expect("validated fixture raster");
                    self.scene
                        .as_mut()
                        .expect("attach_surface precedes resource registration")
                        .register_raster_resource(resource);
                    self.raster_resources.insert(resource, raster);
                }
                Command::LoadRasterResource {
                    id,
                    generation,
                    source,
                } => {
                    let source = match source {
                        ResourceSourceFixture::Url { value } => ResourceSource::Url(value.clone()),
                        ResourceSourceFixture::Bytes { media_type, base64 } => {
                            ResourceSource::Bytes {
                                media_type: media_type.clone(),
                                data: base64::engine::general_purpose::STANDARD
                                    .decode(base64)
                                    .expect("validated fixture base64"),
                            }
                        }
                    };
                    self.resource_service
                        .command(ResourceCommand::Load(ResourceRequest {
                            resource: ResourceId::new(*id).unwrap(),
                            generation: *generation,
                            kind: ResourceKind::RasterImage,
                            source,
                        }))
                        .expect("valid Desktop resource load");
                }
                Command::ReleaseRasterResource { id, generation } => {
                    let updates = self
                        .resource_service
                        .command(ResourceCommand::Release {
                            resource: ResourceId::new(*id).unwrap(),
                            generation: *generation,
                        })
                        .expect("valid Desktop resource release");
                    self.apply_resource_updates(updates);
                }
                Command::CheckpointResource {
                    id,
                    generation,
                    state,
                    width,
                    height,
                } => self.check_resource(*id, *generation, *state, *width, *height),
                Command::PresentBox {
                    revision,
                    rect,
                    background,
                    border,
                } => self.present_box(*revision, *rect, background, border.as_ref()),
                Command::PresentScene { revision, nodes } => self.present_scene(*revision, nodes),
                Command::Checkpoint {
                    name,
                    samples,
                    relations,
                } => {
                    assert!(
                        name.starts_with("paint.") || name == "interaction.pointer.lynx",
                        "unsupported Desktop checkpoint"
                    );
                    if name == "paint.text.shadow-single" {
                        self.assert_text_shadow();
                    } else if name == "paint.text.decoration-lynx" {
                        self.assert_text_decoration();
                    } else if name == "paint.text.align-lynx" {
                        self.assert_text_alignment();
                    } else if name == "paint.text.indent-lynx" {
                        self.assert_text_indent();
                    } else if name == "paint.text.wrap-overflow-lynx" {
                        self.assert_text_wrap_overflow();
                    } else if name == "paint.text.font-features-lynx" {
                        self.assert_text_font_features();
                    } else if name == "paint.text.basic-style-lynx" {
                        self.assert_text_basic_style();
                    }
                    checkpoints.push(Checkpoint {
                        logical_size: [
                            self.logical_size[0].round() as u32,
                            self.logical_size[1].round() as u32,
                        ],
                        primitives: self.clipped_box_primitives(),
                        backdrops: self.backdrop_checkpoints(),
                        raster_resources: self.raster_resources.clone(),
                        samples: samples.clone(),
                        relations: relations.clone(),
                    });
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
                    font_features,
                    font_variations,
                    font_optical_sizing,
                    white_space,
                    word_break,
                    max_lines,
                    overflow,
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
                    font_features,
                    font_variations,
                    *font_optical_sizing,
                    *white_space,
                    *word_break,
                    *max_lines,
                    *overflow,
                    *available_width,
                ),
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
                    pointer_kind,
                    pointer_id,
                    timestamp_ms,
                    x,
                    y,
                    buttons,
                    changed_button,
                } => self.emit_pointer(
                    *event,
                    *pointer_kind,
                    *pointer_id,
                    *timestamp_ms,
                    [*x, *y],
                    *buttons,
                    *changed_button,
                ),
                Command::CheckpointInput {
                    event,
                    pointer_kind,
                    pointer_id,
                    x,
                    y,
                } => self.check_input(*event, *pointer_kind, *pointer_id, [*x, *y]),
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
                assert_eq!(*actual_paint, Some(&expected_paint));
            }
            _ => panic!("one projected Desktop box command"),
        }
    }

    fn apply_resource_updates(&mut self, updates: Vec<DesktopResourceUpdate>) {
        for update in updates {
            match update {
                DesktopResourceUpdate::Ready { event, raster } => {
                    let whisker_protocol::ResourceEvent::Ready { resource, .. } = event else {
                        unreachable!()
                    };
                    self.scene
                        .as_mut()
                        .expect("resource load follows surface attach")
                        .register_raster_resource(resource);
                    self.raster_resources.insert(resource, raster);
                }
                DesktopResourceUpdate::Failed(_) => {}
                DesktopResourceUpdate::Released {
                    resource, evict, ..
                } => {
                    if evict {
                        self.scene
                            .as_mut()
                            .expect("resource release follows surface attach")
                            .release_raster_resource(resource);
                        self.raster_resources.remove(&resource);
                    }
                }
            }
        }
    }

    fn check_resource(
        &mut self,
        id: u64,
        generation: u64,
        expected: ResourceStateFixture,
        width: Option<u32>,
        height: Option<u32>,
    ) {
        let resource = ResourceId::new(id).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while self.resource_service.state(resource, generation)
            == Some(DesktopResourceState::Loading)
        {
            let updates = self.resource_service.drain();
            self.apply_resource_updates(updates);
            assert!(
                std::time::Instant::now() < deadline,
                "Desktop resource checkpoint timed out"
            );
            std::thread::yield_now();
        }
        let actual = self.resource_service.state(resource, generation);
        match expected {
            ResourceStateFixture::Ready => assert_eq!(
                actual,
                Some(DesktopResourceState::Ready {
                    width: width.unwrap(),
                    height: height.unwrap(),
                })
            ),
            ResourceStateFixture::Failed => {
                assert!(matches!(actual, Some(DesktopResourceState::Failed(_))))
            }
            ResourceStateFixture::Released => {
                assert_eq!(actual, Some(DesktopResourceState::Released))
            }
        }
    }

    fn present_scene(&mut self, revision: u64, nodes: &[SceneNodeFixture]) {
        let surface = self.surface.expect("attach_surface precedes present_scene");
        let mut operations = Vec::new();
        for fixture in nodes {
            operations.push(Operation::CreateNode {
                node: NodeId::new(fixture.id).expect("validated fixture node id"),
                element_type: standard_element_type(if fixture.text.is_some() {
                    whisker::TEXT_ELEMENT_NAME
                } else {
                    whisker::VIEW_ELEMENT_NAME
                }),
            });
        }
        let mut child_counts = std::collections::BTreeMap::<u64, u32>::new();
        for fixture in nodes.iter().filter(|fixture| fixture.parent.is_some()) {
            let parent = fixture.parent.unwrap();
            let index = child_counts.entry(parent).or_default();
            operations.push(Operation::InsertChild {
                parent: NodeId::new(parent).unwrap(),
                child: NodeId::new(fixture.id).unwrap(),
                index: *index,
            });
            *index += 1;
        }
        for fixture in nodes {
            let node = NodeId::new(fixture.id).unwrap();
            operations.push(Operation::SetLayout {
                node,
                geometry: LayoutGeometry {
                    border_box: layout_rect(fixture.rect),
                    content_box: layout_rect(fixture.resolved_content_box()),
                },
            });
            operations.push(Operation::SetBoxPaint {
                node,
                paint: box_paint(&fixture.background, fixture.border.as_ref()),
            });
            if let Some(text) = &fixture.text {
                operations.push(Operation::SetText {
                    node,
                    content: fixture_text_content(text),
                });
            }
            if !fixture.box_shadows.is_empty()
                || fixture.clip_path.is_some()
                || fixture.backdrop_blur.is_some()
                || fixture.image_rendering != ImageRenderingFixture::Auto
            {
                operations.push(Operation::SetVisualEffects {
                    node,
                    effects: whisker_protocol::VisualEffects {
                        box_shadows: fixture
                            .box_shadows
                            .iter()
                            .map(|shadow| whisker_protocol::BoxShadow {
                                offset_x: shadow.offset[0],
                                offset_y: shadow.offset[1],
                                blur_radius: shadow.blur_radius,
                                spread_radius: shadow.spread_radius,
                                color: color_protocol(&shadow.color),
                                inset: shadow.inset,
                            })
                            .collect(),
                        clip_path: fixture.clip_path.as_ref().map(clip_path_protocol),
                        backdrop_blur: fixture.backdrop_blur,
                        image_rendering: match fixture.image_rendering {
                            ImageRenderingFixture::Auto => ImageRendering::Auto,
                            ImageRenderingFixture::Pixelated => ImageRendering::Pixelated,
                            ImageRenderingFixture::CrispEdges => ImageRendering::CrispEdges,
                        },
                        ..Default::default()
                    },
                });
            }
            if !fixture.background_layers.is_empty() {
                operations.push(Operation::SetBackgroundLayers {
                    node,
                    layers: fixture
                        .background_layers
                        .iter()
                        .map(|layer| background_image_protocol(&layer.image, &layer.geometry))
                        .collect(),
                });
            } else if let Some(gradient) = &fixture.linear_gradient {
                operations.push(Operation::SetBackgroundLayers {
                    node,
                    layers: vec![linear_gradient_protocol(
                        gradient,
                        &fixture.background_layer,
                    )],
                });
            } else if let Some(gradient) = &fixture.radial_gradient {
                operations.push(Operation::SetBackgroundLayers {
                    node,
                    layers: vec![radial_gradient_protocol(
                        gradient,
                        &fixture.background_layer,
                    )],
                });
            } else if let Some(gradient) = &fixture.conic_gradient {
                operations.push(Operation::SetBackgroundLayers {
                    node,
                    layers: vec![conic_gradient_protocol(gradient, &fixture.background_layer)],
                });
            }
            operations.push(Operation::SetClip {
                node,
                clip: BoxClip {
                    horizontal: overflow_clip_protocol(fixture.clip.horizontal),
                    vertical: overflow_clip_protocol(fixture.clip.vertical),
                },
            });
            if let Some(transform) = fixture.transform {
                operations.push(Operation::SetTransform {
                    node,
                    transform: Transform(transform),
                });
            }
            if let Some(opacity) = fixture.opacity {
                operations.push(Operation::SetOpacity { node, opacity });
            }
            if let Some(visibility) = fixture.visibility {
                operations.push(Operation::SetVisibility {
                    node,
                    visibility: match visibility {
                        VisibilityFixture::Visible => Visibility::Visible,
                        VisibilityFixture::Hidden => Visibility::Hidden,
                    },
                });
            }
            if let Some(z_order) = fixture.z_order {
                operations.push(Operation::SetZOrder { node, z_order });
            }
            operations.push(Operation::SetCursor {
                node,
                cursor: cursor_protocol(fixture.cursor),
            });
            operations.push(Operation::SetHitTest {
                node,
                behavior: match fixture.pointer_events {
                    PointerEventsFixture::Auto => HitTestBehavior::Auto,
                    PointerEventsFixture::None => HitTestBehavior::None,
                },
            });
        }
        self.scene
            .as_mut()
            .expect("attached Desktop scene")
            .present(&FramePacket {
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
            })
            .expect("canonical Host scene fixture is valid");
    }

    fn assert_text_shadow(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let content = commands
            .iter()
            .find_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(content),
                _ => None,
            })
            .expect("text fixture creates one text paint command");
        let shadow = content
            .paint
            .shadows
            .first()
            .expect("text shadow is retained");
        assert_eq!([shadow.offset_x, shadow.offset_y], [3.0, 4.0]);
        assert_eq!(shadow.blur_radius, 2.0);
        assert_eq!(
            shadow.color,
            PaintColor::Srgba {
                red: 255,
                green: 0,
                blue: 0,
                alpha: 0.75
            }
        );
    }

    fn assert_text_font_features(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let texts = commands
            .iter()
            .filter_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(content),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(texts.len(), 3);
        assert_eq!(texts[0].payload.style.features.len(), 2);
        assert_eq!(texts[0].payload.style.features[0].tag.get(), *b"kern");
        assert_eq!(texts[0].payload.style.features[0].value, 0);
        assert_eq!(texts[0].payload.style.features[1].tag.get(), *b"liga");
        assert_eq!(texts[0].payload.style.features[1].value, 1);
        assert_eq!(texts[1].payload.style.variations.len(), 2);
        assert_eq!(texts[1].payload.style.variations[0].tag.get(), *b"wdth");
        assert_eq!(texts[1].payload.style.variations[0].value, 90.0);
        assert_eq!(texts[1].payload.style.variations[1].tag.get(), *b"wght");
        assert_eq!(texts[1].payload.style.variations[1].value, 650.0);
        assert_eq!(
            texts[2].payload.style.optical_sizing,
            whisker_protocol::FontOpticalSizing::Auto
        );
    }

    fn assert_text_basic_style(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let content = commands
            .iter()
            .find_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(content),
                _ => None,
            })
            .expect("basic style fixture creates one text paint command");

        assert_eq!(
            content.payload.style.font_families,
            vec![
                MeasureFontFamily::Named("Whisker Fixture Sans".into()),
                MeasureFontFamily::System,
            ]
        );
        assert_eq!(content.payload.style.font_size, 20.0);
        assert_eq!(content.payload.style.font_weight, 650);
        assert_eq!(content.payload.style.font_style, MeasureFontStyle::Italic);
        assert_eq!(
            content.payload.style.line_height,
            MeasureLineHeight::LogicalPixels(28.0)
        );
        assert_eq!(content.payload.style.letter_spacing, 1.5);
        assert_eq!(
            content.paint.foreground,
            PaintColor::Srgba {
                red: 32,
                green: 64,
                blue: 192,
                alpha: 1.0,
            }
        );
    }

    fn assert_text_decoration(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let decorations = commands
            .iter()
            .filter_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(&content.paint.decoration),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(decorations.len(), 5);
        assert!(decorations[0].lines.underline);
        assert_eq!(
            decorations[0].style,
            whisker_protocol::TextDecorationStyle::Solid
        );
        assert!(decorations[1].lines.underline);
        assert_eq!(
            decorations[1].style,
            whisker_protocol::TextDecorationStyle::Double
        );
        assert!(decorations[2].lines.underline);
        assert_eq!(
            decorations[2].style,
            whisker_protocol::TextDecorationStyle::Dotted
        );
        assert!(decorations[3].lines.line_through);
        assert_eq!(
            decorations[3].style,
            whisker_protocol::TextDecorationStyle::Dashed
        );
        assert!(decorations[4].lines.line_through);
        assert_eq!(
            decorations[4].style,
            whisker_protocol::TextDecorationStyle::Wavy
        );
        assert_eq!(decorations[0].color, PaintColor::Named("red".into()));
    }

    fn assert_text_alignment(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let alignments = commands
            .iter()
            .filter_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(content.payload.alignment),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            alignments,
            [
                whisker_protocol::MeasureTextAlignment::Left,
                whisker_protocol::MeasureTextAlignment::Right,
                whisker_protocol::MeasureTextAlignment::Center,
                whisker_protocol::MeasureTextAlignment::Start,
                whisker_protocol::MeasureTextAlignment::End,
            ]
        );
    }

    fn assert_text_indent(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let indents = commands
            .iter()
            .filter_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(content.payload.indent),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(indents.len(), 2);
        assert_eq!(indents[0].logical_pixels, 24.0);
        assert_eq!(indents[0].percentage, 0.0);
        assert_eq!(indents[1].logical_pixels, 0.0);
        assert_eq!(indents[1].percentage, 15.0);
        assert_eq!(indents[1].resolve(200.0), 30.0);
    }

    fn assert_text_wrap_overflow(&self) {
        let commands = self
            .scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands();
        let payloads = commands
            .iter()
            .filter_map(|command| match command {
                PaintCommand::Text { content, .. } => Some(&content.payload),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(payloads.len(), 5);
        assert_eq!(payloads[0].wrap, MeasureTextWrap::Wrap);
        assert_eq!(payloads[0].word_break, MeasureTextWordBreak::Normal);
        assert_eq!(payloads[1].wrap, MeasureTextWrap::NoWrap);
        assert_eq!(payloads[2].word_break, MeasureTextWordBreak::BreakAll);
        assert_eq!(payloads[3].word_break, MeasureTextWordBreak::KeepAll);
        assert_eq!(payloads[4].max_lines, Some(1));
        assert_eq!(payloads[4].overflow, MeasureTextOverflow::Ellipsis);
    }

    fn clipped_box_primitives(&self) -> Vec<ClippedBoxPrimitive> {
        let scene = self.scene.as_ref().expect("checkpoint follows attach");
        let mut primitives = Vec::new();
        for command in scene.paint_commands() {
            if let PaintCommand::Box {
                rect,
                content_rect,
                paint,
                background_layers,
                visual_effects,
                clip,
                shape_clips,
                transform,
                opacity,
                ..
            } = command
            {
                let default_paint = BoxPaint::default();
                let paint = paint.unwrap_or(&default_paint);
                primitives.extend(
                    visual_effects
                        .box_shadows
                        .iter()
                        .rev()
                        .filter(|shadow| !shadow.inset)
                        .filter_map(|shadow| {
                            crate::paint::box_paint::box_shadow_primitive(
                                rect, paint, shadow, opacity,
                            )
                            .map(|primitive| {
                                (
                                    primitive,
                                    clip,
                                    transform,
                                    shape_clips.clone(),
                                    None,
                                    None,
                                    false,
                                )
                            })
                        }),
                );
                let mut boxes = Vec::new();
                lower_box(rect, paint, opacity, |primitive| boxes.push(primitive));
                primitives.extend(
                    boxes
                        .iter()
                        .copied()
                        .filter(|primitive| primitive.kind == BoxPrimitiveKind::Fill)
                        .map(|primitive| {
                            (
                                primitive,
                                clip,
                                transform,
                                shape_clips.clone(),
                                None,
                                None,
                                false,
                            )
                        }),
                );
                let box_geometry = resolve_box_geometry(rect, paint);
                for layer in background_layers.iter().rev() {
                    let positioning_rect = match layer.origin {
                        PaintBox::Border => box_geometry.outer_rect,
                        PaintBox::Padding => box_geometry.inner_rect,
                        PaintBox::Content => content_rect,
                        _ => continue,
                    };
                    let (gradient, resource) = match &layer.image {
                        PaintImage::Resource(resource) => {
                            let Some(raster) = self.raster_resources.get(resource) else {
                                continue;
                            };
                            let Some((draw, resource)) = background_resource_draw(
                                positioning_rect,
                                layer,
                                [raster.width as f32, raster.height as f32],
                                opacity,
                            ) else {
                                continue;
                            };
                            (draw, Some(resource))
                        }
                        _ => {
                            let Some(draw) =
                                background_gradient_draw(positioning_rect, layer, opacity)
                            else {
                                continue;
                            };
                            (draw, None)
                        }
                    };
                    primitives.push((
                        background_gradient_primitive(rect, content_rect, paint, layer.clip),
                        clip,
                        transform,
                        shape_clips.clone(),
                        Some(gradient),
                        resource,
                        resource.is_some()
                            && matches!(
                                visual_effects.image_rendering,
                                ImageRendering::Pixelated | ImageRendering::CrispEdges
                            ),
                    ));
                }
                primitives.extend(
                    visual_effects
                        .box_shadows
                        .iter()
                        .rev()
                        .filter(|shadow| shadow.inset)
                        .filter_map(|shadow| {
                            crate::paint::box_paint::box_shadow_primitive(
                                rect, paint, shadow, opacity,
                            )
                            .map(|primitive| {
                                (
                                    primitive,
                                    clip,
                                    transform,
                                    shape_clips.clone(),
                                    None,
                                    None,
                                    false,
                                )
                            })
                        }),
                );
                primitives.extend(
                    boxes
                        .into_iter()
                        .filter(|primitive| primitive.kind == BoxPrimitiveKind::Border)
                        .map(|primitive| {
                            (
                                primitive,
                                clip,
                                transform,
                                shape_clips.clone(),
                                None,
                                None,
                                false,
                            )
                        }),
                );
            }
        }
        primitives
    }

    fn backdrop_checkpoints(&self) -> Vec<BackdropCheckpoint> {
        self.scene
            .as_ref()
            .expect("checkpoint follows attach")
            .paint_commands()
            .into_iter()
            .filter_map(|command| match command {
                PaintCommand::BackdropBlur { rect, radius, clip } => Some((rect, radius, clip)),
                _ => None,
            })
            .collect()
    }

    fn measure_text(
        &mut self,
        key: u64,
        value: &str,
        font_families: &[String],
        font_size: f32,
        font_weight: u16,
        font_style: whisker_host_conformance::FontStyleFixture,
        line_height: f32,
        letter_spacing: f32,
        font_features: &[whisker_host_conformance::FontFeatureFixture],
        font_variations: &[whisker_host_conformance::FontVariationFixture],
        font_optical_sizing: whisker_host_conformance::FontOpticalSizingFixture,
        white_space: whisker_host_conformance::WhiteSpaceFixture,
        word_break: whisker_host_conformance::WordBreakFixture,
        max_lines: u32,
        overflow: whisker_host_conformance::TextOverflowFixture,
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
                    font_style: match font_style {
                        whisker_host_conformance::FontStyleFixture::Normal => {
                            MeasureFontStyle::Normal
                        }
                        whisker_host_conformance::FontStyleFixture::Italic => {
                            MeasureFontStyle::Italic
                        }
                        whisker_host_conformance::FontStyleFixture::Oblique => {
                            MeasureFontStyle::Oblique
                        }
                    },
                    line_height: MeasureLineHeight::LogicalPixels(line_height),
                    letter_spacing,
                    features: font_features
                        .iter()
                        .map(|feature| whisker_protocol::FontFeature {
                            tag: fixture_font_tag(&feature.tag),
                            value: feature.value,
                        })
                        .collect(),
                    variations: font_variations
                        .iter()
                        .map(|variation| whisker_protocol::FontVariation {
                            tag: fixture_font_tag(&variation.tag),
                            value: variation.value,
                        })
                        .collect(),
                    optical_sizing: match font_optical_sizing {
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
                alignment: whisker_protocol::MeasureTextAlignment::Start,
                indent: Default::default(),
                wrap: match white_space {
                    whisker_host_conformance::WhiteSpaceFixture::Normal => MeasureTextWrap::Wrap,
                    whisker_host_conformance::WhiteSpaceFixture::NoWrap => MeasureTextWrap::NoWrap,
                },
                word_break: match word_break {
                    whisker_host_conformance::WordBreakFixture::Normal => {
                        MeasureTextWordBreak::Normal
                    }
                    whisker_host_conformance::WordBreakFixture::BreakAll => {
                        MeasureTextWordBreak::BreakAll
                    }
                    whisker_host_conformance::WordBreakFixture::KeepAll => {
                        MeasureTextWordBreak::KeepAll
                    }
                },
                max_lines: (max_lines > 0).then_some(max_lines),
                overflow: match overflow {
                    whisker_host_conformance::TextOverflowFixture::Clip => {
                        MeasureTextOverflow::Clip
                    }
                    whisker_host_conformance::TextOverflowFixture::Ellipsis => {
                        MeasureTextOverflow::Ellipsis
                    }
                },
            }),
        };
        self.text
            .measure_batch(surface, &[request], &mut self.measurement_responses)
            .expect("Desktop text measurement is infallible");
        if !font_features.is_empty() || !font_variations.is_empty() {
            let response = self
                .measurement_responses
                .last()
                .expect("Desktop text measurement returned one response");
            let MeasurementResponse::Ready { metrics, .. } = response else {
                panic!("Desktop text measurement is synchronously ready");
            };
            let prepared = self
                .text
                .prepared
                .get(
                    &metrics
                        .prepared_content
                        .expect("Desktop retains shaped text"),
                )
                .expect("prepared text is retained by its response ID");
            let attrs = prepared
                .buffer
                .lines
                .first()
                .expect("text produces one source line")
                .attrs_list()
                .defaults();
            let expected_features = font_features
                .iter()
                .map(|feature| (feature.tag.as_bytes(), feature.value))
                .collect::<Vec<_>>();
            let actual_features = attrs
                .font_features
                .features
                .iter()
                .map(|feature| (feature.tag.as_bytes().as_slice(), feature.value))
                .collect::<Vec<_>>();
            assert_eq!(actual_features, expected_features);
            if let Some(weight) = font_variations
                .iter()
                .rev()
                .find(|variation| variation.tag == "wght")
            {
                assert_eq!(attrs.weight.0, weight.value.clamp(1.0, 1000.0) as u16);
            }
        }
    }

    fn check_measurement(
        &self,
        key: u64,
        width: [f32; 2],
        height: [f32; 2],
        expects_prepared_content: Option<bool>,
    ) {
        let response = self
            .measurement_responses
            .iter()
            .find(|response| response.key().get() == key)
            .expect("measurement checkpoint key was observed");
        let MeasurementResponse::Ready { metrics, .. } = response else {
            panic!("Desktop text measurement is synchronously ready");
        };
        assert!(
            (width[0]..=width[1]).contains(&metrics.size.width),
            "measured width {} is outside {width:?}",
            metrics.size.width
        );
        assert!(
            (height[0]..=height[1]).contains(&metrics.size.height),
            "measured height {} is outside {height:?}",
            metrics.size.height
        );
        if let Some(expected) = expects_prepared_content {
            assert_eq!(metrics.prepared_content.is_some(), expected);
        }
    }

    fn emit_pointer(
        &mut self,
        kind: PointerEventFixture,
        pointer_kind: PointerKindFixture,
        pointer_id: u64,
        timestamp_ms: f64,
        position: [f32; 2],
        buttons: u32,
        changed_button: i16,
    ) {
        let adapter = self
            .pointer
            .as_ref()
            .expect("attach_surface precedes emit_pointer");
        self.input.dispatch(
            adapter
                .pointer_event(DesktopPointerEvent {
                    timestamp_ms,
                    phase: match kind {
                        PointerEventFixture::Down => DesktopPointerPhase::Down,
                        PointerEventFixture::Move => DesktopPointerPhase::Move,
                        PointerEventFixture::Up => DesktopPointerPhase::Up,
                        PointerEventFixture::Cancel => DesktopPointerPhase::Cancel,
                    },
                    pointer_id,
                    pointer_kind: pointer_kind_protocol(pointer_kind),
                    position: InputPoint {
                        x: position[0],
                        y: position[1],
                    },
                    buttons,
                    changed_button,
                })
                .expect("scenario pointer event passes the production Desktop adapter"),
        );
    }

    fn check_input(
        &self,
        kind: PointerEventFixture,
        pointer_kind: PointerKindFixture,
        pointer_id: u64,
        position: [f32; 2],
    ) {
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
        assert_eq!(pointer.kind, pointer_kind_protocol(pointer_kind));
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

fn overflow_clip_protocol(value: OverflowClipFixture) -> OverflowClip {
    match value {
        OverflowClipFixture::Visible => OverflowClip::Visible,
        OverflowClipFixture::Hidden => OverflowClip::Hidden,
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
            let coordinate =
                |value: whisker_host_conformance::LengthPercentageFixture| PaintCoordinate {
                    length: value.length,
                    fraction: value.fraction,
                };
            let radius = |value: whisker_host_conformance::CornerRadiusFixture| PaintCornerRadius {
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
                    top: coordinate(edges[0]),
                    right: coordinate(edges[1]),
                    bottom: coordinate(edges[2]),
                    left: coordinate(edges[3]),
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
            radius: PaintLengthPercentage {
                length: radius.length,
                fraction: radius.fraction,
            },
            center: PaintPosition {
                x: PaintCoordinate {
                    length: center[0].length,
                    fraction: center[0].fraction,
                },
                y: PaintCoordinate {
                    length: center[1].length,
                    fraction: center[1].fraction,
                },
            },
        },
        ClipShapeFixture::Ellipse { radii, center } => ClipShape::Ellipse {
            radius_x: PaintLengthPercentage {
                length: radii[0].length,
                fraction: radii[0].fraction,
            },
            radius_y: PaintLengthPercentage {
                length: radii[1].length,
                fraction: radii[1].fraction,
            },
            center: PaintPosition {
                x: PaintCoordinate {
                    length: center[0].length,
                    fraction: center[0].fraction,
                },
                y: PaintCoordinate {
                    length: center[1].length,
                    fraction: center[1].fraction,
                },
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
    let position = |point: &[whisker_host_conformance::LengthPercentageFixture; 2]| PaintPosition {
        x: PaintCoordinate {
            length: point[0].length,
            fraction: point[0].fraction,
        },
        y: PaintCoordinate {
            length: point[1].length,
            fraction: point[1].fraction,
        },
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

    let test_pixels = pollster::block_on(render_clipped_box_primitives_offscreen(
        &test[0].primitives,
        test[0].logical_size,
        &test[0].raster_resources,
    ))
    .expect("Desktop test pixel checkpoint");
    let reference_pixels = pollster::block_on(render_clipped_box_primitives_offscreen(
        &reference[0].primitives,
        reference[0].logical_size,
        &reference[0].raster_resources,
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
        let pixels = pollster::block_on(render_clipped_box_primitives_with_backdrops_offscreen(
            &checkpoint.primitives,
            &checkpoint.backdrops,
            checkpoint.logical_size,
            &checkpoint.raster_resources,
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
            lower_box(
                rect,
                paint.expect("render! emits box paint"),
                opacity,
                |primitive| {
                    primitives.push(primitive);
                },
            );
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
