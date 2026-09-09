use std::convert::Infallible;
use whisker::prelude::*;
use whisker::runtime::view::{set_root, with_installed_renderer};
use whisker::runtime::{ElementRegistry, SurfaceRuntime};
use whisker::{CustomMeasurePayload, ModuleMeasureContext, Owner};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    MeasuredSize, MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse,
    Operation, SurfaceId, WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{LayoutOptions, MeasurementProvider, RecordingRenderer};

#[whisker::module_element(name = "whisker.test/MeasuredControl", measurement = Custom, text_style = true)]
fn measured_control(value: Signal<String>, enabled: Signal<bool>, decoration: Signal<bool>) {}

fn payload(context: ModuleMeasureContext<'_>) -> Option<CustomMeasurePayload> {
    if context.property("enabled") == Some(&WhiskerValue::Bool(false)) {
        return None;
    }
    let value = match context.property("value") {
        Some(WhiskerValue::String(value)) => value.as_str(),
        _ => "",
    };
    let size = context.text_style().unwrap().style.font_size;
    Some(CustomMeasurePayload {
        version: 1,
        data: format!("{value}:{size}:{}", context.scale_factor()).into_bytes(),
    })
}

#[derive(Default)]
struct Host {
    requests: Vec<MeasurementRequest>,
}
impl MeasurementProvider for Host {
    type Error = Infallible;
    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        for request in requests {
            let MeasurementPayload::Custom(payload) = &request.payload else {
                panic!("custom payload expected")
            };
            let text = String::from_utf8(payload.data.clone()).unwrap();
            let length = text.split(':').next().unwrap().len() as f32;
            let width = request.constraints.known_dimensions[0].unwrap_or(100.0);
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics::from_size(MeasuredSize::new(
                    width,
                    20.0 * (length / 5.0).ceil().max(1.0),
                )),
            });
        }
        self.requests.extend_from_slice(requests);
        Ok(())
    }
}

#[test]
fn custom_payload_tracks_props_and_inherited_style_without_host_views() {
    whisker::runtime::reactive::__reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::with_element_registry(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(300.0, 600.0, 1.0, 14.0),
        ElementRegistry::standard_with_linked_providers().unwrap(),
    );
    let (value, font_size, decoration, enabled) =
        with_installed_renderer(surface.renderer(), || {
            owner.with(|| {
                let value = signal("short".to_string());
                let font_size = signal(16.0_f32);
                let decoration = signal(false);
                let enabled = signal(true);
                let root = render! {
                    View(
                        style: computed(move || {
                            Css::new()
                                .display_flex()
                                .flex_direction(FlexDirection::Column)
                                .font_size(px(font_size.get()))
                        }),
                    ) {
                        MeasuredControl(
                            value: value,
                            enabled: enabled,
                            decoration: decoration,
                            measure_with: payload,
                            style: Css::new().width(px(150)).max_height(px(60)),
                        )
                    }
                };
                set_root(root);
                (value, font_size, decoration, enabled)
            })
        });
    let mut host = Host::default();
    let mut sink = RecordingRenderer::new(surface.surface());
    let frame = |host: &mut Host, sink: &mut RecordingRenderer| {
        surface
            .render_frame(
                LayoutSize::new(300.0, 600.0),
                1,
                1,
                host,
                sink,
                LayoutOptions::default(),
            )
            .unwrap()
    };
    frame(&mut host, &mut sink);
    assert!(!host.requests.is_empty());
    assert_eq!(sink.frames().len(), 1);
    let measured_node = host.requests[0].node;
    let height = |sink: &RecordingRenderer| {
        sink.frames()
            .iter()
            .rev()
            .flat_map(|frame| frame.packet.operations.iter().rev())
            .find_map(|operation| match operation {
                Operation::SetLayout { node, geometry } if *node == measured_node => {
                    Some(geometry.border_box.height)
                }
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(height(&sink), 20.0);
    let calls = host.requests.len();
    frame(&mut host, &mut sink);
    assert_eq!(host.requests.len(), calls);
    with_installed_renderer(surface.renderer(), || {
        decoration.set(true);
        whisker::flush();
    });
    frame(&mut host, &mut sink);
    assert_eq!(
        host.requests.len(),
        calls,
        "unchanged payload reuses Host results"
    );
    with_installed_renderer(surface.renderer(), || {
        value.set("a much longer value".into());
        whisker::flush();
    });
    frame(&mut host, &mut sink);
    assert!(host.requests.len() > calls);
    assert_eq!(height(&sink), 60.0, "CSS clamps the measured height");
    with_installed_renderer(surface.renderer(), || {
        font_size.set(24.0);
        whisker::flush();
    });
    frame(&mut host, &mut sink);
    let MeasurementPayload::Custom(last) = &host.requests.last().unwrap().payload else {
        unreachable!()
    };
    assert!(String::from_utf8_lossy(&last.data).contains(":24:"));
    with_installed_renderer(surface.renderer(), || {
        value.set(String::new());
        whisker::flush();
    });
    frame(&mut host, &mut sink);
    assert_eq!(height(&sink), 20.0, "deleting text shrinks the layout");
    let calls = host.requests.len();
    with_installed_renderer(surface.renderer(), || {
        enabled.set(false);
        whisker::flush();
    });
    frame(&mut host, &mut sink);
    assert_eq!(host.requests.len(), calls);
    with_installed_renderer(surface.renderer(), || {
        owner.dispose();
        set_root(render! { View() });
    });
    frame(&mut host, &mut sink);
    assert_eq!(
        host.requests.len(),
        calls,
        "disposing the reactive owner does not leave borrowed state in the payload builder"
    );
}

#[test]
fn payload_builders_reject_elements_without_custom_leaf_measurement() {
    whisker::runtime::reactive::__reset_for_tests();
    let surface = SurfaceRuntime::new(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(300.0, 600.0, 1.0, 14.0),
    );
    with_installed_renderer(surface.renderer(), || {
        let root = View::builder().measure_with(payload).build();
        assert!(
            matches!(surface.binding_error(), Some(whisker::RuntimeBindingError::InvalidMeasurementBinding { element }) if element == root)
        );
    });
}
