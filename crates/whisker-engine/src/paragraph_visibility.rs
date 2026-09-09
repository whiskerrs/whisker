use std::collections::HashMap;

use whisker_layout::{LayoutParticipation, LayoutSnapshot};
use whisker_protocol::{Accessibility, NodeId, Visibility};

use crate::Scene;

#[derive(Clone, Debug)]
struct Presentation {
    visibility: Visibility,
    accessibility: Accessibility,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ParagraphVisibility {
    suppressed: HashMap<NodeId, Presentation>,
}

impl ParagraphVisibility {
    pub(crate) fn contains(&self, node: NodeId) -> bool {
        self.suppressed.contains_key(&node)
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.suppressed.is_empty()
    }

    pub(crate) fn remove(&mut self, node: NodeId) {
        self.suppressed.remove(&node);
    }

    pub(crate) fn visibility(&mut self, node: NodeId, value: Visibility) -> Visibility {
        if let Some(presentation) = self.suppressed.get_mut(&node) {
            presentation.visibility = value;
            Visibility::Hidden
        } else {
            value
        }
    }

    pub(crate) fn accessibility(
        &mut self,
        node: NodeId,
        mut value: Accessibility,
    ) -> Accessibility {
        if let Some(presentation) = self.suppressed.get_mut(&node) {
            presentation.accessibility = value.clone();
            value.hidden = true;
        }
        value
    }

    pub(crate) fn project(&mut self, scene: &mut Scene, snapshot: &LayoutSnapshot) {
        for (node, _) in snapshot.iter() {
            if snapshot.participation(node) == Some(LayoutParticipation::SuppressedByParagraph) {
                if self.suppressed.contains_key(&node) {
                    continue;
                }
                let state = scene
                    .node(node)
                    .expect("layout references retained scene nodes");
                let presentation = Presentation {
                    visibility: state.visibility().unwrap_or(Visibility::Visible),
                    accessibility: state.accessibility().cloned().unwrap_or_default(),
                };
                let mut accessibility = presentation.accessibility.clone();
                accessibility.hidden = true;
                self.suppressed.insert(node, presentation);
                scene
                    .set_visibility(node, Visibility::Hidden)
                    .expect("mutable retained inline node");
                scene
                    .set_accessibility(node, accessibility)
                    .expect("mutable retained inline node");
            } else if let Some(presentation) = self.suppressed.remove(&node) {
                scene
                    .set_visibility(node, presentation.visibility)
                    .expect("mutable retained inline node");
                scene
                    .set_accessibility(node, presentation.accessibility)
                    .expect("mutable retained inline node");
            }
        }
    }
}
