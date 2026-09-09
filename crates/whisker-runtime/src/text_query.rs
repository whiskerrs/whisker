use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_channel::oneshot;
use whisker_engine::SceneNode;
use whisker_protocol::{LayoutRect, NodeId, TextRange, WhiskerValue};

use crate::lifetime::Scoped;

/// Failure of a paragraph operation or an asynchronous text query.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextQueryError {
    /// The handle or its element is no longer mounted.
    NotBound,
    /// A nested Text contributes runs but is not an independent paragraph.
    NotParagraph,
    /// The displayed paragraph changed before the operation completed.
    StaleLayout,
    /// The caller's Owner or the query future was disposed.
    Cancelled,
    /// The range is reversed, out of bounds, or splits a surrogate pair.
    InvalidRange,
    /// The Host did not complete the request within five seconds of active frames.
    Timeout,
    /// The Host rejected the operation or returned a malformed response.
    Host(String),
}

impl std::fmt::Display for TextQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotBound => f.write_str("text handle is not bound"),
            Self::NotParagraph => f.write_str("text handle is not a paragraph root"),
            Self::StaleLayout => f.write_str("paragraph layout is stale"),
            Self::Cancelled => f.write_str("text query was cancelled"),
            Self::InvalidRange => f.write_str("invalid UTF-16 text range"),
            Self::Timeout => f.write_str("text query timed out"),
            Self::Host(message) => write!(f, "text Host: {message}"),
        }
    }
}
impl std::error::Error for TextQueryError {}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum TextQuery {
    SelectedText,
    BoundingRects(TextRange),
}

#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub enum TextQueryOutput {
    Text(String),
    Rects(Vec<LayoutRect>),
}

type QueryResult = Result<TextQueryOutput, TextQueryError>;

#[doc(hidden)]
pub struct TextQueryFuture {
    receiver: oneshot::Receiver<QueryResult>,
    registration: Option<Scoped<Registration>>,
}

impl TextQueryFuture {
    pub(crate) fn failed(error: TextQueryError) -> Self {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Err(error));
        Self {
            receiver,
            registration: None,
        }
    }
}

