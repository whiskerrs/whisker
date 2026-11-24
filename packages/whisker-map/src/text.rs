//! Signed-distance glyphs from a style's `glyphs` URL, and laying text out with them.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::tiles::log;
use crate::tiles::mvt::{DecodeError, Reader};

/// The font size glyph PBFs are rendered at.
pub(crate) const ONE_EM: f32 = 24.0;
/// The SDF margin around every glyph bitmap.
pub(crate) const BORDER: u32 = 3;
/// MapLibre's offset from a line's middle to the reference its glyph `top`s are measured from.
const BASELINE_OFFSET: f32 = -17.0;

#[derive(Debug)]
pub(crate) struct Glyph {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub advance: f32,
    /// `(width + 2 * BORDER) * (height + 2 * BORDER)` distances, 192 at the glyph's edge.
    pub bitmap: Vec<u8>,
}

pub(crate) fn parse_glyphs(data: &[u8]) -> Result<Vec<Glyph>, DecodeError> {
    let mut glyphs = Vec::new();
    let mut reader = Reader::new(data);
    while !reader.done() {
        let (field, wire) = reader.key()?;
        if field != 1 || wire != 2 {
            reader.skip(wire)?;
            continue;
        }
        let mut stack = Reader::new(reader.bytes()?);
        while !stack.done() {
            let (field, wire) = stack.key()?;
            if field != 3 || wire != 2 {
                stack.skip(wire)?;
                continue;
            }
            let mut g = Reader::new(stack.bytes()?);
            let mut glyph = Glyph {
                id: 0,
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                advance: 0.0,
                bitmap: Vec::new(),
            };
            while !g.done() {
                let (field, wire) = g.key()?;
                match (field, wire) {
                    (1, 0) => glyph.id = g.varint()? as u32,
                    (2, 2) => glyph.bitmap = g.bytes()?.to_vec(),
                    (3, 0) => glyph.width = g.varint()? as u32,
                    (4, 0) => glyph.height = g.varint()? as u32,
                    (5, 0) => glyph.left = unzigzag(g.varint()?),
                    (6, 0) => glyph.top = unzigzag(g.varint()?),
                    (7, 0) => glyph.advance = g.varint()? as f32,
                    _ => g.skip(wire)?,
                }
            }
            let expected = ((glyph.width + 2 * BORDER) * (glyph.height + 2 * BORDER)) as usize;
            if glyph.width > 0 && glyph.bitmap.len() != expected {
                glyph.bitmap.clear();
                glyph.width = 0;
                glyph.height = 0;
            }
            glyphs.push(glyph);
        }
    }
    Ok(glyphs)
}

fn unzigzag(v: u64) -> i32 {
    ((v >> 1) as i64 ^ -((v & 1) as i64)) as i32
}

enum RangeState {
    Loading,
    Done,
}

/// The glyphs fetched so far for every font stack the style uses.
type ByFont<T> = RefCell<HashMap<(Arc<str>, u32), T>>;

pub(crate) struct GlyphStore {
    template: String,
    ranges: ByFont<RangeState>,
    glyphs: ByFont<Arc<Glyph>>,
    in_flight: Cell<usize>,
    on_change: Rc<dyn Fn()>,
}

const MAX_IN_FLIGHT: usize = 8;

pub(crate) enum Lookup {
    Ready(Arc<Glyph>),
    /// The font has no such glyph.
    Missing,
    Pending,
}

impl GlyphStore {
    pub fn new(template: String, on_change: Rc<dyn Fn()>) -> Rc<GlyphStore> {
        Rc::new(GlyphStore {
            template,
            ranges: RefCell::new(HashMap::new()),
            glyphs: RefCell::new(HashMap::new()),
            in_flight: Cell::new(0),
            on_change,
        })
    }

    pub fn url(&self) -> &str {
        &self.template
    }

    pub fn is_loading(&self) -> bool {
        self.in_flight.get() > 0
    }

