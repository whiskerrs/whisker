use super::*;
use crate::text::NativeTextHost;

impl DesktopScene {
    pub(crate) fn resolve_text_queries(&mut self, host: &NativeTextHost) {
        for id in self.pending_text_queries.drain() {
            if let Some(text) = self
                .nodes
                .get_mut(&id)
                .and_then(|node| node.content.text_state_mut())
            {
                text.resolve_queries(host);
            }
        }
    }
}

impl DesktopScene {
    pub(crate) fn text_pointer(
        &mut self,
        host: &NativeTextHost,
        event: &whisker_protocol::InputEvent,
        mut target: Option<NodeId>,
    ) -> bool {
        use whisker_protocol::InputEventKind;
        let Some(pointer) = event
            .pointer
            .filter(|p| p.kind == whisker_protocol::PointerKind::Mouse)
        else {
            return false;
        };
        match event.kind {
            InputEventKind::PointerDown if pointer.changed_button == 0 => {
                while let Some(id) = target {
                    let Some(node) = self.nodes.get(&id) else {
                        target = None;
                        break;
                    };
                    if node.content.text_state().is_some_and(|t| t.selectable) {
                        break;
                    }
                    target = node.presentation.parent;
                }
                if self.selected_paragraph != target {
                    if let Some(old) = self
                        .selected_paragraph
                        .and_then(|id| self.nodes.get_mut(&id))
                        .and_then(|n| n.content.text_state_mut())
                    {
                        old.focused = false;
                        old.dragging = false;
                        old.set_selection(None);
                    }
                }
                self.selected_paragraph = target;
                let Some(id) = target else {
                    return false;
                };
                let Some(position) =
                    self.text_position(host, id, [pointer.position.x, pointer.position.y])
                else {
                    return false;
                };
                let text = self
                    .nodes
                    .get_mut(&id)
                    .and_then(|n| n.content.text_state_mut())
                    .unwrap();
                text.focused = true;
                text.dragging = true;
                text.select_to(position, false);
                true
            }
            InputEventKind::PointerMove => {
                let Some(id) = self.selected_paragraph.filter(|id| {
                    self.nodes
                        .get(id)
                        .and_then(|n| n.content.text_state())
                        .is_some_and(|t| t.dragging)
                }) else {
                    return false;
                };
                let Some(position) =
                    self.text_position(host, id, [pointer.position.x, pointer.position.y])
                else {
                    return false;
                };
                self.nodes
                    .get_mut(&id)
                    .and_then(|n| n.content.text_state_mut())
                    .unwrap()
                    .select_to(position, true);
                true
            }
            InputEventKind::PointerUp | InputEventKind::PointerCancel => {
                if let Some(text) = self
                    .selected_paragraph
                    .and_then(|id| self.nodes.get_mut(&id))
                    .and_then(|n| n.content.text_state_mut())
                {
                    text.dragging = false;
                }
                false
            }
            _ => false,
        }
    }

