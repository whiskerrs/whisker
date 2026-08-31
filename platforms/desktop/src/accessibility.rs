//! AccessKit projection shared by the macOS, Windows, and Linux Hosts.

use std::collections::HashMap;

use accesskit::{
    Action, ActionRequest, Affine, Node, NodeId as AccessNodeId, Rect, Role, Toggled, TreeId,
    TreeInfo, TreeUpdate,
};
use whisker_protocol::{
    Accessibility, AccessibilityChecked, AccessibilityRole, LayoutRect, NodeId as WhiskerNodeId,
};

const ROOT_ID: AccessNodeId = AccessNodeId(0);

/// Host-neutral semantic node produced from the retained Desktop scene.
#[derive(Clone, Debug)]
pub(crate) struct DesktopAccessibilityNode {
    pub(crate) id: WhiskerNodeId,
    pub(crate) children: Vec<WhiskerNodeId>,
    pub(crate) bounds: LayoutRect,
    pub(crate) semantics: Accessibility,
    pub(crate) text: Option<String>,
    pub(crate) hidden: bool,
}

/// Complete semantic snapshot of one Desktop surface.
#[derive(Clone, Debug, Default)]
pub(crate) struct DesktopAccessibilitySnapshot {
    pub(crate) roots: Vec<WhiskerNodeId>,
    pub(crate) nodes: Vec<DesktopAccessibilityNode>,
}

/// Runtime result of an action requested by an assistive technology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DesktopAccessibilityAction {
    Click(WhiskerNodeId),
    FocusChanged,
    Ignored,
}

/// Converts scene snapshots into incremental AccessKit tree updates.
pub(crate) struct DesktopAccessibilityBridge {
    previous: HashMap<AccessNodeId, Node>,
    focus: AccessNodeId,
}

impl Default for DesktopAccessibilityBridge {
    fn default() -> Self {
        Self {
            previous: HashMap::new(),
            focus: ROOT_ID,
        }
    }
}

impl DesktopAccessibilityBridge {
    pub(crate) fn reset(&mut self) {
        self.previous.clear();
        self.focus = ROOT_ID;
    }

    pub(crate) fn update(
        &mut self,
        snapshot: DesktopAccessibilitySnapshot,
        title: &str,
        logical_size: [f32; 2],
        scale: f32,
        force_full: bool,
    ) -> TreeUpdate {
        let mut next = HashMap::with_capacity(snapshot.nodes.len() + 1);
        let mut root = Node::new(Role::Window);
        root.set_label(title);
        root.set_bounds(Rect::new(
            0.0,
            0.0,
            logical_size[0] as f64,
            logical_size[1] as f64,
        ));
        root.set_transform(Affine::scale(scale as f64));
        root.set_children(
            snapshot
                .roots
                .into_iter()
                .map(access_node_id)
                .collect::<Vec<_>>(),
        );
        next.insert(ROOT_ID, root);

        for semantic in snapshot.nodes {
            next.insert(access_node_id(semantic.id), access_node(semantic));
        }
        if !next.contains_key(&self.focus) {
            self.focus = ROOT_ID;
        }

        let full = force_full || self.previous.is_empty();
        let mut nodes = next
            .iter()
            .filter(|(id, node)| full || self.previous.get(id) != Some(*node))
            .map(|(id, node)| (*id, node.clone()))
            .collect::<Vec<_>>();
        nodes.sort_by_key(|(id, _)| id.0);
        self.previous = next;

        let tree = full.then(|| {
            let mut tree = TreeInfo::new(ROOT_ID);
            tree.toolkit_name = Some("Whisker".into());
            tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
            tree
        });
        TreeUpdate {
            nodes,
            tree,
            tree_id: TreeId::ROOT,
            focus: self.focus,
        }
    }

