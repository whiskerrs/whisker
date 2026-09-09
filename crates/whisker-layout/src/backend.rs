//! Taffy's layout algorithms with retained, bounded intrinsic-size reuse.

use crate::{IntrinsicResult, LayoutSize, MeasuredInlineChild, MeasurementState};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};
use whisker_protocol::MeasurementKey;

use taffy::{
    BlockContext, CacheTree, Display, Layout, LayoutBlockContainer, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RunMode, Size,
    Style, TaffyTree, TraversePartialTree, compute_block_layout, compute_cached_layout,
    compute_flexbox_layout, compute_grid_layout, compute_hidden_layout, compute_leaf_layout,
    compute_root_layout,
};

mod paragraph;

const SIZE_CACHE_CAPACITY: usize = 32;

#[derive(Clone, Debug, Default)]
struct NodeState {
    layout: Layout,
    sizes: Rc<Vec<(LayoutInput, LayoutOutput)>>,
    next_slot: usize,
    selection: Option<MeasurementKey>,
}

impl NodeState {
    fn clear_sizes(&mut self) {
        if let Some(sizes) = Rc::get_mut(&mut self.sizes) {
            sizes.clear();
        } else {
            self.sizes = Rc::default();
        }
        self.next_slot = 0;
    }

    fn store_size(&mut self, input: LayoutInput, size: LayoutOutput) {
        let sizes = Rc::make_mut(&mut self.sizes);
        if sizes.len() < SIZE_CACHE_CAPACITY {
            sizes.push((input, size));
        } else {
            sizes[self.next_slot] = (input, size);
            self.next_slot = (self.next_slot + 1) % SIZE_CACHE_CAPACITY;
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct LayoutState {
    nodes: HashMap<NodeId, NodeState>,
    paragraphs: HashSet<NodeId>,
    model_nodes: HashMap<NodeId, whisker_protocol::NodeId>,
    transient: HashSet<NodeId>,
    suppressed: HashSet<NodeId>,
    blocked: u64,
    provisional: u64,
}

impl LayoutState {
    pub(super) fn remove(&mut self, node: NodeId) {
        self.nodes.remove(&node);
        self.paragraphs.remove(&node);
        self.model_nodes.remove(&node);
        self.transient.remove(&node);
        self.suppressed.remove(&node);
    }

    pub(super) fn register(&mut self, node: NodeId, model: whisker_protocol::NodeId) {
        self.model_nodes.insert(node, model);
        self.nodes.entry(node).or_default();
    }
    pub(super) fn set_paragraph(&mut self, node: NodeId, paragraph: bool) {
        if paragraph {
            self.paragraphs.insert(node);
        } else {
            self.paragraphs.remove(&node);
        }
    }
    pub(super) fn suppressed(&self, node: NodeId) -> bool {
        self.suppressed.contains(&node)
    }
    pub(super) fn selection(&self, node: NodeId) -> Option<MeasurementKey> {
        self.nodes.get(&node)?.selection
    }

    pub(super) fn layout(&self, node: NodeId) -> &Layout {
        &self.nodes[&node].layout
    }

    pub(super) fn compute_detailed<Context: Clone, Measure>(
        &mut self,
        tree: &mut TaffyTree<Context>,
        root: NodeId,
        available: Size<taffy::AvailableSpace>,
        measure: Measure,
    ) where
        Measure: FnMut(
            Size<Option<f32>>,
            Size<taffy::AvailableSpace>,
            Option<&Context>,
            &[MeasuredInlineChild],
        ) -> IntrinsicResult,
    {
        // Taffy propagates style, tree and measurement invalidation to ancestors.
        // Consume that state before layout repopulates its built-in caches.
        for (node, state) in &mut self.nodes {
            if tree.dirty(*node).expect("retained layout node") {
                state.clear_sizes();
            }
        }
        compute_root_layout(
            &mut LayoutPass {
                tree,
                state: self,
                measure,
            },
            root,
            available,
        );
        for node in self.transient.drain() {
            tree.mark_dirty(node).expect("retained transient node");
            self.nodes
                .get_mut(&node)
                .expect("retained transient node")
                .clear_sizes();
        }
    }

    #[cfg(test)]
    pub(super) fn compute<Context: Clone, Measure>(
        &mut self,
        tree: &mut TaffyTree<Context>,
        root: NodeId,
        available: Size<taffy::AvailableSpace>,
        mut measure: Measure,
    ) where
        Measure:
            FnMut(Size<Option<f32>>, Size<taffy::AvailableSpace>, Option<&Context>) -> Size<f32>,
    {
        self.compute_detailed(tree, root, available, |known, available, context, _| {
            let size = measure(known, available, context);
            LayoutSize::new(size.width, size.height).into()
        });
    }
}

struct LayoutPass<'a, Context, Measure> {
    tree: &'a mut TaffyTree<Context>,
    state: &'a mut LayoutState,
    measure: Measure,
}

impl<Context: Clone, Measure> LayoutPass<'_, Context, Measure>
where
    Measure: FnMut(
        Size<Option<f32>>,
        Size<taffy::AvailableSpace>,
        Option<&Context>,
        &[MeasuredInlineChild],
    ) -> IntrinsicResult,
{
    fn compute(
        &mut self,
        node: NodeId,
        input: LayoutInput,
        block: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if input.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node);
        }
        let before = (self.state.blocked, self.state.provisional);
        let output = compute_cached_layout(self, node, input, |tree, node, input| {
            let display = tree.tree.style(node).expect("retained style").display;
            if display != Display::None && tree.state.paragraphs.contains(&node) {
                return tree.compute_paragraph(node, input);
            }
            match (display, tree.child_count(node) != 0) {
                (Display::None, _) => compute_hidden_layout(tree, node),
                (Display::Block, true) => compute_block_layout(tree, node, input, block),
                (Display::FlowRoot, true) => compute_block_layout(tree, node, input, None),
                (Display::Flex, true) => compute_flexbox_layout(tree, node, input),
                (Display::Grid, true) => compute_grid_layout(tree, node, input),
                (_, false) => tree.compute_measured_leaf(node, input),
            }
        });
        if before != (self.state.blocked, self.state.provisional) {
            self.cache_clear(node);
            self.state.transient.insert(node);
        }
        output
    }
}

