//! Round-trip tests that don't depend on golden files — they
//! exercise the format invariants directly (header validation,
//! every opcode encoded → decoded back to its arguments, error
//! surfaces).

use whisker_svg_core::builder::{Color, Transform};
use whisker_svg_core::format::*;
use whisker_svg_core::replay::{replay, ReplayError, Visitor};
use whisker_svg_core::DisplayListBuilder;

/// Capture every visitor call as a tagged event so we can
/// match-assert the sequence and arguments precisely.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Save,
    Restore,
    Concat(Transform),
    Viewport(f32, f32, f32, f32),
    FillColor(Color),
    StrokeColor(Color),
    StrokeWidth(f32),
    Opacity(f32),
    FillTint,
    StrokeTint,
    PathBegin,
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
    Fill,
    Stroke,
    FillAndStroke,
}

#[derive(Default)]
struct Recorder(Vec<Event>);

impl Visitor for Recorder {
    fn save(&mut self) {
        self.0.push(Event::Save)
    }
    fn restore(&mut self) {
        self.0.push(Event::Restore)
    }
    fn concat(&mut self, t: &Transform) {
        self.0.push(Event::Concat(*t))
    }
    fn viewport(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.0.push(Event::Viewport(x, y, w, h));
    }
    fn fill_color(&mut self, c: Color) {
        self.0.push(Event::FillColor(c))
    }
    fn stroke_color(&mut self, c: Color) {
        self.0.push(Event::StrokeColor(c))
    }
    fn stroke_width(&mut self, w: f32) {
        self.0.push(Event::StrokeWidth(w))
    }
    fn opacity(&mut self, a: f32) {
        self.0.push(Event::Opacity(a))
    }
    fn fill_tint(&mut self) {
        self.0.push(Event::FillTint)
    }
    fn stroke_tint(&mut self) {
        self.0.push(Event::StrokeTint)
    }
    fn path_begin(&mut self) {
        self.0.push(Event::PathBegin)
    }
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Event::MoveTo(x, y))
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Event::LineTo(x, y))
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0.push(Event::QuadTo(cx, cy, x, y));
    }
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.0.push(Event::CubicTo(c1x, c1y, c2x, c2y, x, y));
    }
    fn close(&mut self) {
        self.0.push(Event::Close)
    }
    fn fill(&mut self) {
        self.0.push(Event::Fill)
    }
    fn stroke(&mut self) {
        self.0.push(Event::Stroke)
    }
    fn fill_and_stroke(&mut self) {
        self.0.push(Event::FillAndStroke)
    }
}

fn roundtrip<F: FnOnce(&mut DisplayListBuilder)>(f: F) -> Vec<Event> {
    let mut b = DisplayListBuilder::new();
    f(&mut b);
    let bytes = b.finish();
    let mut r = Recorder::default();
    replay(&bytes, &mut r).expect("clean replay");
    r.0
}

#[test]
fn header_is_six_bytes() {
    let b = DisplayListBuilder::new();
    assert_eq!(b.len(), HEADER_LEN);
    assert!(b.is_empty());
    let bytes = b.finish();
    // 6-byte header + END
    assert_eq!(bytes.len(), HEADER_LEN + 1);
    assert_eq!(&bytes[0..4], &MAGIC);
    assert_eq!(bytes[4], VERSION);
    assert_eq!(bytes[5], FLAGS_RESERVED);
    assert_eq!(bytes[6], OP_END);
}

#[test]
fn every_opcode_roundtrips() {
    let events = roundtrip(|b| {
        b.viewport(0.0, 0.0, 24.0, 24.0);
        b.save();
        b.concat(&Transform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 5.0,
            ty: -3.5,
        });
        b.opacity(0.75);
        b.fill_color(Color::rgba(0x10, 0x20, 0x30, 0xC0));
        b.stroke_color(Color::rgba(0xAA, 0xBB, 0xCC, 0xDD));
        b.stroke_width(2.5);
        b.fill_tint();
        b.stroke_tint();
        b.path_begin();
        b.move_to(1.0, 2.0);
        b.line_to(3.0, 4.0);
        b.quad_to(5.0, 6.0, 7.0, 8.0);
        b.cubic_to(9.0, 10.0, 11.0, 12.0, 13.0, 14.0);
        b.close();
        b.fill();
        b.stroke();
        b.fill_and_stroke();
        b.restore();
    });

    assert_eq!(
        events,
        vec![
            Event::Viewport(0.0, 0.0, 24.0, 24.0),
            Event::Save,
            Event::Concat(Transform {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 5.0,
                ty: -3.5
            }),
            Event::Opacity(0.75),
            Event::FillColor(Color::rgba(0x10, 0x20, 0x30, 0xC0)),
            Event::StrokeColor(Color::rgba(0xAA, 0xBB, 0xCC, 0xDD)),
            Event::StrokeWidth(2.5),
            Event::FillTint,
            Event::StrokeTint,
            Event::PathBegin,
            Event::MoveTo(1.0, 2.0),
            Event::LineTo(3.0, 4.0),
            Event::QuadTo(5.0, 6.0, 7.0, 8.0),
            Event::CubicTo(9.0, 10.0, 11.0, 12.0, 13.0, 14.0),
            Event::Close,
            Event::Fill,
            Event::Stroke,
            Event::FillAndStroke,
            Event::Restore,
        ]
    );
}

