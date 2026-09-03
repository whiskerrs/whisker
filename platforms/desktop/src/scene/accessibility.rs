use super::*;
use crate::accessibility::{DesktopAccessibilityNode, DesktopAccessibilitySnapshot};

impl DesktopScene {
    pub(crate) fn accessibility_snapshot(&self) -> DesktopAccessibilitySnapshot {
        let mut roots = self
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                node.presentation
                    .parent
                    .is_none()
                    .then_some((*id, node.presentation.z_order))
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|(id, z_order)| (*z_order, id.get()));
        let root_ids = roots.iter().map(|(id, _)| *id).collect();
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for (root, _) in roots {
            self.collect_accessibility(root, PresentationContext::default(), false, &mut nodes);
        }
        DesktopAccessibilitySnapshot {
            roots: root_ids,
            nodes,
        }
    }

    fn collect_accessibility(
        &self,
        id: NodeId,
        context: PresentationContext,
        ancestor_hidden: bool,
        nodes: &mut Vec<DesktopAccessibilityNode>,
    ) {
        let node = self.nodes.get(&id).expect("retained child remains live");
        let presentation = &node.presentation;
        let border = LayoutRect {
            x: context.origin[0] + presentation.layout.border_box.x,
            y: context.origin[1] + presentation.layout.border_box.y,
            width: presentation.layout.border_box.width,
            height: presentation.layout.border_box.height,
        };
        let transform = multiply_transform(
            context.transform,
            transform_around(presentation.transform, border.x, border.y),
        );
        let bounds = transform_rect_aabb(border, transform).unwrap_or(border);
        let hidden = ancestor_hidden
            || presentation.visibility != Visibility::Visible
            || presentation.accessibility.hidden;
        nodes.push(DesktopAccessibilityNode {
            id,
            children: presentation.children.clone(),
            bounds,
            semantics: presentation.accessibility.clone(),
            text: node.content.text().map(|text| text.payload.text.clone()),
            hidden,
        });

        let child_origin = if node.content.is_scroll_container() {
            [
                border.x - node.scroll_offset[0],
                border.y - node.scroll_offset[1],
            ]
        } else {
            [border.x, border.y]
        };
        for child in &presentation.children {
            self.collect_accessibility(
                *child,
                PresentationContext {
                    origin: child_origin,
                    transform,
                    ..PresentationContext::default()
                },
                hidden,
                nodes,
            );
        }
    }
}