impl<Context: Clone, Measure> TraversePartialTree for LayoutPass<'_, Context, Measure> {
    type ChildIter<'a>
        = <TaffyTree<Context> as TraversePartialTree>::ChildIter<'a>
    where
        Self: 'a;
    fn child_ids(&self, parent: NodeId) -> Self::ChildIter<'_> {
        self.tree.child_ids(parent)
    }
    fn child_count(&self, parent: NodeId) -> usize {
        self.tree.child_count(parent)
    }
    fn get_child_id(&self, parent: NodeId, index: usize) -> NodeId {
        self.tree.get_child_id(parent, index)
    }
}

impl<Context: Clone, Measure> CacheTree for LayoutPass<'_, Context, Measure> {
    fn cache_get(&self, node: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        if input.run_mode == RunMode::ComputeSize
            && let Some(size) = self.state.nodes.get(&node).and_then(|state| {
                state
                    .sizes
                    .iter()
                    .find(|(key, _)| key == input)
                    .map(|(_, size)| *size)
            })
        {
            return Some(size);
        }
        self.tree.cache_get(node, input)
    }

    fn cache_store(&mut self, node: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.tree.cache_store(node, input, output);
        if input.run_mode == RunMode::ComputeSize {
            self.state
                .nodes
                .entry(node)
                .or_default()
                .store_size(*input, output);
        }
    }

    fn cache_clear(&mut self, node: NodeId) {
        self.tree.cache_clear(node);
        if let Some(state) = self.state.nodes.get_mut(&node) {
            state.clear_sizes();
        }
    }
}

