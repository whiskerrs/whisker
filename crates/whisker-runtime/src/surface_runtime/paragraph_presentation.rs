use super::*;

type TextLayoutNotification = (Rc<dyn Fn(WhiskerValue)>, WhiskerValue);

pub(super) struct PresentedParagraph {
    node: std::sync::Arc<whisker_engine::SceneNode>,
    ancestors: Vec<(NodeId, std::sync::Arc<whisker_engine::SceneNode>)>,
    geometry_revision: u64,
}

fn same_geometry(a: &whisker_engine::SceneNode, b: &whisker_engine::SceneNode) -> bool {
    std::ptr::eq(a, b)
        || (a.parent() == b.parent()
            && a.layout() == b.layout()
            && a.transform() == b.transform()
            && a.clip() == b.clip()
            && a.visibility() == b.visibility()
            && a.hit_test() == b.hit_test())
}

fn same_paragraph(a: &whisker_engine::SceneNode, b: &whisker_engine::SceneNode) -> bool {
    if !same_geometry(a, b) {
        return false;
    }
    let (Some(a), Some(b)) = (a.text(), b.text()) else {
        return false;
    };
    a.payload == b.payload
        && a.runs
            .iter()
            .map(|run| (run.range, run.span))
            .eq(b.runs.iter().map(|run| (run.range, run.span)))
}

impl BindingState {
    pub(super) fn commit_paragraph_presentations(&mut self, changed: &HashSet<NodeId>) {
        self.presented_paragraphs
            .retain(|node, _| self.surface.node(*node).is_some());
        let mut affected = changed.clone();
        affected.extend(
            self.presented_paragraphs
                .iter()
                .filter(|(_, paragraph)| {
                    paragraph
                        .ancestors
                        .iter()
                        .any(|(node, _)| changed.contains(node))
                })
                .map(|(node, _)| *node),
        );
        for id in affected {
            let Some(node) = self
                .surface
                .scene()
                .node_snapshot(id)
                .filter(|node| node.text().is_some_and(|text| text.paragraph.is_some()))
            else {
                self.presented_paragraphs.remove(&id);
                continue;
            };
            let mut ancestors = Vec::new();
            let mut parent = node.parent();
            while let Some(id) = parent {
                let Some(ancestor) = self.surface.scene().node_snapshot(id) else {
                    break;
                };
                parent = ancestor.parent();
                ancestors.push((id, ancestor));
            }
            let previous = self.presented_paragraphs.get(&id).filter(|previous| {
                same_paragraph(&node, &previous.node)
                    && previous.ancestors.len() == ancestors.len()
                    && previous
                        .ancestors
                        .iter()
                        .zip(&ancestors)
                        .all(|((a_id, a), (b_id, b))| a_id == b_id && same_geometry(a, b))
            });
            let geometry_revision = previous
                .map_or(self.surface.scene().accepted_revision(), |previous| {
                    previous.geometry_revision
                });
            self.presented_paragraphs.insert(
                id,
                PresentedParagraph {
                    node,
                    ancestors,
                    geometry_revision,
                },
            );
        }
    }

    pub(super) fn paragraph_input_is_current(&self, node: NodeId) -> bool {
        self.paragraph_input_is_current_at(node, None)
    }

    pub(super) fn paragraph_input_is_current_at(
        &self,
        node: NodeId,
        revision: Option<u64>,
    ) -> bool {
        let mut current_id = Some(node);
        while let Some(id) = current_id {
            let Some(current) = self.surface.node(id) else {
                return false;
            };
            if id == node && current.visibility() == Some(whisker_protocol::Visibility::Hidden) {
                return false;
            }
            if let Some(presented) = self.presented_paragraphs.get(&id) {
                if revision.is_some_and(|revision| revision < presented.geometry_revision)
                    || !same_paragraph(current, &presented.node)
                    || presented.ancestors.iter().any(|(id, previous)| {
                        self.surface
                            .node(*id)
                            .is_none_or(|current| !same_geometry(current, previous))
                    })
                {
                    return false;
                }
            } else if current
                .text()
                .is_some_and(|text| !text.runs.is_empty() || text.paragraph.is_some())
            {
                return false;
            }
            current_id = current.parent();
        }
        true
    }

