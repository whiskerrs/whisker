use wasm_bindgen::{JsCast, JsValue};
use whisker_protocol::WhiskerValue;

struct Leaf {
    node: web_sys::Node,
    start: u32,
    end: u32,
    attachment: bool,
}

pub(super) fn source(content: &web_sys::Element) -> String {
    content
        .get_attribute("data-whisker-text-source")
        .or_else(|| content.text_content())
        .unwrap_or_default()
}

fn leaves(content: &web_sys::Element) -> Result<Vec<Leaf>, JsValue> {
    let source = source(content);
    let spans = content.query_selector_all("[data-start]")?;
    if spans.length() == 0 {
        return Ok(content
            .first_child()
            .map(|node| Leaf {
                node,
                start: 0,
                end: source.encode_utf16().count() as u32,
                attachment: false,
            })
            .into_iter()
            .collect());
    }
    let mut leaves = Vec::new();
    for index in 0..spans.length() {
        let Some(reference) = spans
            .item(index)
            .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
        else {
            continue;
        };
        let Some(span) = reference.first_element_child() else {
            continue;
        };
        if reference.get_client_rects().length() == 0 {
            continue;
        }
        let start_byte = reference
            .get_attribute("data-start")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| JsValue::from_str("invalid source offset"))?;
        let start = source
            .get(..start_byte)
            .ok_or_else(|| JsValue::from_str("invalid source boundary"))?
            .encode_utf16()
            .count() as u32;
        let attachment = span.has_attribute("data-whisker-inline-node");
        if attachment {
            leaves.push(Leaf {
                node: span.into(),
                start,
                end: start + 1,
                attachment,
            });
        } else if let Some(node) = span.first_child() {
            let count = node
                .text_content()
                .unwrap_or_default()
                .encode_utf16()
                .count() as u32;
            leaves.push(Leaf {
                node,
                start,
                end: start + count,
                attachment,
            });
        }
    }
    Ok(leaves)
}

fn browser_selection(content: &web_sys::Element) -> Result<Option<web_sys::Selection>, JsValue> {
    content
        .owner_document()
        .ok_or_else(|| JsValue::from_str("missing document"))?
        .get_selection()
}

pub(super) fn selection(
    content: &web_sys::Element,
) -> Result<Option<(u32, u32, &'static str)>, JsValue> {
    let Some(selection) = browser_selection(content)? else {
        return Ok(None);
    };
    let (Some(anchor), Some(focus)) = (selection.anchor_node(), selection.focus_node()) else {
        return Ok(None);
    };
    if !content.contains(Some(&anchor)) || !content.contains(Some(&focus)) {
        return Ok(None);
    }
    let leaves = leaves(content)?;
    let (Some(anchor), Some(focus)) = (
        source_offset(&leaves, &anchor, selection.anchor_offset()),
        source_offset(&leaves, &focus, selection.focus_offset()),
    ) else {
        return Ok(None);
    };
    Ok(Some((
        anchor.min(focus),
        anchor.max(focus),
        if anchor <= focus {
            "forward"
        } else {
            "backward"
        },
    )))
}

fn source_offset(leaves: &[Leaf], node: &web_sys::Node, offset: u32) -> Option<u32> {
    if let Some(leaf) = leaves
        .iter()
        .find(|leaf| leaf.node.is_same_node(Some(node)))
    {
        return Some(leaf.start + offset.min(leaf.end - leaf.start));
    }
    let children = node.child_nodes();
    for index in offset..children.length() {
        let child = children.item(index)?;
        if let Some(leaf) = leaves.iter().find(|leaf| child.contains(Some(&leaf.node))) {
            return Some(leaf.start);
        }
    }
    leaves
        .iter()
        .rev()
        .find(|leaf| node.contains(Some(&leaf.node)))
        .map(|leaf| leaf.end)
}

pub(super) fn clear(content: &web_sys::Element) -> Result<(), JsValue> {
    if selection(content)?.is_some() {
        if let Some(selection) = browser_selection(content)? {
            selection.remove_all_ranges()?;
        }
    }
    Ok(())
}

pub(super) fn select(content: &web_sys::Element, start: u32, end: u32) -> Result<(), JsValue> {
    select_direction(content, start, end, false)
}