    pub(crate) fn handle_action(&mut self, request: &ActionRequest) -> DesktopAccessibilityAction {
        let Some(target) = self.previous.get(&request.target_node) else {
            return DesktopAccessibilityAction::Ignored;
        };
        if request.target_tree != TreeId::ROOT || !target.supports_action(request.action) {
            return DesktopAccessibilityAction::Ignored;
        }
        match request.action {
            Action::Focus if request.target_node != ROOT_ID => {
                self.focus = request.target_node;
                DesktopAccessibilityAction::FocusChanged
            }
            Action::Blur if self.focus == request.target_node => {
                self.focus = ROOT_ID;
                DesktopAccessibilityAction::FocusChanged
            }
            Action::Click => {
                let Some(node) = WhiskerNodeId::new(request.target_node.0) else {
                    return DesktopAccessibilityAction::Ignored;
                };
                self.focus = request.target_node;
                DesktopAccessibilityAction::Click(node)
            }
            _ => DesktopAccessibilityAction::Ignored,
        }
    }
}

fn access_node_id(id: WhiskerNodeId) -> AccessNodeId {
    AccessNodeId(id.get())
}

fn access_node(semantic: DesktopAccessibilityNode) -> Node {
    let role = semantic.semantics.role.map(access_role).unwrap_or_else(|| {
        if semantic.text.is_some() {
            Role::Label
        } else {
            Role::GenericContainer
        }
    });
    let actionable = matches!(
        role,
        Role::Button
            | Role::Link
            | Role::CheckBox
            | Role::RadioButton
            | Role::Switch
            | Role::Slider
            | Role::SearchInput
            | Role::Tab
    );
    let mut node = Node::new(role);
    node.set_children(
        semantic
            .children
            .into_iter()
            .map(access_node_id)
            .collect::<Vec<_>>(),
    );
    node.set_bounds(Rect::new(
        semantic.bounds.x as f64,
        semantic.bounds.y as f64,
        (semantic.bounds.x + semantic.bounds.width) as f64,
        (semantic.bounds.y + semantic.bounds.height) as f64,
    ));
    if let Some(label) = semantic.semantics.label {
        node.set_label(label);
    }
    if let Some(text) = semantic.text {
        node.set_value(text);
    }
    if let Some(hint) = semantic.semantics.hint {
        node.set_description(hint);
    }
    if let Some(identifier) = semantic.semantics.identifier {
        node.set_author_id(identifier);
    }
    if semantic.hidden || semantic.semantics.hidden {
        node.set_hidden();
    }
    if semantic.semantics.modal {
        node.set_modal();
    }
    if semantic.semantics.state.disabled == Some(true) {
        node.set_disabled();
    }
    if let Some(selected) = semantic.semantics.state.selected {
        node.set_selected(selected);
    }
    if let Some(expanded) = semantic.semantics.state.expanded {
        node.set_expanded(expanded);
    }
    if let Some(checked) = semantic.semantics.state.checked {
        node.set_toggled(match checked {
            AccessibilityChecked::Unchecked => Toggled::False,
            AccessibilityChecked::Checked => Toggled::True,
            AccessibilityChecked::Mixed => Toggled::Mixed,
            _ => Toggled::Mixed,
        });
    }
    if actionable && semantic.semantics.state.disabled != Some(true) {
        node.add_action(Action::Focus);
        node.add_action(Action::Blur);
        node.add_action(Action::Click);
    }
    node
}