    pub(super) fn paragraph_action(&self, node: NodeId, detail: &WhiskerValue) -> Option<Element> {
        if !self.paragraph_input_is_current(node) {
            return None;
        }
        let WhiskerValue::Map(fields) = detail else {
            return None;
        };
        let (Some(WhiskerValue::Int(span)), Some(WhiskerValue::Int(revision))) =
            (fields.get("span"), fields.get("revision"))
        else {
            return None;
        };
        let content = self.surface.node(node)?.text()?;
        if u64::try_from(*revision).ok() != content.prepared_content.map(|id| id.get()) {
            return None;
        }
        let span = whisker_protocol::TextSpanId::new(u64::try_from(*span).ok()?)?;
        if !content
            .accessible_actions()
            .iter()
            .any(|action| action.0 == span)
        {
            return None;
        }
        Some(Element::from_raw(u32::try_from(span.get() - 1).ok()?))
    }

    pub(super) fn text_layout_notifications(&mut self) -> Vec<TextLayoutNotification> {
        let changed = self
            .text_layout_observed
            .iter()
            .filter_map(|element| {
                let entry = self.elements.get(element)?;
                let listeners = entry.listeners.get("textlayout")?;
                if listeners.is_empty() || !entry.kind.is_rich_text() {
                    return None;
                }
                let node = self.surface.node(entry.node?)?;
                let text = node.text()?;
                let content = node.layout()?.content_box;
                let lines = text
                    .paragraph
                    .as_ref()
                    .map(|paragraph| paragraph.lines.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|line| {
                        WhiskerValue::map([
                            ("start", WhiskerValue::Int(i64::from(line.range.start))),
                            ("end", WhiskerValue::Int(i64::from(line.range.end))),
                            (
                                "ellipsisCount",
                                WhiskerValue::Int(i64::from(line.ellipsis_count)),
                            ),
                        ])
                    })
                    .collect::<Vec<_>>();
                let detail = WhiskerValue::map([
                    ("lineCount", WhiskerValue::Int(lines.len() as i64)),
                    ("lines", WhiskerValue::Array(lines)),
                    (
                        "size",
                        WhiskerValue::map([
                            ("width", WhiskerValue::Float(f64::from(content.width))),
                            ("height", WhiskerValue::Float(f64::from(content.height))),
                        ]),
                    ),
                ]);
                (entry.last_text_layout.as_ref() != Some(&detail)).then_some((*element, detail))
            })
            .collect::<Vec<_>>();
        let mut notifications = Vec::new();
        for (element, detail) in changed {
            let target = self.element_target_value(element);
            let body = WhiskerValue::map([
                ("type", WhiskerValue::String("textlayout".into())),
                ("target", target.clone()),
                ("currentTarget", target),
                ("detail", detail.clone()),
            ]);
            let entry = self.elements.get_mut(&element).expect("retained paragraph");
            entry.last_text_layout = Some(detail);
            notifications.extend(
                entry.listeners["textlayout"]
                    .iter()
                    .map(|listener| (listener.callback.clone(), body.clone())),
            );
        }
        notifications
    }
}
pub(super) struct ParagraphFrameSink<'a, Sink> {
    sink: &'a mut Sink,
    presented: &'a HashMap<NodeId, PresentedParagraph>,
    pub changed: HashSet<NodeId>,
}

impl<'a, Sink> ParagraphFrameSink<'a, Sink> {
    pub fn new(sink: &'a mut Sink, presented: &'a HashMap<NodeId, PresentedParagraph>) -> Self {
        Self {
            sink,
            presented,
            changed: HashSet::new(),
        }
    }
}

impl<Sink: FrameSink> FrameSink for ParagraphFrameSink<'_, Sink> {
    type Error = Sink::Error;
    fn capabilities(&self) -> whisker_engine::whisker_protocol::RenderCapabilities {
        self.sink.capabilities()
    }
    fn present(
        &mut self,
        packet: &whisker_engine::whisker_protocol::FramePacket,
    ) -> Result<whisker_engine::whisker_protocol::ApplyResult, Self::Error> {
        use whisker_engine::whisker_protocol::{ApplyResult, Operation};
        let result = self.sink.present(packet)?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            self.changed.extend(
                packet
                    .operations
                    .iter()
                    .filter_map(|operation| match operation {
                        Operation::SetText { node, content }
                            if content.paragraph.is_some() || self.presented.contains_key(node) =>
                        {
                            Some(*node)
                        }
                        Operation::SetLayout { node, .. }
                        | Operation::SetTransform { node, .. }
                        | Operation::SetClip { node, .. }
                        | Operation::SetVisibility { node, .. }
                        | Operation::SetHitTest { node, .. }
                            if !self.presented.is_empty() =>
                        {
                            Some(*node)
                        }
                        Operation::InsertChild { child, .. }
                        | Operation::RemoveChild { child, .. }
                        | Operation::MoveChild { child, .. }
                            if !self.presented.is_empty() =>
                        {
                            Some(*child)
                        }
                        _ => None,
                    }),
            );
        }
        Ok(result)
    }
}
