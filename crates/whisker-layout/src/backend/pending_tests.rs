use super::*;
use std::cell::Cell;
use taffy::{AvailableSpace, FlexDirection};

#[test]
fn pending_measurements_do_not_expand_with_nested_flex_depth() {
    for measurement_state in [MeasurementState::Blocked, MeasurementState::Provisional] {
        let mut tree = TaffyTree::new();
        let leaf = tree.new_leaf_with_context(Style::default(), ()).unwrap();
        let mut root = leaf;
        for depth in 0..8 {
            root = tree
                .new_with_children(
                    Style {
                        flex_direction: if depth % 2 == 0 {
                            FlexDirection::Row
                        } else {
                            FlexDirection::Column
                        },
                        ..Style::default()
                    },
                    &[root],
                )
                .unwrap();
        }
        let available = Size {
            width: AvailableSpace::Definite(600.0),
            height: AvailableSpace::Definite(800.0),
        };
        let mut state = LayoutState::default();
        let calls = Cell::new(0);
        state.compute_detailed(&mut tree, root, available, |_, _, _, _| {
            calls.set(calls.get() + 1);
            IntrinsicResult {
                size: LayoutSize::new(40.0, 20.0),
                state: measurement_state,
                ..IntrinsicResult::default()
            }
        });
        assert!(
            calls.get() < 100,
            "{measurement_state:?}: {} measurements",
            calls.get()
        );
        assert!(tree.dirty(root).unwrap());

        let ready_calls = Cell::new(0);
        state.compute_detailed(&mut tree, root, available, |_, _, _, _| {
            ready_calls.set(ready_calls.get() + 1);
            LayoutSize::new(90.0, 45.0).into()
        });
        assert!(ready_calls.get() > 0);
        assert_eq!(
            state.layout(leaf).size,
            Size {
                width: 90.0,
                height: 45.0
            }
        );
        state.compute_detailed(&mut tree, root, available, |_, _, _, _| {
            panic!("ready geometry must remain reusable")
        });
    }
}

fn size_input(width: f32) -> LayoutInput {
    LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: taffy::SizingMode::InherentSize,
        axis: taffy::RequestedAxis::Both,
        known_dimensions: Size {
            width: Some(width),
            height: None,
        },
        parent_size: Size::NONE,
        available_space: Size {
            width: AvailableSpace::MaxContent,
            height: AvailableSpace::MaxContent,
        },
        vertical_margins_are_collapsible: taffy::Line::FALSE,
    }
}

#[test]
fn cached_pending_sizes_preserve_dependency_flags_and_input_constraints() {
    for measurement_state in [MeasurementState::Blocked, MeasurementState::Provisional] {
        let mut tree = TaffyTree::new();
        let leaf = tree.new_leaf_with_context(Style::default(), ()).unwrap();
        let mut state = LayoutState::default();
        let calls = Cell::new(0);
        let mut pass = LayoutPass {
            tree: &mut tree,
            state: &mut state,
            pending_sizes: PendingSizes::default(),
            measure: |known: Size<Option<f32>>, _, _: Option<&()>, _: &[MeasuredInlineChild]| {
                calls.set(calls.get() + 1);
                IntrinsicResult {
                    size: LayoutSize::new(known.width.unwrap(), known.width.unwrap() / 2.0),
                    state: measurement_state,
                    ..IntrinsicResult::default()
                }
            },
        };
        for iteration in 0..3 {
            for width in 1..=12 {
                let before = (pass.state.blocked, pass.state.provisional);
                let output = pass.compute(leaf, size_input(width as f32), None);
                assert_eq!(output.size.height, width as f32 / 2.0);
                assert_eq!(
                    pass.state.blocked > before.0,
                    measurement_state == MeasurementState::Blocked
                );
                assert_eq!(
                    pass.state.provisional > before.1,
                    measurement_state == MeasurementState::Provisional
                );
            }
            assert_eq!(
                calls.get(),
                12,
                "iteration {iteration} must reuse all widths"
            );
        }
        for width in 13..=SIZE_CACHE_CAPACITY + 1 {
            pass.compute(leaf, size_input(width as f32), None);
        }
        let before = calls.get();
        let output = pass.compute(leaf, size_input(1.0), None);
        assert_eq!(calls.get(), before + 1, "old widths must be evicted");
        assert_eq!(output.size.height, 0.5);

        let mut other_constraints = size_input(1.0);
        other_constraints.parent_size.width = Some(500.0);
        let before = calls.get();
        pass.compute(leaf, other_constraints, None);
        assert_eq!(
            calls.get(),
            before + 1,
            "parent constraints belong to the cache key"
        );
    }
}

#[test]
fn pending_layout_execution_is_not_replaced_by_a_size_cache_hit() {
    let mut tree = TaffyTree::new();
    let leaf = tree.new_leaf_with_context(Style::default(), ()).unwrap();
    let mut state = LayoutState::default();
    let calls = Cell::new(0);
    let mut pass = LayoutPass {
        tree: &mut tree,
        state: &mut state,
        pending_sizes: PendingSizes::default(),
        measure: |_: Size<Option<f32>>, _, _: Option<&()>, _: &[MeasuredInlineChild]| {
            calls.set(calls.get() + 1);
            IntrinsicResult {
                size: LayoutSize::new(40.0, 20.0),
                state: MeasurementState::Provisional,
                ..IntrinsicResult::default()
            }
        },
    };
    pass.compute(leaf, size_input(40.0), None);
    let mut input = size_input(40.0);
    input.run_mode = RunMode::PerformLayout;
    pass.compute(leaf, input, None);
    pass.compute(leaf, input, None);
    assert_eq!(calls.get(), 3);
}