#[test]
fn bad_magic_rejected() {
    let bytes = b"NOPE\x01\x00\xFF".to_vec();
    let mut r = Recorder::default();
    assert_eq!(replay(&bytes, &mut r), Err(ReplayError::BadMagic));
    assert!(r.0.is_empty());
}

#[test]
fn unsupported_version_rejected() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION + 1); // future version
    bytes.push(FLAGS_RESERVED);
    bytes.push(OP_END);
    let mut r = Recorder::default();
    assert_eq!(
        replay(&bytes, &mut r),
        Err(ReplayError::UnsupportedVersion(VERSION + 1))
    );
}

#[test]
fn non_zero_flags_rejected() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(0x01); // any non-zero flag
    bytes.push(OP_END);
    let mut r = Recorder::default();
    assert_eq!(
        replay(&bytes, &mut r),
        Err(ReplayError::UnsupportedFlags(1))
    );
}

#[test]
fn header_too_short() {
    let bytes = vec![b'W', b'S', b'D'];
    let mut r = Recorder::default();
    assert_eq!(replay(&bytes, &mut r), Err(ReplayError::HeaderTooShort));
}

#[test]
fn truncated_mid_op() {
    // Header + start of CONCAT but missing the 6 floats.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(FLAGS_RESERVED);
    bytes.push(OP_CONCAT);
    bytes.extend_from_slice(&[0u8; 12]); // half the floats, then end-of-buf
    let mut r = Recorder::default();
    assert_eq!(replay(&bytes, &mut r), Err(ReplayError::Truncated));
}

#[test]
fn missing_end_marker_is_truncated() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(FLAGS_RESERVED);
    bytes.push(OP_SAVE);
    // no OP_END
    let mut r = Recorder::default();
    assert_eq!(replay(&bytes, &mut r), Err(ReplayError::Truncated));
    // Save was dispatched before truncation was detected.
    assert_eq!(r.0, vec![Event::Save]);
}

#[test]
fn reserved_opcode_rejected() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(FLAGS_RESERVED);
    bytes.push(0x40); // first reserved (gradient)
    bytes.push(OP_END);
    let mut r = Recorder::default();
    assert_eq!(
        replay(&bytes, &mut r),
        Err(ReplayError::UnsupportedOpcode(0x40))
    );
}

#[test]
fn unknown_opcode_rejected() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(FLAGS_RESERVED);
    bytes.push(0x06); // gap in container range, not reserved
    bytes.push(OP_END);
    let mut r = Recorder::default();
    assert_eq!(
        replay(&bytes, &mut r),
        Err(ReplayError::UnknownOpcode(0x06))
    );
}

#[test]
fn color_byte_order_is_rgba() {
    // Manually craft FILL_COLOR with RGBA = (0x11, 0x22, 0x33, 0x44)
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(FLAGS_RESERVED);
    bytes.push(OP_PAINT_FILL_COLOR);
    bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    bytes.push(OP_END);
    let mut r = Recorder::default();
    replay(&bytes, &mut r).unwrap();
    assert_eq!(
        r.0,
        vec![Event::FillColor(Color::rgba(0x11, 0x22, 0x33, 0x44))]
    );
}

#[test]
fn floats_are_little_endian() {
    // CONCAT [1.0 0 0 1.0 0 0] = identity. f32::to_le_bytes(1.0) ==
    // [0x00, 0x00, 0x80, 0x3F].
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.push(FLAGS_RESERVED);
    bytes.push(OP_CONCAT);
    let one = [0x00u8, 0x00, 0x80, 0x3F];
    let zero = [0x00u8, 0x00, 0x00, 0x00];
    bytes.extend_from_slice(&one); // a
    bytes.extend_from_slice(&zero); // b
    bytes.extend_from_slice(&zero); // c
    bytes.extend_from_slice(&one); // d
    bytes.extend_from_slice(&zero); // tx
    bytes.extend_from_slice(&zero); // ty
    bytes.push(OP_END);
    let mut r = Recorder::default();
    replay(&bytes, &mut r).unwrap();
    assert_eq!(r.0, vec![Event::Concat(Transform::IDENTITY)]);
}