fn access_role(role: AccessibilityRole) -> Role {
    match role {
        AccessibilityRole::Group => Role::Group,
        AccessibilityRole::Text => Role::Label,
        AccessibilityRole::Button => Role::Button,
        AccessibilityRole::Link => Role::Link,
        AccessibilityRole::Image => Role::Image,
        AccessibilityRole::Header => Role::Heading,
        AccessibilityRole::Checkbox => Role::CheckBox,
        AccessibilityRole::Radio => Role::RadioButton,
        AccessibilityRole::Switch => Role::Switch,
        AccessibilityRole::Adjustable => Role::Slider,
        AccessibilityRole::SearchBox => Role::SearchInput,
        AccessibilityRole::Tab => Role::Tab,
        _ => Role::GenericContainer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::AccessibilityState;

    fn id(value: u64) -> WhiskerNodeId {
        WhiskerNodeId::new(value).unwrap()
    }

    fn snapshot(accessibility: Accessibility) -> DesktopAccessibilitySnapshot {
        DesktopAccessibilitySnapshot {
            roots: vec![id(1)],
            nodes: vec![DesktopAccessibilityNode {
                id: id(1),
                children: Vec::new(),
                bounds: LayoutRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
                semantics: accessibility,
                text: None,
                hidden: false,
            }],
        }
    }

    #[test]
    fn full_tree_maps_semantics_state_geometry_and_actions() {
        let semantics = Accessibility::new()
            .label("Airplane mode")
            .hint("Toggles wireless radios")
            .identifier("airplane-mode")
            .role(AccessibilityRole::Switch)
            .state(
                AccessibilityState::new()
                    .checked(AccessibilityChecked::Checked)
                    .selected(false)
                    .expanded(false),
            );
        let mut bridge = DesktopAccessibilityBridge::default();

        let update = bridge.update(snapshot(semantics), "Example", [100.0, 80.0], 2.0, true);

        assert!(update.tree.is_some());
        assert_eq!(update.focus, ROOT_ID);
        let node = update
            .nodes
            .iter()
            .find_map(|(node_id, node)| (*node_id == AccessNodeId(1)).then_some(node))
            .unwrap();
        assert_eq!(node.role(), Role::Switch);
        assert_eq!(node.label(), Some("Airplane mode"));
        assert_eq!(node.description(), Some("Toggles wireless radios"));
        assert_eq!(node.author_id(), Some("airplane-mode"));
        assert_eq!(node.toggled(), Some(Toggled::True));
        assert_eq!(node.is_selected(), Some(false));
        assert_eq!(node.is_expanded(), Some(false));
        assert!(node.supports_action(Action::Click));
        assert_eq!(node.bounds(), Some(Rect::new(10.0, 20.0, 40.0, 60.0)));
    }

    #[test]
    fn unchanged_snapshots_emit_an_empty_incremental_update() {
        let mut bridge = DesktopAccessibilityBridge::default();
        let semantics = Accessibility::new().role(AccessibilityRole::Button);
        bridge.update(
            snapshot(semantics.clone()),
            "Example",
            [100.0, 80.0],
            1.0,
            true,
        );

        let update = bridge.update(snapshot(semantics), "Example", [100.0, 80.0], 1.0, false);

        assert!(update.tree.is_none());
        assert!(update.nodes.is_empty());
    }

    #[test]
    fn accessibility_click_focuses_and_targets_the_whisker_node() {
        let mut bridge = DesktopAccessibilityBridge::default();
        bridge.update(
            snapshot(Accessibility::new().role(AccessibilityRole::Button)),
            "Example",
            [100.0, 80.0],
            1.0,
            true,
        );
        let request = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: AccessNodeId(1),
            data: None,
        };

        assert_eq!(
            bridge.handle_action(&request),
            DesktopAccessibilityAction::Click(id(1))
        );
        let update = bridge.update(
            snapshot(Accessibility::new().role(AccessibilityRole::Button)),
            "Example",
            [100.0, 80.0],
            1.0,
            false,
        );
        assert_eq!(update.focus, AccessNodeId(1));
    }

    #[test]
    fn removing_a_subtree_updates_its_parent_and_restores_root_focus() {
        let mut bridge = DesktopAccessibilityBridge::default();
        bridge.update(
            snapshot(Accessibility::new().role(AccessibilityRole::Button)),
            "Example",
            [100.0, 80.0],
            1.0,
            true,
        );
        bridge.handle_action(&ActionRequest {
            action: Action::Focus,
            target_tree: TreeId::ROOT,
            target_node: AccessNodeId(1),
            data: None,
        });

        let update = bridge.update(
            DesktopAccessibilitySnapshot::default(),
            "Example",
            [100.0, 80.0],
            1.0,
            false,
        );

        assert_eq!(update.focus, ROOT_ID);
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].0, ROOT_ID);
        assert!(update.nodes[0].1.children().is_empty());
    }

    #[test]
    fn plain_text_is_exposed_without_explicit_semantics() {
        let mut semantic = snapshot(Accessibility::new());
        semantic.nodes[0].text = Some("Balance: $42".into());
        let mut bridge = DesktopAccessibilityBridge::default();

        let update = bridge.update(semantic, "Example", [100.0, 80.0], 1.0, true);

        let node = update
            .nodes
            .iter()
            .find_map(|(node_id, node)| (*node_id == AccessNodeId(1)).then_some(node))
            .unwrap();
        assert_eq!(node.role(), Role::Label);
        assert_eq!(node.value(), Some("Balance: $42"));
    }
}
