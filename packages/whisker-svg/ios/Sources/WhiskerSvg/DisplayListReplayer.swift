// Decoder — walks a byte stream conforming to
// `packages/whisker-svg/SPEC.md` and dispatches each opcode to a
// [`DLVisitor`].
//
// The same `replay` function powers `WhiskerSvgViewTests` (with a
// `TraceVisitor` that records every dispatched call as a text line)
// and `WhiskerSvgView` (with `CGContextVisitor` that actually paints
// to a `CGContext`). Both visitors implement the same protocol;
// neither knows about the other.

import Foundation

/// Error surface for a malformed or unsupported display-list
/// stream. The replayer stops at the first error and returns.
enum DLReplayError: Error, Equatable {
    /// Header bytes didn't start with `"WSDL"`.
    case badMagic
    /// Header version byte is greater than DLFormat.version.
    case unsupportedVersion(UInt8)
    /// `flags` byte was non-zero in a v1 stream.
    case unsupportedFlags(UInt8)
    /// Stream was shorter than the 6-byte header.
    case headerTooShort
    /// Stream ended before `OP_END` was reached.
    case truncated
    /// Encountered an opcode that's reserved for future use.
    case unsupportedOpcode(UInt8)
    /// Encountered a byte that doesn't match any defined opcode
    /// AND isn't in a reserved range (a real protocol violation).
    case unknownOpcode(UInt8)
}

/// Sink for decoded opcodes. Every method has a default no-op
/// implementation so partial visitors only override what they
/// care about — matches the `Visitor` trait shape on the Rust
/// side `packages/whisker-svg/src/replay.rs`.
protocol DLVisitor {
    mutating func save()
    mutating func restore()
    mutating func concat(_ t: DLTransform)
    mutating func viewport(minX: Float, minY: Float, width: Float, height: Float)
    mutating func fillColor(_ c: DLColor)
    mutating func strokeColor(_ c: DLColor)
    mutating func strokeWidth(_ w: Float)
    mutating func opacity(_ a: Float)
    mutating func fillTint()
    mutating func strokeTint()
    mutating func pathBegin()
    mutating func moveTo(x: Float, y: Float)
    mutating func lineTo(x: Float, y: Float)
    mutating func quadTo(cx: Float, cy: Float, x: Float, y: Float)
    mutating func cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float)
    mutating func close()
    mutating func fill()
    mutating func stroke()
    mutating func fillAndStroke()
}

extension DLVisitor {
    mutating func save() {}
    mutating func restore() {}
    mutating func concat(_ t: DLTransform) {}
    mutating func viewport(minX: Float, minY: Float, width: Float, height: Float) {}
    mutating func fillColor(_ c: DLColor) {}
    mutating func strokeColor(_ c: DLColor) {}
    mutating func strokeWidth(_ w: Float) {}
    mutating func opacity(_ a: Float) {}
    mutating func fillTint() {}
    mutating func strokeTint() {}
    mutating func pathBegin() {}
    mutating func moveTo(x: Float, y: Float) {}
    mutating func lineTo(x: Float, y: Float) {}
    mutating func quadTo(cx: Float, cy: Float, x: Float, y: Float) {}
    mutating func cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float) {}
    mutating func close() {}
    mutating func fill() {}
    mutating func stroke() {}
    mutating func fillAndStroke() {}
}

