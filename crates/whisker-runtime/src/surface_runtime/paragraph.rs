use super::*;
use whisker_engine::{
    InlineAttachmentInput, ResolvedTextRun,
    whisker_protocol::{TextByteRange, TextSpanId},
};

pub(super) struct CompiledParagraph {
    root: Element,
    pub input: PlainTextInput,
    pub runs: Vec<ResolvedTextRun>,
    pub attachments: Vec<InlineAttachmentInput>,
    pub resolved: Vec<(Element, ResolvedNodeStyle)>,
}

impl BindingState {
    pub(super) fn paragraph_root(&self, element: Element) -> Element {
        let mut root = element;
        while let Some(parent) = self.elements.get(&root).and_then(|entry| entry.parent) {
            if !self
                .elements
                .get(&parent)
                .is_some_and(|entry| entry.kind.is_rich_text())
            {
                break;
            }
            root = parent;
        }
        root
    }

    pub(super) fn paragraph_event_mask(&self, element: Element) -> u64 {
        let Some(entry) = self.elements.get(&element) else {
            return 0;
        };
        let own = entry
            .listeners
            .keys()
            .fold(0, |mask, name| mask | event_mask(&entry.kind, name));
        let own = own
            | if entry
                .listeners
                .get("tap")
                .is_some_and(|listeners| !listeners.is_empty())
            {
                event_mask(&entry.kind, "textactivate")
            } else {
                0
            };
        let own = own
            | if entry.text_selectable == Some(true) {
                event_mask(&entry.kind, "selectionchange")
            } else {
                0
            };
        entry
            .children
            .iter()
            .filter(|child| {
                self.elements
                    .get(child)
                    .is_some_and(|entry| entry.kind.is_rich_text())
            })
            .fold(own, |mask, child| mask | self.paragraph_event_mask(*child))
    }

    pub(super) fn paragraph_has_listener(&self, element: Element, name: &str) -> bool {
        let Some(entry) = self
            .elements
            .get(&element)
            .filter(|entry| entry.kind.is_rich_text())
        else {
            return false;
        };
        entry
            .listeners
            .get(name)
            .is_some_and(|listeners| !listeners.is_empty())
            || entry
                .children
                .iter()
                .any(|child| self.paragraph_has_listener(*child, name))
    }

    pub(super) fn materialize_text(&mut self, element: Element) -> Result<(), RuntimeBindingError> {
        let entry = self.element(element)?;
        if entry.node.is_some() || !entry.kind.is_rich_text() {
            return Ok(());
        }
        let registration = entry.kind.registration().expect("registered Text");
        let resolved = entry.resolved.as_ref().expect("registered style");
        let node = self.surface.create_node(
            registration.element_type,
            resolved.computed().layout().clone(),
        )?;
        self.element_mut(element)?.node = Some(node);
        self.node_elements.insert(node, element);
        let entry = self.element(element)?;
        let accessibility = entry.accessibility.clone();
        let mask = entry
            .listeners
            .keys()
            .fold(0, |mask, name| mask | event_mask(&entry.kind, name));
        self.surface.set_accessibility(node, accessibility)?;
        self.surface.set_event_mask(node, mask)?;
        self.sync_paragraph_children(element)
    }

    pub(super) fn dematerialize_inline_text(
        &mut self,
        element: Element,
    ) -> Result<(), RuntimeBindingError> {
        if !self.element(element)?.kind.is_rich_text() {
            return Ok(());
        }
        let children = self.element(element)?.children.clone();
        if let Some(node) = self.element(element)?.node {
            let scene_children = self
                .surface
                .node(node)
                .map(|entry| entry.children().to_vec())
                .unwrap_or_default();
            for child in scene_children {
                self.surface.remove_child(node, child)?;
            }
            self.surface.delete_node(node)?;
            self.node_elements.remove(&node);
            self.element_mut(element)?.node = None;
        }
        for child in children {
            self.dematerialize_inline_text(child)?;
        }
        Ok(())
    }

