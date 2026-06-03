//! Decoder — walks a byte stream conforming to
//! `packages/whisker-svg/SPEC.md` and dispatches each opcode to a
//! [`Visitor`].
//!
//! The same `replay` function powers Rust-side unit tests
//! (`TraceVisitor` records every call and assertions compare the
//! trace to a golden file) and will, in the future, power any
//! Rust-side rasteriser. The per-platform replayers in
//! `packages/whisker-svg/{ios,android}/` are independent
//! reimplementations of this logic against the same SPEC; the
//! cross-platform tests in `tests/` verify that all three agree
//! by feeding them identical fixture bytes.

use crate::builder::{Color, Transform};
use crate::format::*;

/// Error surface for a malformed or unsupported display-list
/// stream. The replayer stops at the first error and returns
/// without further dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    /// Header bytes didn't start with `"WSDL"`.
    BadMagic,
    /// Header version byte is greater than [`VERSION`].
    UnsupportedVersion(u8),
    /// `flags` byte was non-zero in a v1 stream.
    UnsupportedFlags(u8),
    /// Stream was shorter than the 6-byte header.
    HeaderTooShort,
    /// Stream ended before [`OP_END`] was reached.
    Truncated,
    /// Encountered an opcode that's reserved for future use.
    ///
    /// SPEC §"Opcode space" — a v1 replayer MUST stop on these
    /// rather than silently render a partial frame.
    UnsupportedOpcode(u8),
    /// Encountered a byte that doesn't match any defined opcode
    /// AND isn't in a reserved range (= a real protocol violation,
    /// not a known-unknown).
    UnknownOpcode(u8),
}

/// Sink for decoded opcodes. Every method has a `_default = no-op`
/// behaviour so tests / partial implementations only need to
/// override what they care about.
#[allow(unused_variables)]
pub trait Visitor {
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn concat(&mut self, transform: &Transform) {}
    fn viewport(&mut self, min_x: f32, min_y: f32, width: f32, height: f32) {}
    fn fill_color(&mut self, color: Color) {}
    fn stroke_color(&mut self, color: Color) {}
    fn stroke_width(&mut self, width: f32) {}
    fn opacity(&mut self, alpha: f32) {}
    fn fill_tint(&mut self) {}
    fn stroke_tint(&mut self) {}
    fn path_begin(&mut self) {}
    fn move_to(&mut self, x: f32, y: f32) {}
    fn line_to(&mut self, x: f32, y: f32) {}
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {}
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {}
    fn close(&mut self) {}
    fn fill(&mut self) {}
    fn stroke(&mut self) {}
    fn fill_and_stroke(&mut self) {}
}

/// Run `visitor` against every opcode in `bytes`. Returns `Ok(())`
/// when the stream terminated cleanly at [`OP_END`].
pub fn replay<V: Visitor>(bytes: &[u8], visitor: &mut V) -> Result<(), ReplayError> {
    let mut cur = Cursor::new(bytes)?;
    loop {
        let op = cur.read_u8()?;
        match op {
            OP_END => return Ok(()),
            OP_SAVE => visitor.save(),
            OP_RESTORE => visitor.restore(),
            OP_CONCAT => {
                let a = cur.read_f32()?;
                let b = cur.read_f32()?;
                let c = cur.read_f32()?;
                let d = cur.read_f32()?;
                let tx = cur.read_f32()?;
                let ty = cur.read_f32()?;
                visitor.concat(&Transform { a, b, c, d, tx, ty });
            }
            OP_VIEWPORT => {
                let x = cur.read_f32()?;
                let y = cur.read_f32()?;
                let w = cur.read_f32()?;
                let h = cur.read_f32()?;
                visitor.viewport(x, y, w, h);
            }
            OP_PAINT_FILL_COLOR => {
                let c = cur.read_color()?;
                visitor.fill_color(c);
            }
            OP_PAINT_STROKE_COLOR => {
                let c = cur.read_color()?;
                visitor.stroke_color(c);
            }
            OP_PAINT_STROKE_WIDTH => {
                let w = cur.read_f32()?;
                visitor.stroke_width(w);
            }
            OP_PAINT_OPACITY => {
                let a = cur.read_f32()?;
                visitor.opacity(a);
            }
            OP_PAINT_FILL_TINT => visitor.fill_tint(),
            OP_PAINT_STROKE_TINT => visitor.stroke_tint(),
            OP_PATH_BEGIN => visitor.path_begin(),
            OP_PATH_MOVE_TO => {
                let x = cur.read_f32()?;
                let y = cur.read_f32()?;
                visitor.move_to(x, y);
            }
            OP_PATH_LINE_TO => {
                let x = cur.read_f32()?;
                let y = cur.read_f32()?;
                visitor.line_to(x, y);
            }
            OP_PATH_QUAD_TO => {
                let cx = cur.read_f32()?;
                let cy = cur.read_f32()?;
                let x = cur.read_f32()?;
                let y = cur.read_f32()?;
                visitor.quad_to(cx, cy, x, y);
            }
            OP_PATH_CUBIC_TO => {
                let c1x = cur.read_f32()?;
                let c1y = cur.read_f32()?;
                let c2x = cur.read_f32()?;
                let c2y = cur.read_f32()?;
                let x = cur.read_f32()?;
                let y = cur.read_f32()?;
                visitor.cubic_to(c1x, c1y, c2x, c2y, x, y);
            }
            OP_PATH_CLOSE => visitor.close(),
            OP_PATH_FILL => visitor.fill(),
            OP_PATH_STROKE => visitor.stroke(),
            OP_PATH_FILL_AND_STROKE => visitor.fill_and_stroke(),
            other if is_reserved(other) => return Err(ReplayError::UnsupportedOpcode(other)),
            other => return Err(ReplayError::UnknownOpcode(other)),
        }
    }
}

