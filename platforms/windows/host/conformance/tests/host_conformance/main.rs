#![cfg(whisker_host_lib)]

use std::ffi::c_void;

use base64::Engine as _;

use whisker::standard_element_registrations;
use whisker_engine::{FrameSink, MeasurementProvider};
use whisker_host_conformance::{
    BackgroundBoxFixture, BackgroundImageFixture, BackgroundLayerFixture, BackgroundSizeFixture,
    BackgroundSizeKeywordFixture, BorderFixture, BorderStyleFixture, ClipPathFixture,
    ClipReferenceBoxFixture, ClipShapeFixture, ColorFixture, Command, ConicGradientFixture,
    CursorFixture, FillRuleFixture, Host, ImageRenderingFixture, ImageRepeatFixture,
    LinearGradientFixture, LoadedCase, OverflowClipFixture, PathCommandFixture,
    PixelRelationFixture, PixelRelationKind, PixelSampleFixture, PointerEventsFixture,
    RadialGradientFixture, ResourceSourceFixture, ResourceStateFixture, Scenario, ScenarioSide,
    SceneNodeFixture, VisibilityFixture, load_required,
};
use whisker_protocol::{
    AvailableSpace, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode,
    BorderLineStyle, BoxClip, BoxPaint, ClipShape, Cursor, CursorKeyword, ElementTypeId, FillRule,
    FrameHeader, FrameMode, FramePacket, GradientStop, HitTestBehavior, ImageRendering,
    ImageRepeat, LayoutGeometry, LayoutRect, MeasureConstraints, MeasureFontFamily,
    MeasureFontStyle, MeasureLineHeight, MeasureTextAlignment, MeasureTextDirection,
    MeasureTextIndent, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, MeasurementKey,
    MeasurementPayload, MeasurementRequest, MeasurementResponse, NodeId, Operation, OverflowClip,
    PaintBox, PaintColor, PaintCoordinate, PaintCornerRadius, PaintCorners, PaintEdges, PaintImage,
    PaintLengthPercentage, PaintPosition, PathCommand, ProtocolVersion, RadialGradientExtent,
    RadialGradientShape, ResourceId, SurfaceId, TextContent, TextMeasurePayload, TextMeasureStyle,
    TextPaint, TextShadow, Transform, Visibility,
};

use whisker_driver::ffi_runtime::host_conformance::{frame_sink, measurement_provider};
use whisker_driver_sys::{
    MobileApplyResponse, MobileFrame, MobileMeasureRequest, MobileMeasureResponse,
    MobileResourceCommand, RESOURCE_COMMAND_LOAD, RESOURCE_COMMAND_RELEASE, RESOURCE_RASTER_IMAGE,
    RESOURCE_SOURCE_BYTES, RESOURCE_SOURCE_NONE, RESOURCE_SOURCE_URL, WhiskerBytesRef,
    WhiskerStringRef,
};

use whisker_windows_host_conformance::*;

extern "C" fn present_frame(
    host: *mut c_void,
    frame: *const MobileFrame,
    response: *mut MobileApplyResponse,
) -> bool {
    // SAFETY: the driver passes the live Host this sink was built with and a frame valid for the call.
    unsafe { whisker_host_conformance_present(host, frame, response) }
}

extern "C" fn measure(
    host: *mut c_void,
    requests: *const MobileMeasureRequest,
    count: usize,
    responses: *mut MobileMeasureResponse,
) -> bool {
    // SAFETY: the driver passes the live Host and `count` readable requests and writable responses.
    unsafe { whisker_host_conformance_measure(host, requests, count, responses) }
}

type PresentFn = Box<dyn FnMut(&FramePacket) -> Result<whisker_protocol::ApplyResult, String>>;
type MeasureFn = Box<
    dyn FnMut(
        SurfaceId,
        &[MeasurementRequest],
        &mut Vec<MeasurementResponse>,
    ) -> Result<(), String>,
>;

