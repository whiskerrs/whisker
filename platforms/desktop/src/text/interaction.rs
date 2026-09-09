use std::collections::BTreeMap;

use unicode_segmentation::UnicodeSegmentation;
use whisker_protocol::{TextContent, TextRange, WhiskerValue};

use super::NativeTextHost;
use crate::element::{DesktopEventEmitter, DesktopNativeEvent};

#[derive(Debug)]
pub(crate) struct TextState {
    pub(crate) content: Option<TextContent>,
    pub(crate) events: DesktopEventEmitter,
    pub(crate) selectable: bool,
    pub(crate) focused: bool,
    pub(crate) dragging: bool,
    anchor: u32,
    focus: u32,
    selection: bool,
    queries: Vec<BTreeMap<String, WhiskerValue>>,
}

impl TextState {
    pub(crate) fn new(events: DesktopEventEmitter) -> Self {
        Self {
            content: None,
            events,
            selectable: false,
            focused: false,
            dragging: false,
            anchor: 0,
            focus: 0,
            selection: false,
            queries: Vec::new(),
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new(DesktopEventEmitter::default());
    }

    pub(crate) fn set_content(&mut self, content: TextContent) {
        let changed = self
            .content
            .as_ref()
            .is_some_and(|old| old.payload.text != content.payload.text);
        self.content = Some(content);
        if changed {
            self.set_selection(None);
            self.dragging = false;
        }
    }

    pub(crate) fn set_selectable(&mut self, selectable: bool) {
        self.selectable = selectable;
        if !selectable {
            self.focused = false;
            self.dragging = false;
            self.set_selection(None);
        }
    }

    pub(crate) fn selection(&self) -> Option<TextRange> {
        self.selection.then_some(TextRange {
            start: self.anchor.min(self.focus),
            end: self.anchor.max(self.focus),
        })
    }

    pub(crate) fn select_to(&mut self, position: u32, extend: bool) {
        let anchor = if extend && self.selection {
            self.anchor
        } else {
            position
        };
        self.set_selection(Some((anchor, position)));
    }

    pub(crate) fn select_all(&mut self) {
        if let Some(content) = &self.content {
            self.set_selection(Some((
                0,
                content.payload.text.encode_utf16().count() as u32,
            )));
        }
    }

    pub(crate) fn set_selection(&mut self, range: Option<(u32, u32)>) {
        let old = self.selection.then_some((self.anchor, self.focus));
        if old == range {
            return;
        }
        self.selection = range.is_some();
        (self.anchor, self.focus) = range.unwrap_or_default();
        let range = self.selection();
        self.events.emit(DesktopNativeEvent {
            event: "selectionchange".into(),
            detail: WhiskerValue::Map(BTreeMap::from([
                (
                    "revision".into(),
                    WhiskerValue::Int(
                        self.content
                            .as_ref()
                            .and_then(|content| content.prepared_content)
                            .map_or(0, |id| id.get() as i64),
                    ),
                ),
                (
                    "start".into(),
                    WhiskerValue::Int(range.map_or(-1, |r| r.start as i64)),
                ),
                (
                    "end".into(),
                    WhiskerValue::Int(range.map_or(-1, |r| r.end as i64)),
                ),
                (
                    "direction".into(),
                    WhiskerValue::String(
                        if self.focus < self.anchor {
                            "backward"
                        } else {
                            "forward"
                        }
                        .into(),
                    ),
                ),
            ])),
        });
    }

    pub(crate) fn selected_text(&self) -> String {
        let Some(content) = &self.content else {
            return String::new();
        };
        self.selection()
            .and_then(|range| content.payload.copy_text(range))
            .unwrap_or_default()
    }

    fn current(&self, args: &BTreeMap<String, WhiskerValue>) -> bool {
        self.content
            .as_ref()
            .and_then(|text| text.prepared_content)
            .is_some_and(|id| integer(args, "revision") == i64::try_from(id.get()).ok())
    }

    pub(crate) fn command(&mut self, command: u32, arguments: &WhiskerValue) {
        let WhiskerValue::Map(args) = arguments else {
            return;
        };
        if command == 3 {
            self.queries.push(args.clone());
            return;
        }
        if !self.current(args) {
            return;
        }
        match command {
            1 if self.selectable => {
                let Some(content) = &self.content else {
                    return;
                };
                let Some(range) =
                    range(args).and_then(|r| grapheme_range(&content.payload.text, r))
                else {
                    return;
                };
                self.set_selection(Some((range.start, range.end)));
            }
            2 => self.set_selection(None),
            _ => {}
        }
    }

    pub(crate) fn resolve_queries(&mut self, host: &NativeTextHost) {
        for query in std::mem::take(&mut self.queries) {
            let Some(id) = integer(&query, "id") else {
                continue;
            };
            let mut reply = BTreeMap::from([("id".into(), WhiskerValue::Int(id))]);
            let result = self.query(&query, host);
            match result {
                Ok((name, value)) => {
                    reply.insert(name.into(), value);
                }
                Err(error) => {
                    reply.insert("error".into(), WhiskerValue::String(error.into()));
                }
            }
            self.events.emit(DesktopNativeEvent {
                event: "textqueryresult".into(),
                detail: WhiskerValue::Map(reply),
            });
        }
    }

    fn query(
        &self,
        args: &BTreeMap<String, WhiskerValue>,
        host: &NativeTextHost,
    ) -> Result<(&'static str, WhiskerValue), &'static str> {
        if !self.current(args) {
            return Err("stale-layout");
        }
        match args.get("kind") {
            Some(WhiskerValue::String(kind)) if kind == "selectedText" => {
                Ok(("text", WhiskerValue::String(self.selected_text())))
            }
            Some(WhiskerValue::String(kind)) if kind == "boundingRects" => {
                let content = self.content.as_ref().ok_or("stale-layout")?;
                let range = range(args)
                    .and_then(|r| grapheme_range(&content.payload.text, r))
                    .ok_or("invalid-range")?;
                let prepared = content
                    .prepared_content
                    .and_then(|id| host.prepared.get(&id))
                    .and_then(|p| p.paragraph.as_ref())
                    .ok_or("stale-layout")?;
                let rects = prepared
                    .selection_rects(&content.payload, range)
                    .ok_or("invalid-range")?;
                Ok((
                    "rects",
                    WhiskerValue::Array(
                        rects
                            .into_iter()
                            .map(|rect| {
                                WhiskerValue::Map(BTreeMap::from([
                                    ("x".into(), WhiskerValue::Float(rect.x as f64)),
                                    ("y".into(), WhiskerValue::Float(rect.y as f64)),
                                    ("width".into(), WhiskerValue::Float(rect.width as f64)),
                                    ("height".into(), WhiskerValue::Float(rect.height as f64)),
                                ]))
                            })
                            .collect(),
                    ),
                ))
            }
            _ => Err("invalid-query"),
        }
    }
}

fn integer(args: &BTreeMap<String, WhiskerValue>, name: &str) -> Option<i64> {
    if let Some(WhiskerValue::Int(value)) = args.get(name) {
        Some(*value)
    } else {
        None
    }
}

fn range(args: &BTreeMap<String, WhiskerValue>) -> Option<TextRange> {
    Some(TextRange {
        start: integer(args, "start")?.try_into().ok()?,
        end: integer(args, "end")?.try_into().ok()?,
    })
}

fn grapheme_range(text: &str, range: TextRange) -> Option<TextRange> {
    let bytes = range.to_utf8(text)?;
    let mut start = bytes.start;
    let mut end = bytes.end;
    for (index, grapheme) in text.grapheme_indices(true) {
        if index < start && index + grapheme.len() > start {
            start = index;
        }
        if index < end && index + grapheme.len() > end {
            end = index + grapheme.len();
        }
    }
    if bytes.is_empty() {
        end = start;
    }
    TextRange::from_utf8(text, start..end)
}