/// Walks `bytes` and dispatches each opcode to `visitor`. Returns
/// when the stream's `OP_END` is reached, or throws on the first
/// malformed / unsupported byte.
func dlReplay<V: DLVisitor>(_ bytes: Data, into visitor: inout V) throws {
    guard bytes.count >= DLFormat.headerLen else {
        throw DLReplayError.headerTooShort
    }
    for i in 0..<4 {
        if bytes[i] != DLFormat.magic[i] {
            throw DLReplayError.badMagic
        }
    }
    let version = bytes[4]
    if version > DLFormat.version {
        throw DLReplayError.unsupportedVersion(version)
    }
    let flags = bytes[5]
    if flags != DLFormat.flagsReserved {
        throw DLReplayError.unsupportedFlags(flags)
    }

    var cur = Cursor(bytes: bytes, pos: DLFormat.headerLen)

    while true {
        let op = try cur.readU8()
        switch op {
        case DLFormat.opEnd:
            return
        case DLFormat.opSave:
            visitor.save()
        case DLFormat.opRestore:
            visitor.restore()
        case DLFormat.opConcat:
            let a = try cur.readF32()
            let b = try cur.readF32()
            let c = try cur.readF32()
            let d = try cur.readF32()
            let tx = try cur.readF32()
            let ty = try cur.readF32()
            visitor.concat(DLTransform(a: a, b: b, c: c, d: d, tx: tx, ty: ty))
        case DLFormat.opViewport:
            let x = try cur.readF32()
            let y = try cur.readF32()
            let w = try cur.readF32()
            let h = try cur.readF32()
            visitor.viewport(minX: x, minY: y, width: w, height: h)
        case DLFormat.opPaintFillColor:
            let c = try cur.readColor()
            visitor.fillColor(c)
        case DLFormat.opPaintStrokeColor:
            let c = try cur.readColor()
            visitor.strokeColor(c)
        case DLFormat.opPaintStrokeWidth:
            let w = try cur.readF32()
            visitor.strokeWidth(w)
        case DLFormat.opPaintOpacity:
            let a = try cur.readF32()
            visitor.opacity(a)
        case DLFormat.opPaintFillTint:
            visitor.fillTint()
        case DLFormat.opPaintStrokeTint:
            visitor.strokeTint()
        case DLFormat.opPathBegin:
            visitor.pathBegin()
        case DLFormat.opPathMoveTo:
            let x = try cur.readF32()
            let y = try cur.readF32()
            visitor.moveTo(x: x, y: y)
        case DLFormat.opPathLineTo:
            let x = try cur.readF32()
            let y = try cur.readF32()
            visitor.lineTo(x: x, y: y)
        case DLFormat.opPathQuadTo:
            let cx = try cur.readF32()
            let cy = try cur.readF32()
            let x = try cur.readF32()
            let y = try cur.readF32()
            visitor.quadTo(cx: cx, cy: cy, x: x, y: y)
        case DLFormat.opPathCubicTo:
            let c1x = try cur.readF32()
            let c1y = try cur.readF32()
            let c2x = try cur.readF32()
            let c2y = try cur.readF32()
            let x = try cur.readF32()
            let y = try cur.readF32()
            visitor.cubicTo(c1x: c1x, c1y: c1y, c2x: c2x, c2y: c2y, x: x, y: y)
        case DLFormat.opPathClose:
            visitor.close()
        case DLFormat.opPathFill:
            visitor.fill()
        case DLFormat.opPathStroke:
            visitor.stroke()
        case DLFormat.opPathFillAndStroke:
            visitor.fillAndStroke()
        default:
            if DLFormat.isReserved(op) {
                throw DLReplayError.unsupportedOpcode(op)
            } else {
                throw DLReplayError.unknownOpcode(op)
            }
        }
    }
}

/// Internal byte-cursor — fans out checked little-endian reads.
private struct Cursor {
    let bytes: Data
    var pos: Int

    mutating func readU8() throws -> UInt8 {
        guard pos < bytes.count else { throw DLReplayError.truncated }
        let v = bytes[pos]
        pos += 1
        return v
    }

    mutating func readF32() throws -> Float {
        guard pos + 4 <= bytes.count else { throw DLReplayError.truncated }
        var word: UInt32 = 0
        // Little-endian: byte 0 is LSB.
        word |= UInt32(bytes[pos])
        word |= UInt32(bytes[pos + 1]) << 8
        word |= UInt32(bytes[pos + 2]) << 16
        word |= UInt32(bytes[pos + 3]) << 24
        pos += 4
        return Float(bitPattern: word)
    }

