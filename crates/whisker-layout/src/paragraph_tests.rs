use super::*;
use whisker_protocol::{InlinePlacement, MeasurementKey};
use whisker_style::{Axes, Edges};

fn node(id: u64) -> NodeId {
    NodeId::new(id).unwrap()
}
fn length(value: f32) -> ComputedSizeValue {
    ComputedSizeValue::Value(ComputedLengthPercentage::new(value, 0.0))
}

#[derive(Default)]
struct ParagraphMeasure {
    children: Vec<Vec<MeasuredInlineChild>>,
    blocked: bool,
}
impl IntrinsicMeasurer for ParagraphMeasure {
    fn measure(&mut self, _: NodeId, _: MeasureRequest) -> LayoutSize {
        panic!("paragraph must receive measured children")
    }
    fn measure_paragraph(
        &mut self,
        _: NodeId,
        _: MeasureRequest,
        children: &[MeasuredInlineChild],
    ) -> IntrinsicResult {
        self.children.push(children.to_vec());
        IntrinsicResult {
            size: LayoutSize::new(100.0, 60.0),
            first_baseline: Some(40.0),
            state: if self.blocked {
                MeasurementState::Blocked
            } else {
                MeasurementState::Ready
            },
            selection: MeasurementKey::new(77),
            inline_placements: children
                .iter()
                .map(|child| InlinePlacement {
                    node: child.node,
                    origin: Some([10.0, 15.0]),
                })
                .collect(),
        }
    }
}

fn inline_tree() -> LayoutTree {
    let mut tree = LayoutTree::new();
    tree.create_node(
        node(1),
        ComputedLayoutStyle {
            size: Axes {
                width: length(100.0),
                height: ComputedSizeValue::Auto,
            },
            ..ComputedLayoutStyle::default()
        },
    )
    .unwrap();
    tree.set_measurable(node(1), true).unwrap();
    tree.set_paragraph(node(1), true).unwrap();
    tree.create_node(
        node(2),
        ComputedLayoutStyle {
            size: Axes {
                width: length(40.0),
                height: length(30.0),
            },
            margin: Edges {
                top: ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(3.0, 0.0)),
                right: ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(2.0, 0.0)),
                bottom: ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(
                    3.0, 0.0,
                )),
                left: ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(2.0, 0.0)),
            },
            ..ComputedLayoutStyle::default()
        },
    )
    .unwrap();
    tree.set_children(node(1), &[node(2)]).unwrap();
    tree
}

#[test]
fn paragraph_measures_atomic_margin_boxes_and_commits_host_positions() {
    let mut tree = inline_tree();
    let mut measure = ParagraphMeasure::default();
    let viewport = LayoutSize::new(100.0, 100.0);
    let first = tree.compute(node(1), viewport, &mut measure).unwrap();
    assert!(!measure.children.is_empty());
    for children in &measure.children {
        assert_eq!(
            children,
            &[MeasuredInlineChild {
                node: node(2),
                size: LayoutSize::new(44.0, 36.0),
                baseline: 33.0
            }]
        );
    }
    let child = first.get(node(2)).unwrap().border_box;
    assert_eq!(
        child,
        LayoutRect {
            x: 12.0,
            y: 18.0,
            width: 40.0,
            height: 30.0
        }
    );
    assert_eq!(
        first.selections().collect::<Vec<_>>(),
        vec![(node(1), MeasurementKey::new(77).unwrap())]
    );
    let calls = measure.children.len();
    let second = tree.compute(node(1), viewport, &mut measure).unwrap();
    assert_eq!(first, second);
    assert_eq!(calls, measure.children.len());
}

#[test]
fn blocked_paragraphs_are_remeasured_instead_of_caching_placeholder_sizes() {
    let mut tree = inline_tree();
    let mut measure = ParagraphMeasure {
        blocked: true,
        ..ParagraphMeasure::default()
    };
    let viewport = LayoutSize::new(100.0, 100.0);
    tree.compute(node(1), viewport, &mut measure).unwrap();
    let calls = measure.children.len();
    measure.blocked = false;
    let snapshot = tree.compute(node(1), viewport, &mut measure).unwrap();
    assert!(measure.children.len() > calls);
    assert_eq!(snapshot.get(node(2)).unwrap().border_box.x, 12.0);
}