    pub fn glyph(self: &Rc<Self>, font: &Arc<str>, codepoint: u32) -> Lookup {
        if let Some(glyph) = self.glyphs.borrow().get(&(font.clone(), codepoint)) {
            return Lookup::Ready(glyph.clone());
        }
        let range = codepoint / 256;
        match self.ranges.borrow().get(&(font.clone(), range)) {
            Some(RangeState::Done) => return Lookup::Missing,
            Some(RangeState::Loading) => return Lookup::Pending,
            None => {}
        }
        self.request(font, range);
        Lookup::Pending
    }

    fn request(self: &Rc<Self>, font: &Arc<str>, range: u32) {
        if self.in_flight.get() >= MAX_IN_FLIGHT {
            return;
        }
        self.ranges
            .borrow_mut()
            .insert((font.clone(), range), RangeState::Loading);
        self.in_flight.set(self.in_flight.get() + 1);
        let url = self
            .template
            .replace("{fontstack}", &encode(font))
            .replace("{range}", &format!("{}-{}", range * 256, range * 256 + 255));
        let weak = Rc::downgrade(self);
        let font = font.clone();
        whisker::spawn_local(async move {
            let fetched = crate::net::fetch(url.clone(), None).await;
            let Some(store) = weak.upgrade() else { return };
            store.in_flight.set(store.in_flight.get().saturating_sub(1));
            let glyphs = match fetched {
                Ok(Some(bytes)) => match parse_glyphs(&bytes) {
                    Ok(glyphs) => glyphs,
                    Err(error) => {
                        log(&format!("whisker-map: glyphs {url}: {error}"));
                        Vec::new()
                    }
                },
                Ok(None) => Vec::new(),
                Err(error) => {
                    log(&format!("whisker-map: glyphs {url}: {error}"));
                    Vec::new()
                }
            };
            {
                let mut map = store.glyphs.borrow_mut();
                for glyph in glyphs {
                    map.insert((font.clone(), glyph.id), Arc::new(glyph));
                }
            }
            store
                .ranges
                .borrow_mut()
                .insert((font, range), RangeState::Done);
            (store.on_change)();
        });
    }
}

