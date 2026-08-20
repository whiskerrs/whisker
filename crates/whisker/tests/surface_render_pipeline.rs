use std::convert::Infallible;

use whisker::SurfaceRuntime;
use whisker::css::{BorderStyle, Overflow};
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{set_root, with_installed_renderer};
use whisker_engine::RecordingRenderer;
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    MeasuredSize, MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse,
    Operation, PaintColor, PreparedContentId, SurfaceId,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{HostLayoutOptions, MeasurementHost};

#[derive(Default)]
struct TextHost {
    calls: Vec<Vec<MeasurementRequest>>,
}

impl MeasurementHost for TextHost {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        self.calls.push(requests.to_vec());
        responses.extend(requests.iter().map(|request| {
            let MeasurementPayload::Text(payload) = &request.payload else {
                panic!("render! Text emitted a non-text measurement")
            };
            assert_eq!(payload.style.font_size, 20.0);
            MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics {
                    size: MeasuredSize::new(payload.text.chars().count() as f32 * 10.0, 24.0),
                    first_baseline: Some(18.0),
                    last_baseline: Some(18.0),
                    overflow: None,
                    prepared_content: PreparedContentId::new(request.key.get()),
                },
            }
        }));
        Ok(())
    }
}

#[test]
fn render_text_reaches_measured_frame_and_paint_only_delta() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(7).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let value = owner.with(|| signal(String::from("hello")));
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: css!(color: Color::rgba(220, 30, 40, 0.75))) {
                    text(
                        value: value,
                        style: css!(font_size: px(20)),
                    )
                }
            }
        });
        set_root(root);
        root
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    let frame = surface
        .render_frame(
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            HostLayoutOptions::default(),
        )
        .expect("render! text frame");
    assert!(frame.layout.has_layout());
    assert!(matches!(
        frame.presentation,
        Some(whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 1 })
    ));
    let measurement_calls = host.calls.len();
    assert!(measurement_calls > 0);

    let snapshot = &renderer.frames()[0].packet;
    let text_node = snapshot
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetText { node, content } => {
                assert_eq!(content.payload.text, "hello");
                assert_eq!(content.payload.style.font_size, 20.0);
                assert_eq!(
                    content.paint.foreground,
                    PaintColor::Srgba {
                        red: 220,
                        green: 30,
                        blue: 40,
                        alpha: 0.75,
                    }
                );
                assert!(content.prepared_content.is_some());
                Some(*node)
            }
            _ => None,
        })
        .expect("snapshot SetText");
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::InsertChild { child, .. } if *child == text_node)
    ));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { node, geometry } if *node == text_node && geometry.border_box.width == 50.0 && geometry.border_box.height == 24.0)
    ));
    with_installed_renderer(surface.renderer(), || {
        value.set(String::from("hello world"));
        whisker::flush();
    });
    surface
        .drive_layout_with_host(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            HostLayoutOptions::default(),
        )
        .expect("reactive render! text update");
    assert!(host.calls.len() > measurement_calls);
    assert!(matches!(
        surface.present(1, &mut renderer),
        Ok(Some(
            whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 2 }
        ))
    ));
    let text_delta = &renderer.frames()[1].packet;
    assert!(text_delta.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetText { node, content }
            if *node == text_node
                && content.payload.text == "hello world"
                && content.paint.foreground == PaintColor::Srgba {
                    red: 220,
                    green: 30,
                    blue: 40,
                    alpha: 0.75,
                }
                && content.prepared_content.is_some()
    )));
    assert!(text_delta.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { node, geometry } if *node == text_node && geometry.border_box.width == 110.0 && geometry.border_box.height == 24.0)
    ));
    let measurement_calls = host.calls.len();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, Css::new().color(Color::rgba(20, 60, 230, 1.0)));
    });
    let progress = surface
        .drive_layout_with_host(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            HostLayoutOptions::default(),
        )
        .expect("paint-only update");
    assert!(progress.has_layout());
    assert_eq!(
        host.calls.len(),
        measurement_calls,
        "paint must not trigger measurement"
    );

    assert!(matches!(
        surface.present(1, &mut renderer),
        Ok(Some(
            whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 3 }
        ))
    ));
    let delta = &renderer.frames()[2].packet;
    assert!(matches!(
        delta.operations.as_slice(),
        [Operation::SetText { node, content }]
            if *node == text_node
                && content.paint.foreground == PaintColor::Srgba {
                    red: 20,
                    green: 60,
                    blue: 230,
                    alpha: 1.0,
                }
                && content.prepared_content.is_some()
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
    assert_eq!(surface.binding_error(), None);
}

fn painted_box(background: Color) -> Css {
    Css::new()
        .width(px(120))
        .height(px(48))
        .background_color(background)
        .border_top_width(px(2))
        .border_right_width(px(2))
        .border_bottom_width(px(2))
        .border_left_width(px(2))
        .border_top_color(Color::rgb(10, 20, 30))
        .border_right_color(Color::rgb(10, 20, 30))
        .border_bottom_color(Color::rgb(10, 20, 30))
        .border_left_color(Color::rgb(10, 20, 30))
        .border_top_style(BorderStyle::Solid)
        .border_right_style(BorderStyle::Solid)
        .border_bottom_style(BorderStyle::Solid)
        .border_left_style(BorderStyle::Solid)
        .border_radius(px(8))
        .overflow(Overflow::Hidden)
        .opacity(0.75)
        .z_index(4)
}

#[test]
fn render_box_paint_and_clip_reach_the_frame_sink() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(8).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: painted_box(Color::rgb(200, 30, 40))) });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    surface
        .drive_layout_with_host(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            HostLayoutOptions::default(),
        )
        .expect("box layout");
    assert!(host.calls.is_empty());

    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .present(1, &mut renderer)
        .expect("present painted snapshot")
        .expect("painted snapshot exists");
    let root_node = surface.root().expect("surface root");
    let snapshot = &renderer.frames()[0].packet;
    assert!(snapshot.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetBoxPaint { node, paint }
            if *node == root_node
                && paint.background_color == PaintColor::Srgba {
                    red: 200,
                    green: 30,
                    blue: 40,
                    alpha: 1.0,
                }
                && paint.border_widths.top.length == 2.0
                && paint.border_radii.top_left.length == 8.0
    )));
    assert!(snapshot.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetClip { node, clip }
            if *node == root_node
                && clip.horizontal == whisker_engine::whisker_protocol::OverflowClip::Hidden
                && clip.vertical == whisker_engine::whisker_protocol::OverflowClip::Hidden
    )));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetOpacity { node, opacity } if *node == root_node && *opacity == 0.75)
    ));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetZOrder { node, z_order } if *node == root_node && *z_order == 4)
    ));

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, painted_box(Color::rgb(20, 60, 230)));
    });
    surface
        .drive_layout_with_host(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            HostLayoutOptions::default(),
        )
        .expect("paint-only box update");
    assert!(host.calls.is_empty());
    surface
        .present(1, &mut renderer)
        .expect("present box delta")
        .expect("box delta exists");
    assert!(matches!(
        renderer.frames()[1].packet.operations.as_slice(),
        [Operation::SetBoxPaint { node, paint }]
            if *node == root_node
                && paint.background_color == PaintColor::Srgba {
                    red: 20,
                    green: 60,
                    blue: 230,
                    alpha: 1.0,
                }
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}