impl<Context: Clone, Measure> LayoutPartialTree for LayoutPass<'_, Context, Measure>
where
    Measure: FnMut(
        Size<Option<f32>>,
        Size<taffy::AvailableSpace>,
        Option<&Context>,
        &[MeasuredInlineChild],
    ) -> IntrinsicResult,
{
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type CustomIdent = String;
    fn get_core_container_style(&self, node: NodeId) -> &Style {
        self.tree.style(node).expect("retained style")
    }
    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        self.state.nodes.entry(node).or_default().layout = *layout;
    }
    fn compute_child_layout(&mut self, node: NodeId, input: LayoutInput) -> LayoutOutput {
        self.compute(node, input, None)
    }
}

impl<Context: Clone, Measure> LayoutFlexboxContainer for LayoutPass<'_, Context, Measure>
where
    Measure: FnMut(
        Size<Option<f32>>,
        Size<taffy::AvailableSpace>,
        Option<&Context>,
        &[MeasuredInlineChild],
    ) -> IntrinsicResult,
{
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_flexbox_container_style(&self, node: NodeId) -> &Style {
        self.get_core_container_style(node)
    }
    fn get_flexbox_child_style(&self, node: NodeId) -> &Style {
        self.get_core_container_style(node)
    }
}

impl<Context: Clone, Measure> LayoutGridContainer for LayoutPass<'_, Context, Measure>
where
    Measure: FnMut(
        Size<Option<f32>>,
        Size<taffy::AvailableSpace>,
        Option<&Context>,
        &[MeasuredInlineChild],
    ) -> IntrinsicResult,
{
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_grid_container_style(&self, node: NodeId) -> &Style {
        self.get_core_container_style(node)
    }
    fn get_grid_child_style(&self, node: NodeId) -> &Style {
        self.get_core_container_style(node)
    }
}