pub(super) fn select_direction(
    content: &web_sys::Element,
    start: u32,
    end: u32,
    backward: bool,
) -> Result<(), JsValue> {
    use unicode_segmentation::UnicodeSegmentation;
    let source = source(content);
    let bytes = whisker_protocol::TextRange { start, end }
        .to_utf8(&source)
        .ok_or_else(|| JsValue::from_str("invalid source range"))?;
    let (start, end) = if start == end {
        (start, end)
    } else {
        let mut first = bytes.start;
        let mut last = bytes.end;
        for (offset, grapheme) in source.grapheme_indices(true) {
            if offset <= bytes.start && bytes.start < offset + grapheme.len() {
                first = offset;
            }
            if offset < bytes.end && bytes.end <= offset + grapheme.len() {
                last = offset + grapheme.len();
            }
        }
        let range = whisker_protocol::TextRange::from_utf8(&source, first..last)
            .ok_or_else(|| JsValue::from_str("invalid grapheme range"))?;
        (range.start, range.end)
    };
    let leaves = leaves(content)?;
    let Some((first, first_offset)) = endpoint(&leaves, start, false) else {
        return clear(content);
    };
    let Some((last, last_offset)) = endpoint(&leaves, end, true) else {
        return clear(content);
    };
    let document = content
        .owner_document()
        .ok_or_else(|| JsValue::from_str("missing document"))?;
    let range = document.create_range()?;
    range.set_start(&first, first_offset)?;
    range.set_end(&last, last_offset)?;
    if let Some(selection) = document.get_selection()? {
        selection.remove_all_ranges()?;
        if backward {
            selection.set_base_and_extent(&last, last_offset, &first, first_offset)?;
        } else {
            selection.add_range(&range)?;
        }
    }
    Ok(())
}

fn endpoint(leaves: &[Leaf], offset: u32, end: bool) -> Option<(web_sys::Node, u32)> {
    let leaf = leaves
        .iter()
        .find(|leaf| offset < leaf.end || end && offset == leaf.end)
        .or_else(|| leaves.last())?;
    if leaf.attachment {
        let parent = leaf.node.parent_node()?;
        let children = parent.child_nodes();
        let index = (0..children.length()).find(|index| {
            children
                .item(*index)
                .is_some_and(|child| child.is_same_node(Some(&leaf.node)))
        })?;
        Some((parent, index + u32::from(offset > leaf.start)))
    } else {
        Some((
            leaf.node.clone(),
            offset.saturating_sub(leaf.start).min(leaf.end - leaf.start),
        ))
    }
}

pub(super) fn bounding_rects(
    content: &web_sys::Element,
    start: u32,
    end: u32,
) -> Result<Vec<WhiskerValue>, JsValue> {
    whisker_protocol::TextRange { start, end }
        .to_utf8(&source(content))
        .ok_or_else(|| JsValue::from_str("invalid source range"))?;
    let root = content.get_bounding_client_rect();
    let document = content
        .owner_document()
        .ok_or_else(|| JsValue::from_str("missing document"))?;
    let range = document.create_range()?;
    let mut result = Vec::new();
    for leaf in leaves(content)? {
        let from = start.max(leaf.start);
        let to = end.min(leaf.end);
        if from > to || from == to && start != end {
            continue;
        }
        if leaf.attachment {
            range.select_node(&leaf.node)?;
        } else {
            range.set_start(&leaf.node, from - leaf.start)?;
            range.set_end(&leaf.node, to - leaf.start)?;
        }
        let Some(rects) = range.get_client_rects() else {
            continue;
        };
        for index in 0..rects.length() {
            let Some(rect) = rects.item(index) else {
                continue;
            };
            let x = rect.left().max(root.left());
            let y = rect.top().max(root.top());
            let width = rect.right().min(root.right()) - x;
            let height = rect.bottom().min(root.bottom()) - y;
            if width < 0.0 || height <= 0.0 {
                continue;
            }
            result.push(WhiskerValue::map([
                ("x", WhiskerValue::Float(x - root.left())),
                ("y", WhiskerValue::Float(y - root.top())),
                ("width", WhiskerValue::Float(width)),
                ("height", WhiskerValue::Float(height)),
            ]));
        }
    }
    Ok(result)
}

pub(super) fn selected_text(content: &web_sys::Element) -> Result<String, JsValue> {
    let Some((start, end, _)) = selection(content)? else {
        return Ok(String::new());
    };
    let source = source(content);
    let bytes = whisker_protocol::TextRange { start, end }
        .to_utf8(&source)
        .ok_or_else(|| JsValue::from_str("invalid range"))?;
    let mut cursor = bytes.start;
    let mut result = String::new();
    for leaf in leaves(content)?
        .into_iter()
        .filter(|leaf| leaf.attachment && leaf.start >= start && leaf.end <= end)
    {
        let range = whisker_protocol::TextRange {
            start: leaf.start,
            end: leaf.end,
        }
        .to_utf8(&source)
        .ok_or_else(|| JsValue::from_str("invalid attachment range"))?;
        result.push_str(&source[cursor..range.start]);
        if let Some(label) = leaf
            .node
            .dyn_ref::<web_sys::Element>()
            .and_then(|element| element.get_attribute("data-whisker-copy-label"))
        {
            result.push_str(&label);
        }
        cursor = range.end;
    }
    result.push_str(&source[cursor..bytes.end]);
    Ok(result)
}
