use futures_util::FutureExt;
use whisker_engine::{PlainTextInput, SurfaceEngine};
use whisker_protocol::{ElementTypeId, SurfaceId};
use whisker_style::{SpecifiedStyle, StyleEnvironment, resolve_style};

use super::*;
use crate::{RuntimeContext, RuntimeWakeHandle, reactive::Owner};

fn paragraph() -> (SurfaceEngine, NodeId) {
    let mut surface = SurfaceEngine::new(SurfaceId::new(1).unwrap());
    let style = resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default()).unwrap();
    let node = surface
        .create_node(
            ElementTypeId::new(1).unwrap(),
            style.computed().layout().clone(),
        )
        .unwrap();
    surface
        .set_plain_text(node, &PlainTextInput::new("Hello 🦀"), style.computed())
        .unwrap();
    (surface, node)
}

#[test]
fn replies_are_correlated_once_and_cannot_target_another_node() {
    RuntimeContext::new(RuntimeWakeHandle::new(|| {})).enter(|| {
        let owner = Owner::new(None);
        let (surface, node) = paragraph();
        let queries = Rc::new(RefCell::new(Queries::default()));
        let (id, future) = owner.with(|| {
            Queries::register(
                &queries,
                node,
                surface.scene().node_snapshot(node).unwrap(),
                TextQuery::SelectedText,
            )
            .unwrap()
        });
        let response = WhiskerValue::map([
            ("id", WhiskerValue::Int(id)),
            ("text", WhiskerValue::String("Hello".into())),
        ]);
        assert!(!queries.borrow_mut().complete(
            NodeId::new(99).unwrap(),
            surface.node(node),
            &response
        ));
        assert!(
            queries
                .borrow_mut()
                .complete(node, surface.node(node), &response)
        );
        assert!(
            !queries
                .borrow_mut()
                .complete(node, surface.node(node), &response)
        );
        assert_eq!(
            future.now_or_never(),
            Some(Ok(TextQueryOutput::Text("Hello".into())))
        );
        assert!(queries.borrow().pending.is_empty());
        owner.dispose();
    });
}

#[test]
fn disposing_owner_or_dropping_future_unregisters_pending_queries() {
    RuntimeContext::new(RuntimeWakeHandle::new(|| {})).enter(|| {
        let owner = Owner::new(None);
        let (surface, node) = paragraph();
        let queries = Rc::new(RefCell::new(Queries::default()));
        for _ in 0..100 {
            let (_, future) = owner.with(|| {
                Queries::register(
                    &queries,
                    node,
                    surface.scene().node_snapshot(node).unwrap(),
                    TextQuery::SelectedText,
                )
                .unwrap()
            });
            drop(future);
        }
        assert!(queries.borrow().pending.is_empty());
        let (_, future) = owner.with(|| {
            Queries::register(
                &queries,
                node,
                surface.scene().node_snapshot(node).unwrap(),
                TextQuery::SelectedText,
            )
            .unwrap()
        });
        owner.dispose();
        assert!(queries.borrow().pending.is_empty());
        assert_eq!(future.now_or_never(), Some(Err(TextQueryError::Cancelled)));
    });
}

#[test]
fn changed_text_node_removal_and_timeout_complete_with_distinct_errors() {
    RuntimeContext::new(RuntimeWakeHandle::new(|| {})).enter(|| {
        let owner = Owner::new(None);
        let (mut surface, node) = paragraph();
        let queries = Rc::new(RefCell::new(Queries::default()));
        let register = |surface: &SurfaceEngine| {
            owner.with(|| {
                Queries::register(
                    &queries,
                    node,
                    surface.scene().node_snapshot(node).unwrap(),
                    TextQuery::SelectedText,
                )
                .unwrap()
            })
        };
        let (id, stale) = register(&surface);
        let style =
            resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default()).unwrap();
        surface
            .set_plain_text(node, &PlainTextInput::new("Changed"), style.computed())
            .unwrap();
        queries.borrow_mut().complete(
            node,
            surface.node(node),
            &WhiskerValue::map([
                ("id", WhiskerValue::Int(id)),
                ("text", WhiskerValue::String("Hello".into())),
            ]),
        );
        assert_eq!(stale.now_or_never(), Some(Err(TextQueryError::StaleLayout)));
        let (_, removed) = register(&surface);
        queries.borrow_mut().cancel_node(node);
        assert_eq!(removed.now_or_never(), Some(Err(TextQueryError::NotBound)));
        let (_, expired) = register(&surface);
        assert!(queries.borrow_mut().step(100.0));
        assert!(queries.borrow_mut().step(5099.0));
        assert!(!queries.borrow_mut().step(5100.0));
        assert_eq!(expired.now_or_never(), Some(Err(TextQueryError::Timeout)));
        owner.dispose();
    });
}

#[test]
fn malformed_geometry_cannot_cross_the_text_query_boundary() {
    for width in [-1.0, f64::INFINITY, f64::NAN, f64::MAX] {
        let response = WhiskerValue::map([(
            "rects",
            WhiskerValue::Array(vec![WhiskerValue::map([
                ("x", WhiskerValue::Float(0.0)),
                ("y", WhiskerValue::Float(0.0)),
                ("width", WhiskerValue::Float(width)),
                ("height", WhiskerValue::Float(20.0)),
            ])]),
        )]);
        assert!(matches!(
            decode_result(
                TextQuery::BoundingRects(TextRange { start: 0, end: 1 }),
                &response
            ),
            Err(TextQueryError::Host(_))
        ));
    }
}
