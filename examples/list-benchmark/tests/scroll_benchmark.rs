use std::convert::Infallible;
use std::hint::black_box;
use std::time::{Duration, Instant};

use whisker::SurfaceRuntime;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{set_root, with_installed_renderer};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    InputEvent, InputEventKind, MeasurementRequest, MeasurementResponse, Operation, SurfaceId,
    WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{LayoutOptions, MeasurementProvider, RecordingRenderer};

const ITEM_COUNT: u32 = 100_000;
const UPDATE_COUNT: u32 = 1_000;
const ROW_HEIGHT: f64 = 44.0;

#[derive(Default)]
struct NoMeasurement;

impl MeasurementProvider for NoMeasurement {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        _responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        assert!(requests.is_empty());
        Ok(())
    }
}

/// Manual throughput benchmark kept at the public List/Host-event seam.
///
/// This is ignored in CI because elapsed-time assertions would be machine
/// dependent. The deterministic performance contract is covered by
/// `surface_render_pipeline::list_scroll_reuses_the_indexed_source_and_only_mutates_window_edges`.
#[test]
#[ignore = "manual release-mode runtime benchmark"]
fn scrolls_a_hundred_thousand_row_list() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(91).unwrap(),
        StyleEnvironment::new(390.0, 844.0, 1.0, 14.0),
    );
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                list(
                    style: css!(width: percent(100), height: px(844)),
                    each: || (0..ITEM_COUNT).collect::<Vec<_>>(),
                    meta: |row: &u32| ItemMeta::key(*row).estimated_size(ROW_HEIGHT as i32),
                    recycled_children: |_row: ReadSignal<u32>| render! {
                        view(style: css!(width: percent(100), height: px(ROW_HEIGHT as i32)))
                    },
                )
            }
        });
        set_root(root);
    });

    let registrations = surface.element_registrations();
    let scroll_type = registrations
        .iter()
        .find(|registration| registration.name == whisker::SCROLL_VIEW_ELEMENT_NAME)
        .unwrap()
        .element_type;
    let mut host = NoMeasurement;
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(390.0, 844.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let scroll_node = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::CreateNode {
                node, element_type, ..
            } if *element_type == scroll_type => Some(*node),
            _ => None,
        })
        .unwrap();

    let started = Instant::now();
    let mut runtime_elapsed = Duration::ZERO;
    let mut presentation_elapsed = Duration::ZERO;
    let mut operation_count = 0_usize;
    with_installed_renderer(surface.renderer(), || {
        for update in 0..UPDATE_COUNT {
            let row = (update * 7) % (ITEM_COUNT - 20);
            let offset = f64::from(row) * ROW_HEIGHT;
            let runtime_started = Instant::now();
            black_box(
                surface
                    .dispatch_input(&InputEvent {
                        surface: surface.surface(),
                        timestamp_ms: f64::from(update) * 16.0,
                        kind: InputEventKind::Named("scroll".to_owned()),
                        pointer: None,
                        target: Some(scroll_node),
                        detail: WhiskerValue::map([
                            ("scrollTop", WhiskerValue::Float(offset)),
                            ("viewportHeight", WhiskerValue::Float(844.0)),
                            (
                                "scrollHeight",
                                WhiskerValue::Float(f64::from(ITEM_COUNT) * ROW_HEIGHT),
                            ),
                        ]),
                    })
                    .unwrap(),
            );
            whisker::flush();
            runtime_elapsed += runtime_started.elapsed();

            let presentation_started = Instant::now();
            black_box(
                surface
                    .present(update + 2, &mut renderer)
                    .expect("benchmark FramePacket must remain valid"),
            );
            presentation_elapsed += presentation_started.elapsed();
            operation_count += renderer
                .frames()
                .last()
                .expect("present must record one frame")
                .packet
                .operations
                .len();
        }
    });
    let elapsed = started.elapsed();
    eprintln!(
        "{UPDATE_COUNT} scroll updates over {ITEM_COUNT} rows: {elapsed:?} ({:.1} ns/update)",
        elapsed.as_nanos() as f64 / f64::from(UPDATE_COUNT)
    );
    eprintln!(
        "runtime: {:.1} ns/update; presentation: {:.1} ns/update; {:.1} operations/update",
        runtime_elapsed.as_nanos() as f64 / f64::from(UPDATE_COUNT),
        presentation_elapsed.as_nanos() as f64 / f64::from(UPDATE_COUNT),
        operation_count as f64 / f64::from(UPDATE_COUNT),
    );
    with_installed_renderer(surface.renderer(), || owner.dispose());
}
