//! Public imperative API and private controller for Rust-owned Lists.

use std::cell::RefCell;
use std::rc::Rc;

use whisker_value::WhiskerValue;

use crate::reactive::{RwSignal, Signal, computed};

use super::handle::Element;
use super::renderer::try_invoke_element_command;

/// Main axis along which a List virtualizes and scrolls.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    #[default]
    Vertical,
    Horizontal,
}

impl ScrollAxis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }
}

impl std::fmt::Display for ScrollAxis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an imperative scroll jumps immediately or animates to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBehavior {
    Instant,
    Smooth,
}

impl ScrollBehavior {
    fn smooth(self) -> bool {
        matches!(self, Self::Smooth)
    }
}

/// Alignment of an indexed/keyed item inside the List viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAlignment {
    Start,
    Center,
    End,
    Nearest,
}

/// A logical List scroll destination.
#[derive(Debug, Clone, PartialEq)]
pub enum ListScrollTarget<K> {
    Start,
    End,
    Offset(f64),
    Index {
        index: usize,
        alignment: ScrollAlignment,
    },
    Key {
        key: K,
        alignment: ScrollAlignment,
    },
}

impl<K> ListScrollTarget<K> {
    pub fn start() -> Self {
        Self::Start
    }

    pub fn end() -> Self {
        Self::End
    }

    pub fn offset(offset: f64) -> Self {
        Self::Offset(offset)
    }

    pub fn index(index: usize, alignment: ScrollAlignment) -> Self {
        Self::Index { index, alignment }
    }

    pub fn key(key: K, alignment: ScrollAlignment) -> Self {
        Self::Key { key, alignment }
    }
}

/// Cached Rust-side List state. Reading it never crosses the Host boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct ListSnapshot<K> {
    pub offset: f64,
    pub viewport_extent: f64,
    pub content_extent: f64,
    pub first_visible_index: Option<usize>,
    pub last_visible_index: Option<usize>,
    pub first_visible_key: Option<K>,
    pub visible_keys: Vec<K>,
}

/// Failures from an imperative List operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListHandleError {
    NotBound,
    TargetNotFound,
    DispatchFailed(String),
}

impl std::fmt::Display for ListHandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotBound => f.write_str("list is not mounted"),
            Self::TargetNotFound => f.write_str("list scroll target does not exist"),
            Self::DispatchFailed(message) => write!(f, "list scroll command failed: {message}"),
        }
    }
}

impl std::error::Error for ListHandleError {}

struct ListState<K> {
    element: Option<Element>,
    keys: Vec<K>,
    starts: Vec<f32>,
    ends: Vec<f32>,
    offset: f32,
    viewport: f32,
    content_extent: f32,
}

impl<K> Default for ListState<K> {
    fn default() -> Self {
        Self {
            element: None,
            keys: Vec::new(),
            starts: Vec::new(),
            ends: Vec::new(),
            offset: 0.0,
            viewport: 0.0,
            content_extent: 0.0,
        }
    }
}

/// Binding token passed to `List(list_ref: handle.r())`.
pub struct ListRef<K: 'static> {
    state: Rc<RefCell<ListState<K>>>,
    bound: RwSignal<bool>,
}

impl<K: 'static> Clone for ListRef<K> {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
            bound: self.bound,
        }
    }
}