    pub(super) fn compile_paragraph(
        &self,
        root: Element,
        input: &PlainTextInput,
        resolved: &ResolvedNodeStyle,
        environment: StyleEnvironment,
    ) -> Result<CompiledParagraph, RuntimeBindingError> {
        let mut result = CompiledParagraph {
            root,
            input: input.clone(),
            runs: Vec::new(),
            attachments: Vec::new(),
            resolved: Vec::new(),
        };
        result.input.text.clear();
        let observed = self.element(root)?.listeners.contains_key("textlayout")
            || self.element(root)?.text_selectable == Some(true)
            || self.element(root)?.text_geometry;
        let styled = observed
            || self.element(root)?.children.iter().any(|child| {
                self.elements
                    .get(child)
                    .is_some_and(|entry| !entry.kind.is_raw_text())
            });
        self.collect_paragraph(root, resolved, environment, styled, &mut result)?;
        let plain = result
            .runs
            .iter()
            .all(|run| run.span.get() == u64::from(root.0) + 1);
        if plain && !observed {
            result.runs.clear();
        }
        if let Some(token) = result
            .attachments
            .iter_mut()
            .find(|attachment| attachment.truncation)
        {
            let end = result.input.text.len().try_into().map_err(|_| {
                RuntimeBindingError::InvalidParagraph {
                    element: root,
                    message: "text exceeds the supported byte range",
                }
            })?;
            token.range = TextByteRange { start: end, end };
        }
        result
            .attachments
            .sort_by_key(|attachment| attachment.range.start);
        Ok(result)
    }

    pub(super) fn sync_paragraph_children(
        &mut self,
        element: Element,
    ) -> Result<(), RuntimeBindingError> {
        let root = self.paragraph_root(element);
        let Some(node) = self.element(root)?.node else {
            return Ok(());
        };
        let mut desired = Vec::new();
        self.collect_inline_nodes(root, &mut desired)?;
        let current = self
            .surface
            .node(node)
            .expect("retained paragraph")
            .children()
            .to_vec();
        for child in current {
            if !desired.contains(&child) {
                self.surface.remove_child(node, child)?;
            }
        }
        for (index, child) in desired.into_iter().enumerate() {
            if self
                .surface
                .node(node)
                .expect("retained paragraph")
                .children()
                .get(index)
                == Some(&child)
            {
                continue;
            }
            if let Some(parent) = self.surface.node(child).and_then(|entry| entry.parent()) {
                self.surface.remove_child(parent, child)?;
            }
            self.surface.insert_child(node, child, index as u32)?;
        }
        Ok(())
    }