    fn text_position(
        &self,
        host: &NativeTextHost,
        node: NodeId,
        mut point: [f32; 2],
    ) -> Option<u32> {
        let content = self.nodes.get(&node)?.content.text()?;
        let prepared = host
            .prepared
            .get(&content.prepared_content?)?
            .paragraph
            .as_ref()?;
        let mut path = Vec::new();
        let mut target = Some(node);
        while let Some(id) = target {
            path.push(id);
            target = self.nodes.get(&id)?.presentation.parent;
        }
        let mut origin = [0.0, 0.0];
        for id in path.into_iter().rev() {
            let state = self.nodes.get(&id)?;
            let border = state.presentation.layout.border_box;
            origin[0] += border.x;
            origin[1] += border.y;
            point = transaction::inverse_map_around(state.presentation.transform, point, origin)?;
            if id == node {
                let rect = state.presentation.layout.content_box;
                point[0] -= origin[0] + rect.x;
                point[1] -= origin[1] + rect.y;
            } else if state.content.is_scroll_container() {
                origin[0] -= state.scroll_offset[0];
                origin[1] -= state.scroll_offset[1];
            }
        }
        prepared.position_at(&content.payload, point)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{element_type, geometry, id, packet, scene, text};
    use super::*;
    use whisker_engine::MeasurementProvider;
    use whisker_protocol::*;

    fn field<'a>(value: &'a WhiskerValue, name: &str) -> Option<&'a WhiskerValue> {
        if let WhiskerValue::Map(map) = value {
            map.get(name)
        } else {
            None
        }
    }

    #[test]
    fn selection_uses_accepted_paragraph_and_recycled_text_rebinds_its_event_target() {
        let mut scene = scene(SurfaceId::new(1).unwrap());
        let mut host = NativeTextHost::new(scene.elements.clone());
        let mut content = text();
        content.payload.text = "Hello 🦀 world".into();
        content.payload.runs = vec![TextMeasureRun {
            alignment: Default::default(),
            range: TextByteRange {
                start: 0,
                end: content.payload.text.len() as u32,
            },
            style: content.payload.style.clone(),
        }];
        let mut responses = Vec::new();
        host.measure_batch(
            SurfaceId::new(1).unwrap(),
            &[MeasurementRequest {
                key: MeasurementKey::new(7).unwrap(),
                node: id(1),
                element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                environment_epoch: 1,
                constraints: MeasureConstraints {
                    known_dimensions: [None, None],
                    available_space: [AvailableSpace::Definite(400.0), AvailableSpace::MaxContent],
                },
                payload: MeasurementPayload::Text(content.payload.clone()),
            }],
            &mut responses,
        )
        .unwrap();
        let MeasurementResponse::Ready { metrics, .. } = &responses[0] else {
            panic!()
        };
        content.prepared_content = metrics.prepared_content;
        content.paragraph = metrics.paragraph.clone();
        let revision = content.prepared_content.unwrap().get() as i64;
        scene
            .present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: id(1),
                        element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                    },
                    Operation::SetLayout {
                        node: id(1),
                        geometry: geometry(0.0, 0.0, 400.0, 120.0),
                    },
                    Operation::SetText {
                        node: id(1),
                        content: content.clone(),
                    },
                    Operation::SetProperty {
                        node: id(1),
                        property: PropertyId::new(1).unwrap(),
                        value: WhiskerValue::Bool(true),
                    },
                    Operation::SetEventMask {
                        node: id(1),
                        event_mask: 3,
                    },
                ],
            ))
            .unwrap();
        let query = |revision, kind| {
            WhiskerValue::map([
                ("revision", WhiskerValue::Int(revision)),
                ("id", WhiskerValue::Int(42)),
                ("kind", WhiskerValue::String(kind)),
                ("start", WhiskerValue::Int(6)),
                ("end", WhiskerValue::Int(8)),
            ])
        };
        scene
            .present(&packet(
                FrameMode::Delta,
                1,
                2,
                vec![
                    Operation::InvokeCommand {
                        node: id(1),
                        command: CommandId::new(1).unwrap(),
                        arguments: query(revision, "".into()),
                    },
                    Operation::InvokeCommand {
                        node: id(1),
                        command: CommandId::new(3).unwrap(),
                        arguments: query(revision, "selectedText".into()),
                    },
                ],
            ))
            .unwrap();
        scene.resolve_text_queries(&host);
        let replies = scene.take_events();
        assert!(replies.iter().any(|event| event.name == "textqueryresult"
            && field(&event.detail, "text").and_then(|v| if let WhiskerValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }) == Some("🦀")));
        let text = scene.nodes[&id(1)].content.text_state().unwrap();
        let rects = host.prepared[&content.prepared_content.unwrap()]
            .paragraph
            .as_ref()
            .unwrap()
            .selection_rects(&content.payload, text.selection().unwrap())
            .unwrap();
        assert!(!rects.is_empty());
        scene
            .present(&packet(
                FrameMode::Delta,
                2,
                3,
                vec![Operation::DeleteNode { node: id(1) }],
            ))
            .unwrap();
        scene
            .present(&packet(
                FrameMode::Delta,
                3,
                4,
                vec![
                    Operation::CreateNode {
                        node: id(2),
                        element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                    },
                    Operation::SetText {
                        node: id(2),
                        content,
                    },
                    Operation::SetEventMask {
                        node: id(2),
                        event_mask: 3,
                    },
                    Operation::InvokeCommand {
                        node: id(2),
                        command: CommandId::new(3).unwrap(),
                        arguments: query(revision + 1, "selectedText".into()),
                    },
                ],
            ))
            .unwrap();
        scene.resolve_text_queries(&host);
        let replies = scene.take_events();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].target, id(2));
        assert_eq!(
            field(&replies[0].detail, "error").and_then(|v| if let WhiskerValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }),
            Some("stale-layout")
        );
        assert!(
            scene.nodes[&id(2)]
                .content
                .text_state()
                .unwrap()
                .selection()
                .is_none()
        );
    }
}