#[test]
fn percentage_inline_height_uses_definite_paragraph_content_height() {
    let mut tree = inline_tree();
    let mut paragraph = tree.style(node(1)).unwrap().clone();
    paragraph.size.height = length(100.0);
    paragraph.padding.top = ComputedLengthPercentage::new(10.0, 0.0);
    paragraph.padding.bottom = ComputedLengthPercentage::new(10.0, 0.0);
    tree.update_style(node(1), paragraph).unwrap();
    let mut inline = tree.style(node(2)).unwrap().clone();
    inline.size.height = ComputedSizeValue::Value(ComputedLengthPercentage::new(0.0, 0.5));
    tree.update_style(node(2), inline).unwrap();
    let mut measure = ParagraphMeasure::default();
    let result = tree
        .compute(node(1), LayoutSize::new(100.0, 200.0), &mut measure)
        .unwrap();
    assert_eq!(result.get(node(2)).unwrap().border_box.height, 40.0);
}

#[derive(Default)]
struct FlexibleMeasure {
    hidden: bool,
    child_state: MeasurementState,
}
impl IntrinsicMeasurer for FlexibleMeasure {
    fn measure(&mut self, _: NodeId, _: MeasureRequest) -> LayoutSize {
        LayoutSize::new(30.0, 20.0)
    }
    fn measure_detailed(&mut self, node: NodeId, request: MeasureRequest) -> IntrinsicResult {
        let mut result: IntrinsicResult = self.measure(node, request).into();
        result.state = self.child_state;
        result
    }
    fn measure_paragraph(
        &mut self,
        _: NodeId,
        _: MeasureRequest,
        children: &[MeasuredInlineChild],
    ) -> IntrinsicResult {
        IntrinsicResult {
            size: LayoutSize::new(100.0, 40.0),
            first_baseline: Some(18.0),
            selection: None,
            state: MeasurementState::Ready,
            inline_placements: children
                .iter()
                .map(|child| InlinePlacement {
                    node: child.node,
                    origin: (!self.hidden).then_some([0.0, 0.0]),
                })
                .collect(),
        }
    }
}

#[test]
fn auto_inline_boxes_remeasure_and_suppression_propagates_to_descendants() {
    let mut tree = inline_tree();
    let mut inline = tree.style(node(2)).unwrap().clone();
    inline.size = Axes {
        width: ComputedSizeValue::Auto,
        height: ComputedSizeValue::Auto,
    };
    tree.update_style(node(2), inline).unwrap();
    tree.set_measurable(node(2), true).unwrap();
    tree.create_node(node(3), ComputedLayoutStyle::default())
        .unwrap();
    tree.set_children(node(2), &[node(3)]).unwrap();
    let mut measure = FlexibleMeasure {
        hidden: true,
        child_state: MeasurementState::Ready,
    };
    let snapshot = tree
        .compute(node(1), LayoutSize::new(100.0, 100.0), &mut measure)
        .unwrap();
    for id in [2, 3] {
        assert_eq!(
            snapshot.get_with_participation(node(id)).unwrap().1,
            LayoutParticipation::SuppressedByParagraph
        );
    }
    assert!(snapshot.has_paragraph_suppression());
    measure.hidden = false;
    tree.invalidate_measurement(node(1)).unwrap();
    let snapshot = tree
        .compute(node(1), LayoutSize::new(100.0, 100.0), &mut measure)
        .unwrap();
    assert_eq!(
        snapshot.participation(node(2)),
        Some(LayoutParticipation::Participating)
    );
    tree.set_children(node(2), &[]).unwrap();
    measure.child_state = MeasurementState::Blocked;
    tree.invalidate_measurement(node(2)).unwrap();
    tree.compute(node(1), LayoutSize::new(100.0, 100.0), &mut measure)
        .unwrap();
    measure.child_state = MeasurementState::Provisional;
    tree.compute(node(1), LayoutSize::new(100.0, 100.0), &mut measure)
        .unwrap();
    measure.child_state = MeasurementState::Ready;
    let result = tree
        .compute(node(1), LayoutSize::new(100.0, 100.0), &mut measure)
        .unwrap();
    assert_eq!(result.get(node(2)).unwrap().border_box.width, 30.0);
    assert!(tree.is_paragraph(node(1)));
    tree.set_paragraph(node(1), true).unwrap();
    assert!(tree.set_paragraph(node(999), true).is_err());
    tree.set_paragraph(node(1), false).unwrap();
    assert!(!tree.is_paragraph(node(1)));
    assert!(!tree.is_paragraph(node(999)));
    assert!(
        tree.layout_state
            .selection(taffy::NodeId::from(999usize))
            .is_none()
    );
}

