// Wire-format constants — the literal Swift mirror of
// `packages/whisker-svg/SPEC.md`.
//
// Every value here MUST match the SPEC. The Rust producer in
// `packages/whisker-svg/src/format.rs` is the parallel
// authority; any change in either file without the matching
// counterpart edit (and SPEC bump) is a protocol break.

import Foundation

enum DLFormat {
    /// Magic bytes (`"WSDL"` — 0x57 0x53 0x44 0x4C).
    static let magic: [UInt8] = [0x57, 0x53, 0x44, 0x4C]

    /// Current wire-format version.
    static let version: UInt8 = 1

    /// `flags` byte — reserved in v1, MUST be zero.
    static let flagsReserved: UInt8 = 0

    /// Size of the 6-byte stream header (magic + version + flags).
    static let headerLen = 6

    // ---- container / state (0x00 – 0x0F) ---------------------------------

    static let opSave: UInt8 = 0x01
    static let opRestore: UInt8 = 0x02
    static let opConcat: UInt8 = 0x03
    static let opViewport: UInt8 = 0x04

    // ---- paint state (0x10 – 0x1F) ---------------------------------------

    static let opPaintFillColor: UInt8 = 0x10
    static let opPaintStrokeColor: UInt8 = 0x11
    static let opPaintStrokeWidth: UInt8 = 0x12
    static let opPaintOpacity: UInt8 = 0x13
    static let opPaintFillTint: UInt8 = 0x14
    static let opPaintStrokeTint: UInt8 = 0x15

    // ---- path commands (0x20 – 0x2F) -------------------------------------

    static let opPathBegin: UInt8 = 0x20
    static let opPathMoveTo: UInt8 = 0x21
    static let opPathLineTo: UInt8 = 0x22
    static let opPathQuadTo: UInt8 = 0x23
    static let opPathCubicTo: UInt8 = 0x24
    static let opPathClose: UInt8 = 0x25

    // ---- path execution (0x30 – 0x3F) ------------------------------------

    static let opPathFill: UInt8 = 0x30
    static let opPathStroke: UInt8 = 0x32
    static let opPathFillAndStroke: UInt8 = 0x33

    // ---- end marker ------------------------------------------------------

    static let opEnd: UInt8 = 0xFF

    /// `true` if the opcode falls in any reserved-for-future-use
    /// range. A v1 replayer MUST reject these (SPEC §"Opcode space")
    /// so a v2 stream silently degrades to a clear error rather
    /// than partial draw.
    static func isReserved(_ op: UInt8) -> Bool {
        // Mirror RESERVED_RANGES in `format.rs`.
        return (op == 0x31)
            || (op >= 0x40 && op <= 0x4F) // gradients
            || (op >= 0x50 && op <= 0x5F) // clipping
            || (op >= 0x60 && op <= 0x6F) // masking
            || (op >= 0x70 && op <= 0x7F) // text
            || (op >= 0x80 && op <= 0x8F) // images
            || (op >= 0x90 && op <= 0xEF) // future
            || (op >= 0xF0 && op <= 0xFE) // markers / control
    }
}

/// 32-bit RGBA colour matching the wire byte order. The four
/// components are 0..255, straight-alpha.
struct DLColor: Equatable {
    let r: UInt8
    let g: UInt8
    let b: UInt8
    let a: UInt8
}

/// 2 × 3 affine transform in `[a, b, c, d, tx, ty]` column-major
/// CoreGraphics convention.
struct DLTransform: Equatable {
    let a: Float
    let b: Float
    let c: Float
    let d: Float
    let tx: Float
    let ty: Float

    static let identity = DLTransform(a: 1, b: 0, c: 0, d: 1, tx: 0, ty: 0)
}