fn encode(font: &str) -> String {
    let mut out = String::new();
    for byte in font.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Anchor {
    /// 0 at the left edge of the box, 1 at the right.
    pub horizontal: f32,
    /// 0 at the top edge, 1 at the bottom.
    pub vertical: f32,
}

impl Anchor {
    pub fn parse(name: &str) -> Anchor {
        let (horizontal, vertical) = match name {
            "left" => (0.0, 0.5),
            "right" => (1.0, 0.5),
            "top" => (0.5, 0.0),
            "bottom" => (0.5, 1.0),
            "top-left" => (0.0, 0.0),
            "top-right" => (1.0, 0.0),
            "bottom-left" => (0.0, 1.0),
            "bottom-right" => (1.0, 1.0),
            _ => (0.5, 0.5),
        };
        Anchor {
            horizontal,
            vertical,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedGlyph {
    pub font: Arc<str>,
    pub glyph: Arc<Glyph>,
    /// Pen position of the glyph's left edge, in pixels at `ONE_EM`, relative to the anchor.
    pub x: f32,
    /// The line's middle plus `BASELINE_OFFSET`.
    pub y: f32,
}

/// Text laid out around its anchor, in pixels at `ONE_EM`.
#[derive(Clone, Debug)]
pub(crate) struct Shaping {
    pub glyphs: Vec<ShapedGlyph>,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Shaping {
    /// The glyph's quad corners relative to the anchor, before scaling to the text size.
    pub fn quad(glyph: &ShapedGlyph) -> [f32; 4] {
        let g = &glyph.glyph;
        let border = BORDER as f32;
        let x0 = glyph.x + g.left as f32 - border;
        let y0 = glyph.y - g.top as f32 - border;
        [
            x0,
            y0,
            x0 + (g.width + 2 * BORDER) as f32,
            y0 + (g.height + 2 * BORDER) as f32,
        ]
    }
}

pub(crate) struct Layout {
    pub max_width: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub justify: f32,
    pub anchor: Anchor,
    pub offset: [f32; 2],
    /// Whether to break lines at all; line labels stay on one line.
    pub wrap: bool,
}

/// Lays out `text`, or `None` while some of its glyphs are still loading.
pub(crate) fn shape(
    text: &str,
    font: &Arc<str>,
    store: &Rc<GlyphStore>,
    layout: &Layout,
) -> Option<Shaping> {
    let mut chars: Vec<(char, Option<Arc<Glyph>>)> = Vec::new();
    let mut pending = false;
    for c in text.chars() {
        if c == '\n' {
            chars.push((c, None));
            continue;
        }
        match store.glyph(font, c as u32) {
            Lookup::Ready(glyph) => chars.push((c, Some(glyph))),
            Lookup::Missing => {}
            Lookup::Pending => pending = true,
        }
    }
    if pending {
        return None;
    }
    let advance = |g: &Option<Arc<Glyph>>| {
        g.as_ref()
            .map_or(0.0, |g| g.advance + layout.letter_spacing * ONE_EM)
    };
    let max_width = layout.max_width * ONE_EM;
    let mut lines: Vec<Vec<(char, Arc<Glyph>)>> = Vec::new();
    let mut paragraph_start = 0;
    let mut i = 0;
    while i <= chars.len() {
        if i == chars.len() || chars[i].0 == '\n' {
            let paragraph = &chars[paragraph_start..i];
            for range in break_lines(paragraph, max_width, layout.wrap, &advance) {
                let line: Vec<(char, Arc<Glyph>)> = paragraph[range]
                    .iter()
                    .filter_map(|(c, g)| g.clone().map(|g| (*c, g)))
                    .collect();
                lines.push(trim(line));
            }
            paragraph_start = i + 1;
        }
        i += 1;
    }
    lines.retain(|line| !line.is_empty());
    if lines.is_empty() {
        return None;
    }
    let line_height = layout.line_height * ONE_EM;
    let widths: Vec<f32> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|(_, g)| g.advance + layout.letter_spacing * ONE_EM)
                .sum::<f32>()
                - layout.letter_spacing * ONE_EM
        })
        .collect();
    let block_width = widths.iter().copied().fold(0.0, f32::max);
    let block_height = lines.len() as f32 * line_height;
    let left = -layout.anchor.horizontal * block_width + layout.offset[0] * ONE_EM;
    let top = -layout.anchor.vertical * block_height + layout.offset[1] * ONE_EM;
    let mut glyphs = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let mut x = left + (block_width - widths[index]) * layout.justify;
        let y = top + (index as f32 + 0.5) * line_height + BASELINE_OFFSET;
        for (_, glyph) in line {
            glyphs.push(ShapedGlyph {
                font: font.clone(),
                glyph: glyph.clone(),
                x,
                y,
            });
            x += glyph.advance + layout.letter_spacing * ONE_EM;
        }
    }
    Some(Shaping {
        glyphs,
        left,
        top,
        right: left + block_width,
        bottom: top + block_height,
    })
}

fn trim(mut line: Vec<(char, Arc<Glyph>)>) -> Vec<(char, Arc<Glyph>)> {
    while line.last().is_some_and(|(c, _)| c.is_whitespace()) {
        line.pop();
    }
    let start = line
        .iter()
        .position(|(c, _)| !c.is_whitespace())
        .unwrap_or(line.len());
    line.drain(..start);
    line
}

/// Whether text may break between this character and the next without a space, as CJK allows.
pub(crate) fn breaks_anywhere(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FFF   // CJK radicals, Kangxi
        | 0x3000..=0x303F // CJK punctuation
        | 0x3040..=0x30FF // Hiragana, Katakana
        | 0x3100..=0x31FF
        | 0x3200..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF // Hangul
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF)
}

