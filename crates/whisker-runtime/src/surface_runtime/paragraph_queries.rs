use super::*;
use crate::text_query::{Queries, TextQuery, TextQueryError, TextQueryFuture};
use whisker_protocol::TextRange;

impl SurfaceRuntime {
    pub(crate) fn step_text_queries(&self, timestamp: f64) -> bool {
        self.state
            .borrow()
            .text_queries
            .borrow_mut()
            .step(timestamp)
    }

    pub(super) fn enqueue_text_query(
        &self,
        handle: Element,
        query: TextQuery,
    ) -> Result<TextQueryFuture, TextQueryError> {
        let mut state = self.state.borrow_mut();
        let (node, snapshot) = state.paragraph_snapshot(handle)?;
        let mut arguments = query_arguments(
            &snapshot,
            match query {
                TextQuery::BoundingRects(range) => Some(range),
                _ => None,
            },
        )?;
        let (id, future) = Queries::register(&state.text_queries, node, snapshot, query)?;
        let WhiskerValue::Map(ref mut values) = arguments else {
            unreachable!()
        };
        values.insert("id".into(), WhiskerValue::Int(id));
        values.insert(
            "kind".into(),
            WhiskerValue::String(
                match query {
                    TextQuery::SelectedText => "selectedText",
                    TextQuery::BoundingRects(_) => "boundingRects",
                }
                .into(),
            ),
        );
        state
            .elements
            .get_mut(&handle)
            .ok_or(TextQueryError::NotBound)?
            .listeners
            .entry("textqueryresult".into())
            .or_default();
        let mask = state
            .surface
            .node(node)
            .and_then(|node| node.event_mask())
            .unwrap_or(0)
            | event_mask(&state.elements[&handle].kind, "textqueryresult");
        state
            .surface
            .set_event_mask(node, mask)
            .map_err(|error| TextQueryError::Host(error.to_string()))?;
        state
            .invoke_command(handle, "textQuery", &arguments)
            .map_err(|error| TextQueryError::Host(error.to_string()))?;
        crate::runtime_wake::wake_runtime();
        Ok(future)
    }

    pub(super) fn enqueue_text_selection(
        &self,
        handle: Element,
        range: Option<TextRange>,
    ) -> Result<(), TextQueryError> {
        let mut state = self.state.borrow_mut();
        let (_, snapshot) = state.paragraph_snapshot(handle)?;
        let arguments = query_arguments(&snapshot, range)?;
        state
            .invoke_command(
                handle,
                if range.is_some() {
                    "setSelection"
                } else {
                    "clearSelection"
                },
                &arguments,
            )
            .map_err(|error| TextQueryError::Host(error.to_string()))?;
        crate::runtime_wake::wake_runtime();
        Ok(())
    }
}

impl BindingState {
    fn paragraph_snapshot(
        &self,
        element: Element,
    ) -> Result<(NodeId, std::sync::Arc<whisker_engine::SceneNode>), TextQueryError> {
        let entry = self
            .elements
            .get(&element)
            .ok_or(TextQueryError::NotBound)?;
        if !entry.kind.is_rich_text() {
            return Err(TextQueryError::NotParagraph);
        }
        let node = entry.node.ok_or_else(|| {
            if self.paragraph_root(element) == element {
                TextQueryError::NotBound
            } else {
                TextQueryError::NotParagraph
            }
        })?;
        if !self.paragraph_input_is_current(node) {
            return Err(TextQueryError::StaleLayout);
        }
        let snapshot = self
            .surface
            .scene()
            .node_snapshot(node)
            .ok_or(TextQueryError::NotBound)?;
        if snapshot.layout().is_none() {
            return Err(TextQueryError::StaleLayout);
        }
        Ok((node, snapshot))
    }
}

fn query_arguments(
    snapshot: &whisker_engine::SceneNode,
    range: Option<TextRange>,
) -> Result<WhiskerValue, TextQueryError> {
    let text = snapshot.text().ok_or(TextQueryError::NotParagraph)?;
    let revision = text
        .prepared_content
        .and_then(|id| i64::try_from(id.get()).ok())
        .ok_or(TextQueryError::StaleLayout)?;
    let mut values = BTreeMap::from([("revision".into(), WhiskerValue::Int(revision))]);
    if let Some(range) = range {
        range
            .to_utf8(&text.payload.text)
            .ok_or(TextQueryError::InvalidRange)?;
        values.insert("start".into(), WhiskerValue::Int(i64::from(range.start)));
        values.insert("end".into(), WhiskerValue::Int(i64::from(range.end)));
    }
    Ok(WhiskerValue::Map(values))
}