    fn diagnose_inline_style(&self, element: Element) {
        #[cfg(debug_assertions)]
        if let Some(entry) = self.elements.get(&element) {
            for declaration in entry.effective_specified().declarations() {
                let property = declaration.property();
                if !property.supports_text_scope(whisker_engine::whisker_style::TextStyleScope::Run)
                    && self
                        .text_style_diagnostics
                        .borrow_mut()
                        .insert((element, Some(property)))
                {
                    eprintln!(
                        "[whisker] `{}` is ignored on inline Text {:?}; put paragraph properties on the outer Text and box properties on an inline View",
                        property.css_name(),
                        element
                    );
                }
            }
            if (entry.text_selectable.is_some()
                || entry
                    .text
                    .as_ref()
                    .is_some_and(|text| text.max_lines.is_some()))
                && self
                    .text_style_diagnostics
                    .borrow_mut()
                    .insert((element, None))
            {
                eprintln!(
                    "[whisker] selectable and max_lines apply only to the outer Text, not inline Text {element:?}"
                );
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = element;
    }

    fn has_truncation_ancestor(&self, element: Element) -> bool {
        let mut current = Some(element);
        while let Some(element) = current {
            let Some(entry) = self.elements.get(&element) else {
                break;
            };
            if entry.inline_truncation {
                return true;
            }
            current = entry.parent;
        }
        false
    }

    fn collect_inline_nodes(
        &self,
        element: Element,
        nodes: &mut Vec<NodeId>,
    ) -> Result<(), RuntimeBindingError> {
        for child in &self.element(element)?.children {
            let entry = self.element(*child)?;
            if entry.kind.is_rich_text() {
                self.collect_inline_nodes(*child, nodes)?;
            } else if let Some(node) = entry.node {
                nodes.push(node);
            }
        }
        Ok(())
    }

    fn inline_action(&self, mut element: Element, root: Element) -> Option<TextSpanId> {
        while element != root {
            let entry = self.elements.get(&element)?;
            if entry.accessibility.hidden || entry.accessibility.state.disabled == Some(true) {
                return None;
            }
            if entry
                .listeners
                .get("tap")
                .is_some_and(|listeners| !listeners.is_empty())
            {
                return TextSpanId::new(u64::from(element.0) + 1);
            }
            element = entry.parent?;
        }
        None
    }

    fn collect_paragraph(
        &self,
        element: Element,
        resolved: &ResolvedNodeStyle,
        environment: StyleEnvironment,
        styled: bool,
        result: &mut CompiledParagraph,
    ) -> Result<(), RuntimeBindingError> {
        for child in &self.element(element)?.children {
            let entry = self.element(*child)?;
            if entry.kind.is_raw_text() {
                if entry.raw_text.is_empty() {
                    continue;
                }
                let start = result.input.text.len();
                result.input.text.push_str(&entry.raw_text);
                if !styled {
                    continue;
                }
                let range = TextByteRange {
                    start: start
                        .try_into()
                        .map_err(|_| RuntimeBindingError::InvalidParagraph {
                            element,
                            message: "text exceeds the supported byte range",
                        })?,
                    end: result.input.text.len().try_into().map_err(|_| {
                        RuntimeBindingError::InvalidParagraph {
                            element,
                            message: "text exceeds the supported byte range",
                        }
                    })?,
                };
                result.runs.push(ResolvedTextRun {
                    range,
                    span: TextSpanId::new(u64::from(element.0) + 1).expect("nonzero span"),
                    style: resolved.computed().clone(),
                    action: self.inline_action(element, result.root),
                    inline_box_paint: element != result.root,
                });
            } else if entry.kind.is_rich_text() {
                self.diagnose_inline_style(*child);
                let child_style = resolve_style(
                    &entry.effective_specified(),
                    Some(resolved.inherited_for_children()),
                    environment,
                )?;
                self.collect_paragraph(*child, &child_style, environment, styled, result)?;
                result.resolved.push((*child, child_style));
            } else if entry.kind.registration().is_some_and(|registration| {
                matches!(
                    registration.name.as_str(),
                    "whisker.ui/View" | "whisker-image:Image"
                )
            }) {
                let child_style = resolve_style(
                    &entry.effective_specified(),
                    Some(resolved.inherited_for_children()),
                    environment,
                )?;
                let alignment = match child_style.computed().vertical_align() {
                    whisker_engine::whisker_style::ComputedVerticalAlign::Baseline => {
                        whisker_engine::whisker_protocol::InlineAlignment::Baseline
                    }
                    whisker_engine::whisker_style::ComputedVerticalAlign::Top => {
                        whisker_engine::whisker_protocol::InlineAlignment::Top
                    }
                    whisker_engine::whisker_style::ComputedVerticalAlign::Middle => {
                        whisker_engine::whisker_protocol::InlineAlignment::Middle
                    }
                    whisker_engine::whisker_style::ComputedVerticalAlign::Bottom => {
                        whisker_engine::whisker_protocol::InlineAlignment::Bottom
                    }
                    whisker_engine::whisker_style::ComputedVerticalAlign::Offset(value) => {
                        whisker_engine::whisker_protocol::InlineAlignment::Offset(value.get())
                    }
                };
                let start = result.input.text.len();
                if entry.inline_truncation {
                    if element != result.root
                        || result.attachments.iter().any(|a| a.truncation)
                        || self.has_truncation_ancestor(result.root)
                    {
                        return Err(RuntimeBindingError::InvalidParagraph {
                            element: *child,
                            message: "InlineTruncation must be the only truncation token and a direct child of the outer Text",
                        });
                    }
                } else {
                    result.input.text.push('\u{fffc}');
                }
                let range = TextByteRange {
                    start: start
                        .try_into()
                        .map_err(|_| RuntimeBindingError::InvalidParagraph {
                            element,
                            message: "text exceeds the supported byte range",
                        })?,
                    end: result.input.text.len().try_into().map_err(|_| {
                        RuntimeBindingError::InvalidParagraph {
                            element,
                            message: "text exceeds the supported byte range",
                        }
                    })?,
                };
                if !entry.inline_truncation {
                    result.runs.push(ResolvedTextRun {
                        range,
                        span: TextSpanId::new(u64::from(element.0) + 1).expect("nonzero span"),
                        style: resolved.computed().clone(),
                        action: self.inline_action(element, result.root),
                        inline_box_paint: element != result.root,
                    });
                }
                result.attachments.push(InlineAttachmentInput {
                    truncation: entry.inline_truncation,
                    label: entry.accessibility.label.clone(),
                    node: entry
                        .node
                        .ok_or(RuntimeBindingError::UnknownElement { element: *child })?,
                    range,
                    alignment,
                });
            } else {
                return Err(RuntimeBindingError::InvalidParagraph {
                    element: *child,
                    message: "Text accepts Text, Image, and View; wrap other elements in View",
                });
            }
        }
        Ok(())
    }
}