/// Internal byte-cursor — fans out checked little-endian reads
/// against the replay loop. Centralised here so a `Truncated` is
/// produced from one branch.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Result<Self, ReplayError> {
        if buf.len() < HEADER_LEN {
            return Err(ReplayError::HeaderTooShort);
        }
        if buf[0..4] != MAGIC {
            return Err(ReplayError::BadMagic);
        }
        let version = buf[4];
        if version > VERSION {
            return Err(ReplayError::UnsupportedVersion(version));
        }
        let flags = buf[5];
        if flags != FLAGS_RESERVED {
            return Err(ReplayError::UnsupportedFlags(flags));
        }
        Ok(Cursor {
            buf,
            pos: HEADER_LEN,
        })
    }

    fn read_u8(&mut self) -> Result<u8, ReplayError> {
        if self.pos >= self.buf.len() {
            return Err(ReplayError::Truncated);
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_f32(&mut self) -> Result<f32, ReplayError> {
        if self.pos + 4 > self.buf.len() {
            return Err(ReplayError::Truncated);
        }
        let mut arr = [0u8; 4];
        arr.copy_from_slice(&self.buf[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(f32::from_le_bytes(arr))
    }

    fn read_color(&mut self) -> Result<Color, ReplayError> {
        if self.pos + 4 > self.buf.len() {
            return Err(ReplayError::Truncated);
        }
        let r = self.buf[self.pos];
        let g = self.buf[self.pos + 1];
        let b = self.buf[self.pos + 2];
        let a = self.buf[self.pos + 3];
        self.pos += 4;
        Ok(Color { r, g, b, a })
    }
}

// ---- helpers ---------------------------------------------------------------

/// `Visitor` that records every dispatched call as a text line —
/// the primary tool for golden-file tests in `tests/`. Trace
/// lines mirror the human-readable opcode names in SPEC.md so a
/// diff against the captured trace is the byte-format change
/// review surface.
#[derive(Debug, Default)]
pub struct TraceVisitor {
    pub lines: Vec<String>,
}

impl TraceVisitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_string(self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

impl Visitor for TraceVisitor {
    fn save(&mut self) {
        self.lines.push("SAVE".into());
    }
    fn restore(&mut self) {
        self.lines.push("RESTORE".into());
    }
    fn concat(&mut self, t: &Transform) {
        self.lines.push(format!(
            "CONCAT [{} {} {} {} {} {}]",
            fmt_f(t.a),
            fmt_f(t.b),
            fmt_f(t.c),
            fmt_f(t.d),
            fmt_f(t.tx),
            fmt_f(t.ty),
        ));
    }
    fn viewport(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.lines.push(format!(
            "VIEWPORT {} {} {} {}",
            fmt_f(x),
            fmt_f(y),
            fmt_f(w),
            fmt_f(h),
        ));
    }
    fn fill_color(&mut self, c: Color) {
        self.lines.push(format!(
            "FILL_COLOR #{:02X}{:02X}{:02X}{:02X}",
            c.r, c.g, c.b, c.a
        ));
    }
    fn stroke_color(&mut self, c: Color) {
        self.lines.push(format!(
            "STROKE_COLOR #{:02X}{:02X}{:02X}{:02X}",
            c.r, c.g, c.b, c.a
        ));
    }
    fn stroke_width(&mut self, w: f32) {
        self.lines.push(format!("STROKE_WIDTH {}", fmt_f(w)));
    }
    fn opacity(&mut self, a: f32) {
        self.lines.push(format!("OPACITY {}", fmt_f(a)));
    }
    fn fill_tint(&mut self) {
        self.lines.push("FILL_TINT".into());
    }
    fn stroke_tint(&mut self) {
        self.lines.push("STROKE_TINT".into());
    }
    fn path_begin(&mut self) {
        self.lines.push("PATH_BEGIN".into());
    }
    fn move_to(&mut self, x: f32, y: f32) {
        self.lines
            .push(format!("MOVE_TO {} {}", fmt_f(x), fmt_f(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.lines
            .push(format!("LINE_TO {} {}", fmt_f(x), fmt_f(y)));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.lines.push(format!(
            "QUAD_TO {} {} {} {}",
            fmt_f(cx),
            fmt_f(cy),
            fmt_f(x),
            fmt_f(y),
        ));
    }
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.lines.push(format!(
            "CUBIC_TO {} {} {} {} {} {}",
            fmt_f(c1x),
            fmt_f(c1y),
            fmt_f(c2x),
            fmt_f(c2y),
            fmt_f(x),
            fmt_f(y),
        ));
    }
    fn close(&mut self) {
        self.lines.push("CLOSE".into());
    }
    fn fill(&mut self) {
        self.lines.push("FILL".into());
    }
    fn stroke(&mut self) {
        self.lines.push("STROKE".into());
    }
    fn fill_and_stroke(&mut self) {
        self.lines.push("FILL_AND_STROKE".into());
    }
}

/// Compact float formatter for trace output. Strips a trailing
/// `.0` so `42.0` prints as `42` (matches what humans write in
/// fixture trace files) and otherwise renders Rust's `{}` form.
fn fmt_f(v: f32) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}
