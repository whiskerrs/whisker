use super::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

fn poll<T>(future: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn selectable_text_handles_query_presented_content_and_cancel_with_the_caller() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(SurfaceId::new(83).unwrap(), StyleEnvironment::default());
    let (handle, inner) = with_installed_renderer(surface.renderer(), || {
        owner.with(|| {
            let handle = TextHandle::new();
            let inner = TextHandle::new();
            let root = render! {
                Text(element_ref: handle.r(), selectable: true) {
                    Text(element_ref: inner.r(), value: "link")
                }
            };
            set_root(root);
            (handle, inner)
        })
    });
    assert_eq!(surface.binding_error(), None);
    let mut recorder = RecordingRenderer::new(surface.surface());
    let mut host = QueryHost;
    surface
        .render_frame(
            LayoutSize::new(160.0, 100.0),
            1,
            1,
            &mut host,
            &mut recorder,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(recorder.frames()[0].packet.operations.iter().any(|operation| matches!(operation, Operation::SetProperty { property, value: WhiskerValue::Bool(true), .. } if property.get() == 1)));
    with_installed_renderer(surface.renderer(), || {
        assert_eq!(
            inner.set_selection(TextRange { start: 0, end: 1 }),
            Err(TextQueryError::NotParagraph)
        );
        assert_eq!(
            handle.set_selection(TextRange { start: 0, end: 5 }),
            Err(TextQueryError::InvalidRange)
        );
    });
    let mut future = Box::pin(handle.selected_text());
    assert!(
        with_installed_renderer(surface.renderer(), || owner.with(|| poll(future.as_mut())))
            .is_pending()
    );
    surface
        .render_frame(
            LayoutSize::new(160.0, 100.0),
            1,
            1,
            &mut host,
            &mut recorder,
            LayoutOptions::default(),
        )
        .unwrap();
    let operations = &recorder.frames().last().unwrap().packet.operations;
    let mask = operations
        .iter()
        .position(|operation| matches!(operation, Operation::SetEventMask { .. }))
        .unwrap();
    let command = operations
        .iter()
        .position(|operation| matches!(operation, Operation::InvokeCommand { .. }))
        .unwrap();
    assert!(
        mask < command,
        "native synchronous replies require the subscription before the command"
    );
    let (node, id) = recorder
        .frames()
        .iter()
        .flat_map(|frame| &frame.packet.operations)
        .find_map(|operation| {
            if let Operation::InvokeCommand {
                node,
                command,
                arguments,
            } = operation
            {
                if command.get() == 3 {
                    if let WhiskerValue::Map(fields) = arguments {
                        return Some((*node, fields["id"].clone()));
                    }
                }
            }
            None
        })
        .unwrap();
    surface
        .dispatch_input(&InputEvent {
            surface: surface.surface(),
            timestamp_ms: 1.0,
            kind: InputEventKind::Named("textqueryresult".into()),
            pointer: None,
            target: Some(node),
            presentation_revision: None,
            detail: WhiskerValue::map([("id", id), ("text", WhiskerValue::String("link".into()))]),
        })
        .unwrap();
    assert_eq!(poll(future.as_mut()), Poll::Ready(Ok("link".into())));
    let caller = Owner::new(Some(owner));
    let mut pending = Box::pin(handle.selected_text());
    assert!(
        with_installed_renderer(surface.renderer(), || caller
            .with(|| poll(pending.as_mut())))
        .is_pending()
    );
    with_installed_renderer(surface.renderer(), || caller.dispose());
    assert_eq!(
        poll(pending.as_mut()),
        Poll::Ready(Err(TextQueryError::Cancelled))
    );
    with_installed_renderer(surface.renderer(), || owner.dispose());
    assert_eq!(handle.clear_selection(), Err(TextQueryError::NotBound));
}

struct QueryHost;
impl MeasurementProvider for QueryHost {
    type Error = Infallible;
    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        for request in requests {
            let mut metrics = MeasurementMetrics::from_size(MeasuredSize::new(40.0, 20.0));
            metrics.prepared_content = PreparedContentId::new(request.key.get());
            metrics.paragraph = Some(whisker_engine::whisker_protocol::ParagraphMetrics {
                lines: Vec::new(),
                fragments: Vec::new(),
            });
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics,
            });
        }
        Ok(())
    }
}