fn sample_rgba(color: &PaintColor) -> [u8; 4] {
    match color {
        PaintColor::Named(name) => csscolorparser::parse(name)
            .expect("fixture color name is a CSS color")
            .to_rgba8(),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => [*red, *green, *blue, (alpha * 255.0).round() as u8],
        other => panic!("unsupported sample color {other:?}"),
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
        paragraph: None,
        runs: Vec::new(),
        payload: TextMeasurePayload {
            runs: Vec::new(),
            attachments: Vec::new(),
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
            },
            locale: None,
            direction: match text.direction {
                whisker_host_conformance::TextDirectionFixture::Auto => MeasureTextDirection::Auto,
                whisker_host_conformance::TextDirectionFixture::LeftToRight => {
                    MeasureTextDirection::LeftToRight
                }
                whisker_host_conformance::TextDirectionFixture::RightToLeft => {
                    MeasureTextDirection::RightToLeft
                }
            },
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
    host: *mut WhiskerHostConformance,
    frames: Option<PresentFn>,
    measurement: Option<MeasureFn>,
    logical_size: [f32; 2],
    measurement_responses: Vec<MeasurementResponse>,
}

struct Checkpoint {
    size: [u32; 2],
    pixels: Vec<u8>,
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

impl Drop for Driver {
    fn drop(&mut self) {
        self.frames = None;
        self.measurement = None;
        if !self.host.is_null() {
            // SAFETY: `host` came from `whisker_host_conformance_create` and is destroyed once.
            unsafe { whisker_host_conformance_destroy(self.host) };
        }
    }
}

impl Driver {
    fn new() -> Self {
        Self {
            surface: None,
            host: std::ptr::null_mut(),
            frames: None,
            measurement: None,
            logical_size: [0.0; 2],
            measurement_responses: Vec::new(),
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
                    self.attach(*width, *height);
                }
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
                        "unsupported Windows checkpoint"
                    );
                    let (size, pixels) = self.render();
                    checkpoints.push(Checkpoint {
                        size,
                        pixels,
                        samples: samples.clone(),
                        relations: relations.clone(),
                    });
                }
                command @ Command::MeasureText { .. } => self.measure_text(command),
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
                Command::RegisterRasterResource {
                    id,
                    width,
                    height,
                    pixels,
                } => {
                    assert_eq!(pixels.len(), (*width * *height) as usize);
                    let rgba = pixels
                        .iter()
                        .flat_map(|color| sample_rgba(&color_protocol(color)))
                        .collect::<Vec<_>>();
                    // SAFETY: `rgba` holds `width * height` pixels and outlives the call.
                    let registered = unsafe {
                        whisker_host_conformance_register_raster(
                            self.host,
                            *id,
                            *width,
                            *height,
                            rgba.as_ptr(),
                        )
                    };
                    assert!(registered, "WinUI Host rejected raster {id}");
                }
                Command::LoadRasterResource {
                    id,
                    generation,
                    source,
                } => {
                    let (kind, identifier, data) = match source {
                        ResourceSourceFixture::Url { value } => {
                            (RESOURCE_SOURCE_URL, value.clone(), Vec::new())
                        }
                        ResourceSourceFixture::Bytes { media_type, base64 } => (
                            RESOURCE_SOURCE_BYTES,
                            media_type.clone(),
                            base64::engine::general_purpose::STANDARD
                                .decode(base64)
                                .expect("validated fixture base64"),
                        ),
                    };
                    self.resource_command(MobileResourceCommand {
                        command: RESOURCE_COMMAND_LOAD,
                        kind: RESOURCE_RASTER_IMAGE,
                        source: kind,
                        _reserved: 0,
                        resource: *id,
                        generation: *generation,
                        identifier: WhiskerStringRef {
                            ptr: identifier.as_ptr().cast(),
                            len: identifier.len(),
                        },
                        data: WhiskerBytesRef {
                            ptr: data.as_ptr(),
                            len: data.len(),
                        },
                    });
                }
                Command::ReleaseRasterResource { id, generation } => {
                    self.resource_command(MobileResourceCommand {
                        command: RESOURCE_COMMAND_RELEASE,
                        kind: 0,
                        source: RESOURCE_SOURCE_NONE,
                        _reserved: 0,
                        resource: *id,
                        generation: *generation,
                        identifier: WhiskerStringRef {
                            ptr: std::ptr::null(),
                            len: 0,
                        },
                        data: WhiskerBytesRef {
                            ptr: std::ptr::null(),
                            len: 0,
                        },
                    });
                }
                Command::CheckpointResource {
                    id,
                    generation,
                    state,
                    width,
                    height,
                } => {
                    let mut size = [0, 0];
                    // SAFETY: `size` provides both out-parameters.
                    let actual = unsafe {
                        whisker_host_conformance_resource_state(
                            self.host,
                            *id,
                            *generation,
                            &mut size[0],
                            &mut size[1],
                        )
                    };
                    let expected = match state {
                        ResourceStateFixture::Ready => 2,
                        ResourceStateFixture::Failed => 3,
                        ResourceStateFixture::Released => 4,
                    };
                    assert_eq!(actual, expected, "resource {id}@{generation} state");
                    if let (Some(width), Some(height)) = (width, height) {
                        assert_eq!(size, [*width, *height], "resource {id}@{generation} size");
                    }
                }
                Command::EmitPointer { .. } | Command::CheckpointInput { .. } => {
                    panic!("Windows pointer translation is not implemented")
                }
            }
        }
        checkpoints
    }

    fn resource_command(&mut self, command: MobileResourceCommand) {
        // SAFETY: the command borrows locals that outlive the synchronous call.
        let accepted = unsafe { whisker_host_conformance_resource(self.host, &command) };
        assert!(
            accepted,
            "WinUI Host rejected resource {}@{}",
            command.resource, command.generation
        );
    }

    fn attach(&mut self, width: f32, height: f32) {
        // SAFETY: the test harness runs every case on one thread.
        let host = unsafe { whisker_host_conformance_create() };
        assert!(
            !host.is_null(),
            "WinUI Host failed to start; run from an interactive logon"
        );
        // SAFETY: the capability profile is a plain value.
        let capabilities = unsafe { whisker_host_conformance_capabilities() };
        let mut sink = frame_sink(present_frame, host.cast(), &capabilities);
        self.frames = Some(Box::new(move |packet| {
            sink.present(packet).map_err(|error| format!("{error:?}"))
        }));
        let mut text = measurement_provider(measure, host.cast());
        self.measurement = Some(Box::new(move |surface, requests, responses| {
            text.measure_batch(surface, requests, responses)
                .map_err(|error| format!("{error:?}"))
        }));
        self.host = host;
        self.surface = Some(SurfaceId::new(1).unwrap());
        self.logical_size = [width, height];
    }

    fn present(&mut self, revision: u64, operations: Vec<Operation>) {
        let surface = self.surface.expect("attach_surface precedes presentation");
        let result = (self.frames.as_mut().expect("attached Windows scene"))(&FramePacket {
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
        });
        match result {
            Ok(whisker_protocol::ApplyResult::Accepted { .. }) => {}
            other => panic!("Host did not accept revision {revision}: {other:?}"),
        }
    }

    /// Captures the root visual at one physical pixel per logical pixel.
    fn render(&self) -> ([u32; 2], Vec<u8>) {
        let size = self.logical_size.map(|value| value.round() as u32);
        let mut pixels = vec![0; size[0] as usize * size[1] as usize * 4];
        // SAFETY: `pixels` holds `width * height` RGBA pixels and `host` is live.
        let captured = unsafe {
            whisker_host_conformance_capture(self.host, size[0], size[1], pixels.as_mut_ptr())
        };
        assert!(captured, "Composition capture failed");
        (size, pixels)
    }

    fn present_box(
        &mut self,
        revision: u64,
        rect: [f32; 4],
        background: &ColorFixture,
        border: Option<&BorderFixture>,
    ) {
        assert!(rect[0] >= 0.0 && rect[1] >= 0.0);
        assert!(rect[0] + rect[2] <= self.logical_size[0]);
        assert!(rect[1] + rect[3] <= self.logical_size[1]);
        let node = NodeId::new(1).unwrap();
        self.present(
            revision,
            vec![
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
                    paint: box_paint(background, border),
                },
            ],
        );
    }

    fn present_scene(&mut self, revision: u64, nodes: &[SceneNodeFixture]) {
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
            let layers = if !fixture.background_layers.is_empty() {
                fixture
                    .background_layers
                    .iter()
                    .map(|layer| background_image_protocol(&layer.image, &layer.geometry))
                    .collect()
            } else if let Some(gradient) = &fixture.linear_gradient {
                vec![linear_gradient_protocol(
                    gradient,
                    &fixture.background_layer,
                )]
            } else if let Some(gradient) = &fixture.radial_gradient {
                vec![radial_gradient_protocol(
                    gradient,
                    &fixture.background_layer,
                )]
            } else if let Some(gradient) = &fixture.conic_gradient {
                vec![conic_gradient_protocol(gradient, &fixture.background_layer)]
            } else {
                Vec::new()
            };
            if !layers.is_empty() {
                operations.push(Operation::SetBackgroundLayers { node, layers });
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
        self.present(revision, operations);
    }

    /// Painted text of every Text node, in node-id order.
    fn measure_text(&mut self, command: &Command) {
        let Command::MeasureText {
            key,
            text: value,
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
            direction,
            alignment,
            indent,
            available_width,
            available_width_kind,
        } = command
        else {
            unreachable!("measure_text is only called for MeasureText commands")
        };
        let surface = self.surface.expect("attach_surface precedes measure_text");
        let request = MeasurementRequest {
            key: MeasurementKey::new(*key).expect("scenario measurement key is non-zero"),
            node: NodeId::new(*key).expect("scenario node key is non-zero"),
            element_type: standard_element_type(whisker::TEXT_ELEMENT_NAME),
            environment_epoch: 1,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [
                    match available_width_kind {
                        whisker_host_conformance::AvailableWidthFixture::Definite => {
                            AvailableSpace::Definite(*available_width)
                        }
                        whisker_host_conformance::AvailableWidthFixture::MinContent => {
                            AvailableSpace::MinContent
                        }
                        whisker_host_conformance::AvailableWidthFixture::MaxContent => {
                            AvailableSpace::MaxContent
                        }
                    },
                    AvailableSpace::MaxContent,
                ],
            },
            payload: MeasurementPayload::Text(TextMeasurePayload {
                runs: Vec::new(),
                attachments: Vec::new(),
                text: value.clone(),
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
                    font_size: *font_size,
                    font_weight: *font_weight,
                    font_style: match *font_style {
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
                    line_height: MeasureLineHeight::LogicalPixels(*line_height),
                    letter_spacing: *letter_spacing,
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
                    optical_sizing: match *font_optical_sizing {
                        whisker_host_conformance::FontOpticalSizingFixture::Auto => {
                            whisker_protocol::FontOpticalSizing::Auto
                        }
                        whisker_host_conformance::FontOpticalSizingFixture::None => {
                            whisker_protocol::FontOpticalSizing::None
                        }
                    },
                },
                locale: None,
                direction: match *direction {
                    whisker_host_conformance::TextDirectionFixture::Auto => {
                        MeasureTextDirection::Auto
                    }
                    whisker_host_conformance::TextDirectionFixture::LeftToRight => {
                        MeasureTextDirection::LeftToRight
                    }
                    whisker_host_conformance::TextDirectionFixture::RightToLeft => {
                        MeasureTextDirection::RightToLeft
                    }
                },
                alignment: match *alignment {
                    whisker_host_conformance::TextAlignmentFixture::Start => {
                        MeasureTextAlignment::Start
                    }
                    whisker_host_conformance::TextAlignmentFixture::End => {
                        MeasureTextAlignment::End
                    }
                    whisker_host_conformance::TextAlignmentFixture::Left => {
                        MeasureTextAlignment::Left
                    }
                    whisker_host_conformance::TextAlignmentFixture::Right => {
                        MeasureTextAlignment::Right
                    }
                    whisker_host_conformance::TextAlignmentFixture::Center => {
                        MeasureTextAlignment::Center
                    }
                },
                indent: MeasureTextIndent {
                    logical_pixels: indent.logical_pixels,
                    percentage: indent.percentage,
                },
                wrap: match *white_space {
                    whisker_host_conformance::WhiteSpaceFixture::Normal => MeasureTextWrap::Wrap,
                    whisker_host_conformance::WhiteSpaceFixture::NoWrap => MeasureTextWrap::NoWrap,
                },
                word_break: match *word_break {
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
                max_lines: (*max_lines > 0).then_some(*max_lines),
                overflow: match *overflow {
                    whisker_host_conformance::TextOverflowFixture::Clip => {
                        MeasureTextOverflow::Clip
                    }
                    whisker_host_conformance::TextOverflowFixture::Ellipsis => {
                        MeasureTextOverflow::Ellipsis
                    }
                },
            }),
        };
        (self.measurement.as_mut().expect("attached Windows Host"))(
            surface,
            &[request],
            &mut self.measurement_responses,
        )
        .expect("DirectWrite measurement succeeds");
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
            panic!("DirectWrite measurement is synchronously ready: {response:?}");
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

fn run_reftest(scenario: &Scenario) {
    let reference_side = scenario
        .reference
        .as_ref()
        .expect("reftest scenario has reference commands");
    let test = Driver::new().execute(&scenario.test);
    let reference = Driver::new().execute(reference_side);
    assert_eq!(test.len(), 1, "one test checkpoint");
    assert_eq!(reference.len(), 1, "one reference checkpoint");
    assert_eq!(test[0].size, reference[0].size);
    assert!(
        test[0]
            .pixels
            .iter()
            .skip(3)
            .step_by(4)
            .any(|alpha| *alpha > 0)
    );
    let largest_difference = test[0]
        .pixels
        .iter()
        .zip(&reference[0].pixels)
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
    for checkpoint in Driver::new().execute(&scenario.test) {
        for sample in &checkpoint.samples {
            let actual = pixel_at(&checkpoint, sample.point, &scenario.id);
            let expected = sample_rgba(&color_protocol(&sample.color));
            let difference = actual
                .into_iter()
                .zip(expected)
                .map(|(actual, expected)| actual.abs_diff(expected))
                .max()
                .unwrap_or(0);
            assert!(
                difference <= sample.tolerance,
                "{} sample {:?} differs by {difference}: {actual:?} != {expected:?}",
                scenario.id,
                sample.point
            );
        }
        for relation in &checkpoint.relations {
            let first = pixel_at(&checkpoint, relation.first, &scenario.id);
            let second = pixel_at(&checkpoint, relation.second, &scenario.id);
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

fn pixel_at(checkpoint: &Checkpoint, point: [f32; 2], id: &str) -> [u8; 4] {
    let [width, height] = checkpoint.size;
    let x = point[0].floor() as u32;
    let y = point[1].floor() as u32;
    assert!(
        x < width && y < height,
        "{id} sample is outside the surface"
    );
    let offset = ((y * width + x) * 4) as usize;
    checkpoint.pixels[offset..offset + 4].try_into().unwrap()
}

fn luminance([red, green, blue, _]: [u8; 4]) -> u32 {
    (u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114) / 1000
}

fn run_case(LoadedCase { manifest, scenario }: &LoadedCase) {
    if scenario.reference.is_some() {
        assert!(
            manifest.checkpoints.iter().any(|value| value == "pixel"),
            "{} reftest must require a pixel checkpoint",
            scenario.id
        );
        run_reftest(scenario);
    } else {
        run_pixel_assertions(scenario);
    }
}

#[test]
fn every_manifest_case_required_by_windows_executes() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../tests/host-conformance");
    let (_, cases) = load_required(&root, Host::Windows).expect("load shared Windows fixtures");
    assert!(!cases.is_empty());
    cases.iter().for_each(run_case);
}