    mutating func readColor() throws -> DLColor {
        guard pos + 4 <= bytes.count else { throw DLReplayError.truncated }
        let c = DLColor(r: bytes[pos], g: bytes[pos + 1], b: bytes[pos + 2], a: bytes[pos + 3])
        pos += 4
        return c
    }
}

// ---- helpers ---------------------------------------------------------------

/// `DLVisitor` that records every dispatched call as a text line.
/// Used by `WhiskerSvgViewTests` to assert the Swift replayer
/// produces the same trace as the Rust `TraceVisitor` does for
/// identical input bytes — the cross-platform conformance check.
///
/// Line format mirrors `packages/whisker-svg/src/replay.rs`
/// `TraceVisitor` exactly, so the produced trace string is
/// byte-for-byte the matching `*.trace.txt` from
/// `packages/whisker-svg/tests/fixtures/`.
struct DLTraceVisitor: DLVisitor {
    var lines: [String] = []

    mutating func save() { lines.append("SAVE") }
    mutating func restore() { lines.append("RESTORE") }
    mutating func concat(_ t: DLTransform) {
        lines.append("CONCAT [\(fmtF(t.a)) \(fmtF(t.b)) \(fmtF(t.c)) \(fmtF(t.d)) \(fmtF(t.tx)) \(fmtF(t.ty))]")
    }
    mutating func viewport(minX: Float, minY: Float, width: Float, height: Float) {
        lines.append("VIEWPORT \(fmtF(minX)) \(fmtF(minY)) \(fmtF(width)) \(fmtF(height))")
    }
    mutating func fillColor(_ c: DLColor) {
        lines.append(String(format: "FILL_COLOR #%02X%02X%02X%02X", c.r, c.g, c.b, c.a))
    }
    mutating func strokeColor(_ c: DLColor) {
        lines.append(String(format: "STROKE_COLOR #%02X%02X%02X%02X", c.r, c.g, c.b, c.a))
    }
    mutating func strokeWidth(_ w: Float) { lines.append("STROKE_WIDTH \(fmtF(w))") }
    mutating func opacity(_ a: Float) { lines.append("OPACITY \(fmtF(a))") }
    mutating func fillTint() { lines.append("FILL_TINT") }
    mutating func strokeTint() { lines.append("STROKE_TINT") }
    mutating func pathBegin() { lines.append("PATH_BEGIN") }
    mutating func moveTo(x: Float, y: Float) { lines.append("MOVE_TO \(fmtF(x)) \(fmtF(y))") }
    mutating func lineTo(x: Float, y: Float) { lines.append("LINE_TO \(fmtF(x)) \(fmtF(y))") }
    mutating func quadTo(cx: Float, cy: Float, x: Float, y: Float) {
        lines.append("QUAD_TO \(fmtF(cx)) \(fmtF(cy)) \(fmtF(x)) \(fmtF(y))")
    }
    mutating func cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float) {
        lines.append("CUBIC_TO \(fmtF(c1x)) \(fmtF(c1y)) \(fmtF(c2x)) \(fmtF(c2y)) \(fmtF(x)) \(fmtF(y))")
    }
    mutating func close() { lines.append("CLOSE") }
    mutating func fill() { lines.append("FILL") }
    mutating func stroke() { lines.append("STROKE") }
    mutating func fillAndStroke() { lines.append("FILL_AND_STROKE") }

    func asString() -> String {
        return lines.joined(separator: "\n") + "\n"
    }
}

/// Compact float formatter that matches the Rust `fmt_f` in
/// `replay.rs`. Drops a trailing `.0` so `42.0` prints as `42` —
/// this is what makes `*.trace.txt` golden files diff-friendly.
private func fmtF(_ v: Float) -> String {
    if v.isFinite && v.rounded() == v {
        return String(Int64(v))
    } else {
        // Match Rust's `{}` float formatter — minimum-digit
        // round-trippable. Swift's default `\(v)` is the same.
        return "\(v)"
    }
}