impl Future for TextQueryFuture {
    type Output = QueryResult;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = Pin::new(&mut self.receiver).poll(cx);
        match result {
            Poll::Ready(result) => {
                self.registration.take();
                Poll::Ready(result.unwrap_or(Err(TextQueryError::Cancelled)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

struct Registration {
    id: i64,
    queries: Weak<RefCell<Queries>>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(queries) = self.queries.upgrade() {
            queries.borrow_mut().pending.remove(&self.id);
        }
    }
}

struct Pending {
    node: NodeId,
    snapshot: Arc<SceneNode>,
    query: TextQuery,
    sender: oneshot::Sender<QueryResult>,
    started_at: Option<f64>,
}

#[derive(Default)]
pub(crate) struct Queries {
    next: i64,
    pending: HashMap<i64, Pending>,
}

impl Queries {
    pub(crate) fn register(
        queries: &Rc<RefCell<Self>>,
        node: NodeId,
        snapshot: Arc<SceneNode>,
        query: TextQuery,
    ) -> Result<(i64, TextQueryFuture), TextQueryError> {
        let (sender, receiver) = oneshot::channel();
        let id = {
            let mut state = queries.borrow_mut();
            state.next = state
                .next
                .checked_add(1)
                .ok_or_else(|| TextQueryError::Host("text query IDs exhausted".into()))?;
            let id = state.next;
            state.pending.insert(
                id,
                Pending {
                    node,
                    snapshot,
                    query,
                    sender,
                    started_at: None,
                },
            );
            id
        };
        let registration = Scoped::new(Registration {
            id,
            queries: Rc::downgrade(queries),
        });
        Ok((
            id,
            TextQueryFuture {
                receiver,
                registration: Some(registration),
            },
        ))
    }

    pub(crate) fn cancel_node(&mut self, node: NodeId) {
        let removed: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(id, pending)| (pending.node == node).then_some(*id))
            .collect();
        for id in removed {
            if let Some(pending) = self.pending.remove(&id) {
                let _ = pending.sender.send(Err(TextQueryError::NotBound));
            }
        }
    }

    pub(crate) fn step(&mut self, timestamp: f64) -> bool {
        if !timestamp.is_finite() {
            return !self.pending.is_empty();
        }
        let expired: Vec<_> = self
            .pending
            .iter_mut()
            .filter_map(|(id, pending)| {
                let start = pending.started_at.get_or_insert(timestamp);
                (timestamp - *start >= 5000.0).then_some(*id)
            })
            .collect();
        for id in expired {
            if let Some(pending) = self.pending.remove(&id) {
                let _ = pending.sender.send(Err(TextQueryError::Timeout));
            }
        }
        !self.pending.is_empty()
    }

    pub(crate) fn complete(
        &mut self,
        node: NodeId,
        current: Option<&SceneNode>,
        detail: &WhiskerValue,
    ) -> bool {
        let Some(id) = field(detail, "id").and_then(integer) else {
            return false;
        };
        if self
            .pending
            .get(&id)
            .is_none_or(|pending| pending.node != node)
        {
            return false;
        }
        let pending = self.pending.remove(&id).expect("matched query");
        let result = match current {
            None => Err(TextQueryError::NotBound),
            Some(current)
                if current.layout() != pending.snapshot.layout()
                    || current.text().map(|text| &text.payload)
                        != pending.snapshot.text().map(|text| &text.payload) =>
            {
                Err(TextQueryError::StaleLayout)
            }
            Some(_) => decode_result(pending.query, detail),
        };
        let _ = pending.sender.send(result);
        true
    }
}

fn decode_result(query: TextQuery, detail: &WhiskerValue) -> QueryResult {
    if let Some(error) = field(detail, "error").and_then(string) {
        return Err(match error {
            "stale-layout" => TextQueryError::StaleLayout,
            "invalid-range" => TextQueryError::InvalidRange,
            _ => TextQueryError::Host(error.to_owned()),
        });
    }
    let malformed = || TextQueryError::Host("malformed text query response".into());
    match query {
        TextQuery::SelectedText => field(detail, "text")
            .and_then(string)
            .map(|text| TextQueryOutput::Text(text.to_owned()))
            .ok_or_else(malformed),
        TextQuery::BoundingRects(_) => {
            let Some(WhiskerValue::Array(rects)) = field(detail, "rects") else {
                return Err(malformed());
            };
            rects
                .iter()
                .map(|rect| {
                    let number = |key| {
                        field(rect, key)
                            .and_then(number)
                            .filter(|value| value.is_finite())
                            .map(|value| value as f32)
                            .filter(|value| value.is_finite())
                            .ok_or_else(malformed)
                    };
                    let rect = LayoutRect {
                        x: number("x")?,
                        y: number("y")?,
                        width: number("width")?,
                        height: number("height")?,
                    };
                    if rect.width < 0.0 || rect.height < 0.0 {
                        return Err(malformed());
                    }
                    Ok(rect)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(TextQueryOutput::Rects)
        }
    }
}

fn field<'a>(value: &'a WhiskerValue, key: &str) -> Option<&'a WhiskerValue> {
    match value {
        WhiskerValue::Map(fields) => fields.get(key),
        _ => None,
    }
}
fn integer(value: &WhiskerValue) -> Option<i64> {
    match value {
        WhiskerValue::Int(value) => Some(*value),
        _ => None,
    }
}
fn string(value: &WhiskerValue) -> Option<&str> {
    match value {
        WhiskerValue::String(value) => Some(value),
        _ => None,
    }
}
fn number(value: &WhiskerValue) -> Option<f64> {
    match value {
        WhiskerValue::Int(value) => Some(*value as f64),
        WhiskerValue::Float(value) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
