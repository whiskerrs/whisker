//! Wire-format constants — the literal Rust mirror of
//! `packages/whisker-svg/SPEC.md`.
//!
//! Every value here MUST match the SPEC. Changing a constant
//! without updating the SPEC (and every platform replayer) is a
//! protocol break.

/// Magic bytes (`"WSDL"`).
pub const MAGIC: [u8; 4] = *b"WSDL";

/// Current wire-format version.
pub const VERSION: u8 = 1;

/// `flags` byte — reserved in v1, MUST be zero.
pub const FLAGS_RESERVED: u8 = 0;

// ---- container / state (0x00 – 0x0F) ---------------------------------------

pub const OP_SAVE: u8 = 0x01;
pub const OP_RESTORE: u8 = 0x02;
pub const OP_CONCAT: u8 = 0x03;
pub const OP_VIEWPORT: u8 = 0x04;

// ---- paint state (0x10 – 0x1F) ---------------------------------------------

pub const OP_PAINT_FILL_COLOR: u8 = 0x10;
pub const OP_PAINT_STROKE_COLOR: u8 = 0x11;
pub const OP_PAINT_STROKE_WIDTH: u8 = 0x12;
pub const OP_PAINT_OPACITY: u8 = 0x13;
pub const OP_PAINT_FILL_TINT: u8 = 0x14;
pub const OP_PAINT_STROKE_TINT: u8 = 0x15;

// ---- path commands (0x20 – 0x2F) -------------------------------------------

pub const OP_PATH_BEGIN: u8 = 0x20;
pub const OP_PATH_MOVE_TO: u8 = 0x21;
pub const OP_PATH_LINE_TO: u8 = 0x22;
pub const OP_PATH_QUAD_TO: u8 = 0x23;
pub const OP_PATH_CUBIC_TO: u8 = 0x24;
pub const OP_PATH_CLOSE: u8 = 0x25;

// ---- path execution (0x30 – 0x3F) ------------------------------------------

pub const OP_PATH_FILL: u8 = 0x30;
pub const OP_PATH_STROKE: u8 = 0x32;
pub const OP_PATH_FILL_AND_STROKE: u8 = 0x33;

// ---- end marker ------------------------------------------------------------

pub const OP_END: u8 = 0xFF;

// ---- reserved opcode ranges (NOT emitted in v1) ----------------------------

/// First byte of every reserved range. A replayer encountering
/// any value in [`reserved_ranges`] MUST report
/// `unsupported opcode 0x{HH}` and stop. Listed here so a future
/// PR adding (say) gradients only needs to drop entries.
pub const RESERVED_RANGES: &[(u8, u8)] = &[
    (0x31, 0x31), // PATH_FILL_EVEN_ODD (reserved)
    (0x40, 0x4F), // gradients
    (0x50, 0x5F), // clipping
    (0x60, 0x6F), // masking
    (0x70, 0x7F), // text
    (0x80, 0x8F), // images
    (0x90, 0xEF), // future
    (0xF0, 0xFE), // markers / control
];

/// Returns true if `op` falls in any reserved range.
pub fn is_reserved(op: u8) -> bool {
    RESERVED_RANGES
        .iter()
        .any(|(lo, hi)| op >= *lo && op <= *hi)
}

/// Size of the 6-byte stream header (`MAGIC` + `VERSION` + `FLAGS`).
pub const HEADER_LEN: usize = 6;
