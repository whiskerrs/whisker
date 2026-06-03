//! Builder — fluent API for emitting a display-list byte stream
//! that conforms to `packages/whisker-svg/SPEC.md`.
//!
//! All write methods are infallible (push to a `Vec<u8>`). The
//! caller calls [`DisplayListBuilder::finish`] when done to get
//! the bytes back; finish writes the trailing `OP_END` and returns
//! the buffer.
//!
//! ## Tint paint mode
//!
//! The two `*_tint` methods correspond to `OP_PAINT_FILL_TINT` /
//! `OP_PAINT_STROKE_TINT` — they tell the replayer to substitute
//! the host's CSS `color` for the next fill/stroke. Producers
//! emit these from SVG `fill="currentColor"` / `stroke="currentColor"`.

use crate::format::*;

/// 32-bit ARGB-packed colour. The wire format is `R, G, B, A` byte
/// order (NOT `A, R, G, B`); the producer breaks it apart before
/// writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0xFF,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 0xFF }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }
}

/// 2 × 3 affine transform in column-major
/// CoreGraphics / Android `Matrix.setValues` convention. Identity
/// is `[1, 0, 0, 1, 0, 0]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub const fn translate(tx: f32, ty: f32) -> Transform {
        Transform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx,
            ty,
        }
    }

    pub const fn scale(sx: f32, sy: f32) -> Transform {
        Transform {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

/// Encoder for a v1 display-list stream.
///
/// Build with [`DisplayListBuilder::new`], call any combination of
/// the `op_*` methods in source order, and finish with
/// [`DisplayListBuilder::finish`] to get the final `Vec<u8>`. The
/// builder doesn't validate ordering — emitting `PATH_FILL` before
/// any `PATH_BEGIN` is legal at the byte level (the replayer
/// decides what that means).
pub struct DisplayListBuilder {
    buf: Vec<u8>,
}

impl Default for DisplayListBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayListBuilder {
    /// Allocate a new builder and write the 6-byte header.
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&MAGIC);
        buf.push(VERSION);
        buf.push(FLAGS_RESERVED);
        Self { buf }
    }

    /// Append the trailing `OP_END` and return the bytes.
    pub fn finish(mut self) -> Vec<u8> {
        self.buf.push(OP_END);
        self.buf
    }

    /// Number of bytes written so far (incl. header, excl. the END
    /// that `finish` will append).
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.len() == HEADER_LEN
    }

    // ---- container / canvas state -----------------------------------------

    pub fn save(&mut self) {
        self.buf.push(OP_SAVE);
    }

    pub fn restore(&mut self) {
        self.buf.push(OP_RESTORE);
    }

    pub fn concat(&mut self, t: &Transform) {
        self.buf.push(OP_CONCAT);
        for v in [t.a, t.b, t.c, t.d, t.tx, t.ty] {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    pub fn viewport(&mut self, min_x: f32, min_y: f32, width: f32, height: f32) {
        self.buf.push(OP_VIEWPORT);
        for v in [min_x, min_y, width, height] {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    // ---- paint state ------------------------------------------------------

    pub fn fill_color(&mut self, c: Color) {
        self.buf.push(OP_PAINT_FILL_COLOR);
        self.buf.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }

    pub fn stroke_color(&mut self, c: Color) {
        self.buf.push(OP_PAINT_STROKE_COLOR);
        self.buf.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }

    pub fn stroke_width(&mut self, w: f32) {
        self.buf.push(OP_PAINT_STROKE_WIDTH);
        self.buf.extend_from_slice(&w.to_le_bytes());
    }

    pub fn opacity(&mut self, alpha: f32) {
        self.buf.push(OP_PAINT_OPACITY);
        self.buf.extend_from_slice(&alpha.to_le_bytes());
    }

    pub fn fill_tint(&mut self) {
        self.buf.push(OP_PAINT_FILL_TINT);
    }

    pub fn stroke_tint(&mut self) {
        self.buf.push(OP_PAINT_STROKE_TINT);
    }

    // ---- path commands ----------------------------------------------------

    pub fn path_begin(&mut self) {
        self.buf.push(OP_PATH_BEGIN);
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.buf.push(OP_PATH_MOVE_TO);
        self.buf.extend_from_slice(&x.to_le_bytes());
        self.buf.extend_from_slice(&y.to_le_bytes());
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.buf.push(OP_PATH_LINE_TO);
        self.buf.extend_from_slice(&x.to_le_bytes());
        self.buf.extend_from_slice(&y.to_le_bytes());
    }

    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.buf.push(OP_PATH_QUAD_TO);
        for v in [cx, cy, x, y] {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    pub fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.buf.push(OP_PATH_CUBIC_TO);
        for v in [c1x, c1y, c2x, c2y, x, y] {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    pub fn close(&mut self) {
        self.buf.push(OP_PATH_CLOSE);
    }

    // ---- path execution ---------------------------------------------------

    pub fn fill(&mut self) {
        self.buf.push(OP_PATH_FILL);
    }

    pub fn stroke(&mut self) {
        self.buf.push(OP_PATH_STROKE);
    }

    pub fn fill_and_stroke(&mut self) {
        self.buf.push(OP_PATH_FILL_AND_STROKE);
    }
}
