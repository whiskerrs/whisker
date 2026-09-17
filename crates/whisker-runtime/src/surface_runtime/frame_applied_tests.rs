use super::*;
use crate::reactive::Owner;
use crate::view::{append_child, create_element, on_next_frame_applied, set_root};
use std::cell::Cell;
use std::convert::Infallible;
use whisker_engine::RecordingRenderer;
use whisker_protocol::{
    ApplyResult, FramePacket, MeasurementRequest, MeasurementResponse, RenderCapabilities,
};

struct NoMeasurements;
impl MeasurementProvider for NoMeasurements {
    type Error = Infallible;
    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[MeasurementRequest],
        _: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        assert!(requests.is_empty());
        Ok(())
    }
}

struct RetrySink;
impl FrameSink for RetrySink {
    type Error = Infallible;
    fn capabilities(&self) -> RenderCapabilities {
        RenderCapabilities::all_frame_native()
    }
    fn present(&mut self, _: &FramePacket) -> Result<ApplyResult, Self::Error> {
        Ok(ApplyResult::NeedSnapshot {
            receiver_revision: 0,
        })
    }
}

fn layout(surface: &SurfaceRuntime) {
    surface
        .drive_layout(
            LayoutSize::new(320.0, 480.0),
            1,
            &mut NoMeasurements,
            LayoutOptions::default(),
        )
        .unwrap();
}

fn with_surface(f: impl FnOnce(&SurfaceRuntime)) {
    let context = crate::RuntimeContext::new(crate::RuntimeWakeHandle::new(|| {}));
    context.enter(|| {
        let owner = Owner::new(None);
        let surface = SurfaceRuntime::new(
            SurfaceId::new(98).unwrap(),
            StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
        );
        owner.with(|| with_installed_renderer(surface.renderer(), || f(&surface)));
        owner.dispose();
    });
}

#[test]
fn callback_waits_for_acceptance_and_is_one_shot_and_reentrant() {
    with_surface(|surface| {
        let root = create_element(ElementTag::View);
        set_root(root);
        let observed = Rc::new(Cell::new(0));
        let observed_callback = observed.clone();
        let source = surface.clone();
        assert!(on_next_frame_applied(root, move |revision| {
            assert_eq!(source.accepted_revision(), revision);
            observed_callback.set(revision);
            // Callback delivery must not retain a surface borrow.
            append_child(root, create_element(ElementTag::View));
        }));
        layout(surface);
        assert_eq!(observed.get(), 0);
        surface.present(1, &mut RetrySink).unwrap();
        assert_eq!(observed.get(), 0);
        let mut sink = RecordingRenderer::new(surface.surface());
        surface.present(1, &mut sink).unwrap();
        assert!(observed.get() > 0);
        let first = observed.get();
        layout(surface);
        surface.present(1, &mut sink).unwrap();
        assert_eq!(observed.get(), first);
    });
}

#[test]
fn unrelated_frame_does_not_release_detached_element() {
    with_surface(|surface| {
        let root = create_element(ElementTag::View);
        let detached = create_element(ElementTag::View);
        set_root(root);
        let hits = Rc::new(Cell::new(0));
        let captured = hits.clone();
        on_next_frame_applied(detached, move |_| captured.set(captured.get() + 1));
        let mut sink = RecordingRenderer::new(surface.surface());
        layout(surface);
        surface.present(1, &mut sink).unwrap();
        assert_eq!(hits.get(), 0);
        append_child(root, detached);
        layout(surface);
        surface.present(1, &mut sink).unwrap();
        assert_eq!(hits.get(), 1);
    });
}

#[test]
fn owner_disposal_cancels_pending_callback_even_if_element_survives() {
    with_surface(|surface| {
        let root = create_element(ElementTag::View);
        set_root(root);
        let owner = Owner::new(Owner::current());
        owner.with(|| on_next_frame_applied(root, |_| panic!("disposed callback")));
        owner.dispose();
        layout(surface);
        surface
            .present(1, &mut RecordingRenderer::new(surface.surface()))
            .unwrap();
    });
}

#[test]
fn callback_registered_during_delivery_waits_for_another_revision() {
    with_surface(|surface| {
        let root = create_element(ElementTag::View);
        set_root(root);
        let revision = Rc::new(Cell::new(0));
        let captured = revision.clone();
        on_next_frame_applied(root, move |first| {
            on_next_frame_applied(root, move |next| {
                assert!(next > first);
                captured.set(next);
            });
        });
        let mut sink = RecordingRenderer::new(surface.surface());
        layout(surface);
        surface.present(1, &mut sink).unwrap();
        assert_eq!(revision.get(), 0);
        // No update is not a new accepted frame.
        surface.present(1, &mut sink).unwrap();
        assert_eq!(revision.get(), 0);
        append_child(root, create_element(ElementTag::View));
        layout(surface);
        surface.present(1, &mut sink).unwrap();
        assert!(revision.get() > 0);
    });
}

#[test]
fn unfinished_list_pass_keeps_runtime_awake_until_initial_layout_is_applied() {
    let surface = SurfaceRuntime::new(
        SurfaceId::new(99).unwrap(),
        StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
    );
    let mut runtime =
        crate::RuntimeInstance::new(surface.clone(), crate::RuntimeWakeHandle::new(|| {}));
    let passes = Rc::new(Cell::new(0));
    let hits = Rc::new(Cell::new(0));
    runtime
        .mount({
            let passes = passes.clone();
            let hits = hits.clone();
            move || {
                let root = create_element(ElementTag::View);
                crate::view::observe_layout_batch_end(
                    root,
                    Box::new(move || {
                        if passes.get() < MAX_LIST_LAYOUT_PASSES + 1 {
                            passes.set(passes.get() + 1);
                            append_child(root, create_element(ElementTag::View));
                            crate::view::renderer::request_list_layout();
                        }
                    }),
                );
                on_next_frame_applied(root, move |_| hits.set(hits.get() + 1));
                root
            }
        })
        .unwrap();
    let mut sink = RecordingRenderer::new(surface.surface());
    let mut drive = |time| {
        runtime
            .drive_frame(
                time,
                StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
                1,
                1,
                &mut NoMeasurements,
                &mut sink,
                LayoutOptions::default(),
            )
            .unwrap()
    };
    let first = drive(0.0);
    assert_eq!(hits.get(), 0);
    assert!(first.needs_frame);
    drive(16.0);
    assert_eq!(hits.get(), 1);
}