impl<Context: Clone, Measure> LayoutBlockContainer for LayoutPass<'_, Context, Measure>
where
    Measure: FnMut(
        Size<Option<f32>>,
        Size<taffy::AvailableSpace>,
        Option<&Context>,
        &[MeasuredInlineChild],
    ) -> IntrinsicResult,
{
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_block_container_style(&self, node: NodeId) -> &Style {
        self.get_core_container_style(node)
    }
    fn get_block_child_style(&self, node: NodeId) -> &Style {
        self.get_core_container_style(node)
    }
    fn compute_block_child_layout(
        &mut self,
        node: NodeId,
        input: LayoutInput,
        block: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute(node, input, block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use taffy::prelude::{TaffyAuto, TaffyMaxContent};
    use taffy::{AvailableSpace, Dimension, FlexDirection, Line, RequestedAxis, SizingMode};

    fn input(width: f32) -> LayoutInput {
        LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size {
                width: Some(width),
                height: None,
            },
            parent_size: Size::NONE,
            available_space: Size::MAX_CONTENT,
            vertical_margins_are_collapsible: Line::FALSE,
        }
    }

    #[test]
    fn alternating_widths_reuse_sizes_until_bounded_eviction() {
        let mut tree = TaffyTree::new();
        let leaf = tree.new_leaf_with_context(Style::default(), ()).unwrap();
        let mut state = LayoutState::default();
        let calls = Cell::new(0);
        let mut pass = LayoutPass {
            tree: &mut tree,
            state: &mut state,
            measure: |known: Size<Option<f32>>, _, _: Option<&()>, _: &[MeasuredInlineChild]| {
                calls.set(calls.get() + 1);
                LayoutSize::new(known.width.unwrap_or(40.0), 20.0).into()
            },
        };
        for _ in 0..3 {
            for width in 1..=12 {
                let result = pass.compute(leaf, input(width as f32), None);
                assert_eq!(
                    result.size,
                    Size {
                        width: width as f32,
                        height: 20.0
                    }
                );
            }
        }
        assert_eq!(
            calls.get(),
            12,
            "sizing widths that share a Taffy slot must remain reusable"
        );
        for width in 13..=SIZE_CACHE_CAPACITY + 2 {
            pass.compute(leaf, input(width as f32), None);
        }
        assert_eq!(pass.state.nodes[&leaf].sizes.len(), SIZE_CACHE_CAPACITY);
        let before = calls.get();
        pass.compute(leaf, input(1.0), None);
        assert_eq!(
            calls.get(),
            before + 1,
            "eviction must recompute, not return a different width's size"
        );
        pass.cache_clear(leaf);
        assert!(pass.state.nodes[&leaf].sizes.is_empty());
        pass.compute(leaf, input(1.0), None);
        assert_eq!(calls.get(), before + 2);
    }

    #[test]
    fn cloned_layout_snapshots_share_sizes_and_isolate_invalidations() {
        let mut state = NodeState::default();
        state.store_size(
            input(40.0),
            LayoutOutput::from_outer_size(Size {
                width: 40.0,
                height: 20.0,
            }),
        );
        let mut cloned = state.clone();
        assert!(Rc::ptr_eq(&state.sizes, &cloned.sizes));
        cloned.clear_sizes();
        assert!(cloned.sizes.is_empty());
        assert_eq!(state.sizes.len(), 1);
        let mut cloned = state.clone();
        cloned.store_size(
            input(50.0),
            LayoutOutput::from_outer_size(Size {
                width: 50.0,
                height: 20.0,
            }),
        );
        assert_eq!(cloned.sizes.len(), 2);
        assert_eq!(state.sizes.len(), 1);
    }

    #[test]
    fn retained_layout_matches_taffy_across_mutations_and_display_modes() {
        for display in [
            Display::Flex,
            Display::Grid,
            Display::Block,
            Display::FlowRoot,
            Display::None,
        ] {
            let mut tree = TaffyTree::new();
            let mut leaves = Vec::new();
            for n in 0..4 {
                leaves.push(
                    tree.new_leaf_with_context(
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: Dimension::length(n as f32 * 3.0),
                                height: Dimension::AUTO,
                            },
                            ..Style::default()
                        },
                        n,
                    )
                    .unwrap(),
                );
            }
            let inner = tree
                .new_with_children(
                    Style {
                        display,
                        flex_direction: FlexDirection::Row,
                        ..Style::default()
                    },
                    &leaves,
                )
                .unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        display: Display::Block,
                        flex_direction: FlexDirection::Column,
                        ..Style::default()
                    },
                    &[inner],
                )
                .unwrap();
            let nodes = leaves
                .iter()
                .copied()
                .chain([inner, root])
                .collect::<Vec<_>>();
            let mut state = LayoutState::default();
            let mut reference = tree.clone();
            for step in 0..5 {
                let style = Style {
                    flex_grow: step as f32,
                    ..Style::default()
                };
                tree.set_style(leaves[0], style.clone()).unwrap();
                reference.set_style(leaves[0], style).unwrap();
                let available = Size {
                    width: AvailableSpace::Definite(120.0 + step as f32),
                    height: AvailableSpace::Definite(200.0),
                };
                let measure = |known: Size<Option<f32>>,
                               available: Size<AvailableSpace>,
                               context: Option<&i32>| {
                    let natural = 30.0 + context.copied().unwrap_or(0) as f32;
                    let width = known.width.unwrap_or(match available.width {
                        AvailableSpace::Definite(w) => w.min(natural),
                        AvailableSpace::MinContent => 10.0,
                        AvailableSpace::MaxContent => natural,
                    });
                    Size {
                        width,
                        height: known
                            .height
                            .unwrap_or(if width < natural { 40.0 } else { 20.0 }),
                    }
                };
                reference.disable_rounding();
                reference
                    .compute_layout_with_measure(
                        root,
                        available,
                        |known, available, _, context, _| {
                            measure(known, available, context.as_deref())
                        },
                    )
                    .unwrap();
                state.compute(&mut tree, root, available, measure);
                for node in &nodes {
                    assert_eq!(
                        state.layout(*node),
                        reference.unrounded_layout(*node),
                        "{display:?} step {step} node {node:?}"
                    );
                }
                state.compute(&mut tree, root, available, measure);
                for node in &nodes {
                    assert_eq!(state.layout(*node), reference.unrounded_layout(*node));
                }
            }
        }
    }
}