#[test]
fn a_size_only_measurer_remains_a_valid_paragraph_fallback() {
    let mut measure = |_: NodeId, _: MeasureRequest| LayoutSize::new(30.0, 20.0);
    let request = MeasureRequest {
        known_dimensions: [None, None],
        available_space: [
            whisker_protocol::AvailableSpace::MinContent,
            whisker_protocol::AvailableSpace::MaxContent,
        ],
    };
    assert_eq!(
        measure.measure_paragraph(node(1), request, &[]).size,
        LayoutSize::new(30.0, 20.0)
    );
    let mut tree = inline_tree();
    let snapshot = tree
        .compute(node(1), LayoutSize::new(100.0, 100.0), &mut measure)
        .unwrap();
    assert_eq!(
        snapshot.participation(node(2)),
        Some(LayoutParticipation::SuppressedByParagraph)
    );
}

#[test]
fn inline_geometry_accounts_for_backend_scrollbars() {
    let mut tree = inline_tree();
    let backend = tree.nodes[&node(2)].backend;
    let mut style = tree.backend.style(backend).unwrap().clone();
    style.overflow = taffy::Point {
        x: taffy::Overflow::Scroll,
        y: taffy::Overflow::Visible,
    };
    style.scrollbar_width = 6.0;
    tree.backend.set_style(backend, style).unwrap();
    tree.compute(
        node(1),
        LayoutSize::new(100.0, 100.0),
        &mut ParagraphMeasure::default(),
    )
    .unwrap();
    assert_eq!(tree.layout_state.layout(backend).scrollbar_size.height, 6.0);
}

#[test]
fn intrinsic_paragraph_width_can_be_measured_without_a_definite_parent_width() {
    let mut tree = inline_tree();
    for id in [1, 2] {
        let mut style = tree.style(node(id)).unwrap().clone();
        style.size.width = ComputedSizeValue::Auto;
        tree.update_style(node(id), style).unwrap();
    }
    tree.set_measurable(node(2), true).unwrap();
    let result = tree
        .compute(
            node(1),
            LayoutSize::new(100.0, 100.0),
            &mut FlexibleMeasure::default(),
        )
        .unwrap();
    assert_eq!(result.get(node(2)).unwrap().border_box.width, 30.0);
}

#[test]
fn max_content_paragraph_measurements_leave_percentage_basis_indefinite() {
    let mut tree = inline_tree();
    let mut paragraph = tree.style(node(1)).unwrap().clone();
    paragraph.size.width = ComputedSizeValue::Auto;
    tree.update_style(node(1), paragraph).unwrap();
    tree.create_node(
        node(3),
        ComputedLayoutStyle {
            display: DisplayValue::Grid,
            grid_template_columns: ComputedGridTemplate::tracks([ComputedGridTrackSizing {
                min: ComputedGridMinTrackSizing::MaxContent,
                max: ComputedGridMaxTrackSizing::MaxContent,
            }]),
            ..Default::default()
        },
    )
    .unwrap();
    tree.set_children(node(3), &[node(1)]).unwrap();
    let result = tree
        .compute(
            node(3),
            LayoutSize::new(100.0, 100.0),
            &mut ParagraphMeasure::default(),
        )
        .unwrap();
    assert_eq!(result.get(node(1)).unwrap().border_box.width, 100.0);
    let mut container = tree.style(node(3)).unwrap().clone();
    container.display = DisplayValue::FlowRoot;
    tree.update_style(node(3), container).unwrap();
    let result = tree
        .compute(
            node(3),
            LayoutSize::new(100.0, 100.0),
            &mut ParagraphMeasure::default(),
        )
        .unwrap();
    assert_eq!(result.get(node(1)).unwrap().border_box.width, 100.0);
}
