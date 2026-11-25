use super::{LayoutInput, LayoutOutput, NodeId, RunMode, SIZE_CACHE_CAPACITY};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub(super) struct PendingSize {
    pub output: LayoutOutput,
    pub blocked: bool,
    pub provisional: bool,
}

#[derive(Default)]
pub(super) struct PendingSizes {
    nodes: FxHashMap<NodeId, VecDeque<(LayoutInput, PendingSize)>>,
}

impl PendingSizes {
    pub fn get(&self, node: NodeId, input: &LayoutInput) -> Option<PendingSize> {
        self.nodes
            .get(&node)?
            .iter()
            .find_map(|(key, value)| (key == input).then_some(*value))
    }

    pub fn insert(&mut self, node: NodeId, input: LayoutInput, value: PendingSize) {
        // PerformLayout also updates descendant geometry and cannot reuse size alone.
        if input.run_mode != RunMode::ComputeSize {
            return;
        }
        let sizes = self.nodes.entry(node).or_default();
        if sizes.len() == SIZE_CACHE_CAPACITY {
            sizes.pop_front();
        }
        sizes.push_back((input, value));
    }
}