impl<K: 'static> ListRef<K> {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(ListState::default())),
            bound: RwSignal::new(false),
        }
    }

    pub fn bound(&self) -> Signal<bool> {
        let bound = self.bound;
        Signal::Dynamic(computed(move || bound.get()))
    }

    pub fn scroll_to(
        &self,
        target: ListScrollTarget<K>,
        behavior: ScrollBehavior,
    ) -> Result<(), ListHandleError>
    where
        K: PartialEq,
    {
        let state = self.state.borrow();
        let element = state.element.ok_or(ListHandleError::NotBound)?;
        let offset = resolve_target(&state, &target).ok_or(ListHandleError::TargetNotFound)?;
        drop(state);
        dispatch(
            element,
            "scrollTo",
            WhiskerValue::map([
                ("offset", WhiskerValue::Float(f64::from(offset))),
                ("smooth", WhiskerValue::Bool(behavior.smooth())),
            ]),
        )
    }

    pub fn scroll_by(&self, delta: f64, behavior: ScrollBehavior) -> Result<(), ListHandleError> {
        let element = self
            .state
            .borrow()
            .element
            .ok_or(ListHandleError::NotBound)?;
        dispatch(
            element,
            "scrollBy",
            WhiskerValue::map([
                ("offset", WhiskerValue::Float(delta)),
                ("smooth", WhiskerValue::Bool(behavior.smooth())),
            ]),
        )
    }

    pub fn snapshot(&self) -> Option<ListSnapshot<K>>
    where
        K: Clone,
    {
        let state = self.state.borrow();
        state.element?;
        let item_count = state.keys.len();
        let first = state
            .ends
            .partition_point(|end| *end <= state.offset)
            .min(item_count);
        let visible_end = state.offset + state.viewport.max(0.0);
        let end = state
            .starts
            .partition_point(|start| *start < visible_end)
            .max(first.saturating_add(1).min(item_count));
        let visible_keys = state.keys[first..end].to_vec();
        Some(ListSnapshot {
            offset: f64::from(state.offset),
            viewport_extent: f64::from(state.viewport),
            content_extent: f64::from(state.content_extent),
            first_visible_index: (first < item_count).then_some(first),
            last_visible_index: end.checked_sub(1).filter(|_| first < item_count),
            first_visible_key: state.keys.get(first).cloned(),
            visible_keys,
        })
    }

    pub(crate) fn bind(&self, element: Element) {
        self.state.borrow_mut().element = Some(element);
        let _ = self.bound.try_set(true);
    }

    pub(crate) fn unbind(&self) {
        self.state.borrow_mut().element = None;
        let _ = self.bound.try_set(false);
    }

    pub(crate) fn update_layout(
        &self,
        keys: &[K],
        starts: &[f32],
        ends: &[f32],
        content_extent: f32,
    ) where
        K: Clone,
    {
        let mut state = self.state.borrow_mut();
        state.keys.clone_from(&keys.to_vec());
        state.starts.clear();
        state.starts.extend_from_slice(starts);
        state.ends.clear();
        state.ends.extend_from_slice(ends);
        state.content_extent = content_extent;
    }

    pub(crate) fn update_geometry(&self, offset: f32, viewport: f32) {
        let mut state = self.state.borrow_mut();
        state.offset = offset;
        state.viewport = viewport;
    }
}

/// User-facing imperative handle for one keyed List.
#[derive(Clone)]
pub struct ListHandle<K: 'static> {
    r: ListRef<K>,
}

impl<K: 'static> ListHandle<K> {
    pub fn new() -> Self {
        Self { r: ListRef::new() }
    }

    pub fn r(&self) -> ListRef<K> {
        self.r.clone()
    }

    pub fn bound(&self) -> Signal<bool> {
        self.r.bound()
    }

    pub fn scroll_to(
        &self,
        target: ListScrollTarget<K>,
        behavior: ScrollBehavior,
    ) -> Result<(), ListHandleError>
    where
        K: PartialEq,
    {
        self.r.scroll_to(target, behavior)
    }

    pub fn scroll_by(&self, delta: f64, behavior: ScrollBehavior) -> Result<(), ListHandleError> {
        self.r.scroll_by(delta, behavior)
    }

    pub fn snapshot(&self) -> Option<ListSnapshot<K>>
    where
        K: Clone,
    {
        self.r.snapshot()
    }
}

impl<K: 'static> Default for ListHandle<K> {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_target<K: PartialEq>(state: &ListState<K>, target: &ListScrollTarget<K>) -> Option<f32> {
    let maximum = (state.content_extent - state.viewport).max(0.0);
    let offset = match target {
        ListScrollTarget::Start => 0.0,
        ListScrollTarget::End => maximum,
        ListScrollTarget::Offset(offset) => *offset as f32,
        ListScrollTarget::Index { index, alignment } => item_offset(state, *index, *alignment)?,
        ListScrollTarget::Key { key, alignment } => {
            let index = state.keys.iter().position(|candidate| candidate == key)?;
            item_offset(state, index, *alignment)?
        }
    };
    Some(offset.clamp(0.0, maximum))
}

fn item_offset<K>(state: &ListState<K>, index: usize, alignment: ScrollAlignment) -> Option<f32> {
    let start = *state.starts.get(index)?;
    let end = *state.ends.get(index)?;
    let extent = end - start;
    Some(match alignment {
        ScrollAlignment::Start => start,
        ScrollAlignment::Center => start - (state.viewport - extent) * 0.5,
        ScrollAlignment::End => end - state.viewport,
        ScrollAlignment::Nearest if start < state.offset => start,
        ScrollAlignment::Nearest if end > state.offset + state.viewport => end - state.viewport,
        ScrollAlignment::Nearest => state.offset,
    })
}

fn dispatch(
    element: Element,
    command: &str,
    arguments: WhiskerValue,
) -> Result<(), ListHandleError> {
    match try_invoke_element_command(element, command, arguments) {
        Some(Ok(())) => Ok(()),
        Some(Err(message)) => Err(ListHandleError::DispatchFailed(message)),
        None => Err(ListHandleError::DispatchFailed(format!(
            "ScrollView has no `{command}` command"
        ))),
    }
}