/// Greedy line breaks that keep each line within `max_width` where a break allows it.
fn break_lines(
    chars: &[(char, Option<Arc<Glyph>>)],
    max_width: f32,
    wrap: bool,
    advance: &dyn Fn(&Option<Arc<Glyph>>) -> f32,
) -> Vec<std::ops::Range<usize>> {
    if !wrap || chars.is_empty() {
        return std::iter::once(0..chars.len()).collect();
    }
    let total: f32 = chars.iter().map(|(_, g)| advance(g)).sum();
    // MapLibre balances lines; aiming at an even split avoids a lone short last line.
    let lines = (total / max_width).ceil().max(1.0);
    let target = total / lines;
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut width = 0.0;
    let mut last_break: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        let (c, g) = &chars[i];
        width += advance(g);
        if width > target.max(max_width.min(target * 1.2))
            && let Some(at) = last_break.filter(|&at| at > start)
        {
            ranges.push(start..at);
            start = at;
            width = chars[start..=i].iter().map(|(_, g)| advance(g)).sum();
            last_break = None;
        }
        if c.is_whitespace()
            || (breaks_anywhere(*c) && i + 1 < chars.len() && !is_closing(chars[i + 1].0))
        {
            last_break = Some(i + 1);
        }
        i += 1;
    }
    ranges.push(start..chars.len());
    ranges
}

fn is_closing(c: char) -> bool {
    matches!(
        c,
        '、' | '。' | '）' | '」' | '』' | '・' | 'ー' | ')' | ',' | '.'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiles::mvt::tests::Writer;

    fn glyph_pbf(glyphs: &[(u32, u32, u32, i32, i32, u32)]) -> Vec<u8> {
        let mut stack = Writer::default();
        stack.bytes(1, b"Test Regular");
        stack.bytes(2, b"0-255");
        for &(id, w, h, left, top, advance) in glyphs {
            let mut g = Writer::default();
            g.key(1, 0);
            g.varint(u64::from(id));
            let bitmap = vec![0u8; ((w + 6) * (h + 6)) as usize];
            g.bytes(2, &bitmap);
            g.key(3, 0);
            g.varint(u64::from(w));
            g.key(4, 0);
            g.varint(u64::from(h));
            g.key(5, 0);
            g.varint(u64::from(crate::tiles::mvt::tests::zz(left)));
            g.key(6, 0);
            g.varint(u64::from(crate::tiles::mvt::tests::zz(top)));
            g.key(7, 0);
            g.varint(u64::from(advance));
            stack.bytes(3, &g.0);
        }
        let mut root = Writer::default();
        root.bytes(1, &stack.0);
        root.0
    }

    #[test]
    fn parses_glyph_pbfs() {
        let data = glyph_pbf(&[(65, 14, 17, 1, -7, 16), (32, 0, 0, 0, -26, 6)]);
        let glyphs = parse_glyphs(&data).unwrap();
        assert_eq!(glyphs.len(), 2);
        assert_eq!(glyphs[0].id, 65);
        assert_eq!(glyphs[0].top, -7);
        assert_eq!(glyphs[0].bitmap.len(), 20 * 23);
        assert_eq!(glyphs[1].advance, 6.0);
    }

    #[test]
    fn encodes_font_stacks() {
        assert_eq!(encode("Noto Sans Regular"), "Noto%20Sans%20Regular");
    }

    fn glyphs(text: &str, advance: f32) -> Vec<(char, Option<Arc<Glyph>>)> {
        text.chars()
            .map(|c| {
                (
                    c,
                    Some(Arc::new(Glyph {
                        id: c as u32,
                        width: 10,
                        height: 10,
                        left: 0,
                        top: 0,
                        advance,
                        bitmap: Vec::new(),
                    })),
                )
            })
            .collect()
    }

    #[test]
    fn breaks_at_spaces_and_between_ideographs() {
        let advance = |g: &Option<Arc<Glyph>>| g.as_ref().map_or(0.0, |g| g.advance);
        let latin = glyphs("aaaa bbbb cccc", 10.0);
        let ranges = break_lines(&latin, 50.0, true, &advance);
        assert_eq!(ranges.len(), 3, "{ranges:?}");
        let cjk = glyphs("東京都千代田区丸の内", 24.0);
        let ranges = break_lines(&cjk, 120.0, true, &advance);
        assert_eq!(ranges.len(), 2, "{ranges:?}");
        assert!(ranges.iter().all(|r| r.len() == 5), "{ranges:?}");
    }
}
