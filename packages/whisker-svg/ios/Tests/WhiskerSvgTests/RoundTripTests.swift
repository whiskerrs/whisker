// Self-contained Swift round-trip tests for the display-list
// replayer. These pin the format invariants directly: every opcode
// constructed in Swift bytes → decoded through the visitor →
// matches its arguments; header validation; error surfaces.
//
// Cross-platform conformance (Rust producer ↔ Swift decoder on the
// same bytes) is in `ConformanceTests.swift`.

import XCTest
@testable import WhiskerSvg

final class RoundTripTests: XCTestCase {

    /// Build the canonical 6-byte header.
    private func header() -> [UInt8] {
        return DLFormat.magic + [DLFormat.version, DLFormat.flagsReserved]
    }

    private func leBytes(_ v: Float) -> [UInt8] {
        let bits = v.bitPattern
        return [
            UInt8(bits & 0xFF),
            UInt8((bits >> 8) & 0xFF),
            UInt8((bits >> 16) & 0xFF),
            UInt8((bits >> 24) & 0xFF),
        ]
    }

    func testHeaderIsSixBytes() throws {
        let bytes = Data(header() + [DLFormat.opEnd])
        var v = DLTraceVisitor()
        try dlReplay(bytes, into: &v)
        XCTAssertTrue(v.lines.isEmpty)
    }

    func testEveryOpcodeRoundTrips() throws {
        var raw: [UInt8] = header()
        // VIEWPORT 0 0 24 24
        raw.append(DLFormat.opViewport)
        raw += leBytes(0) + leBytes(0) + leBytes(24) + leBytes(24)
        // SAVE
        raw.append(DLFormat.opSave)
        // CONCAT [1 0 0 1 5 -3.5]
        raw.append(DLFormat.opConcat)
        raw += leBytes(1) + leBytes(0) + leBytes(0) + leBytes(1) + leBytes(5) + leBytes(-3.5)
        // OPACITY 0.75
        raw.append(DLFormat.opPaintOpacity)
        raw += leBytes(0.75)
        // FILL_COLOR #102030C0
        raw.append(DLFormat.opPaintFillColor)
        raw += [0x10, 0x20, 0x30, 0xC0]
        // STROKE_COLOR #AABBCCDD
        raw.append(DLFormat.opPaintStrokeColor)
        raw += [0xAA, 0xBB, 0xCC, 0xDD]
        // STROKE_WIDTH 2.5
        raw.append(DLFormat.opPaintStrokeWidth)
        raw += leBytes(2.5)
        // FILL_TINT
        raw.append(DLFormat.opPaintFillTint)
        // STROKE_TINT
        raw.append(DLFormat.opPaintStrokeTint)
        // PATH_BEGIN, MOVE_TO 1 2, LINE_TO 3 4, QUAD_TO 5 6 7 8,
        // CUBIC_TO 9 10 11 12 13 14, CLOSE, FILL, STROKE,
        // FILL_AND_STROKE
        raw.append(DLFormat.opPathBegin)
        raw.append(DLFormat.opPathMoveTo)
        raw += leBytes(1) + leBytes(2)
        raw.append(DLFormat.opPathLineTo)
        raw += leBytes(3) + leBytes(4)
        raw.append(DLFormat.opPathQuadTo)
        raw += leBytes(5) + leBytes(6) + leBytes(7) + leBytes(8)
        raw.append(DLFormat.opPathCubicTo)
        raw += leBytes(9) + leBytes(10) + leBytes(11) + leBytes(12) + leBytes(13) + leBytes(14)
        raw.append(DLFormat.opPathClose)
        raw.append(DLFormat.opPathFill)
        raw.append(DLFormat.opPathStroke)
        raw.append(DLFormat.opPathFillAndStroke)
        // RESTORE, END
        raw.append(DLFormat.opRestore)
        raw.append(DLFormat.opEnd)

        var visitor = DLTraceVisitor()
        try dlReplay(Data(raw), into: &visitor)
        let expected = """
        VIEWPORT 0 0 24 24
        SAVE
        CONCAT [1 0 0 1 5 -3.5]
        OPACITY 0.75
        FILL_COLOR #102030C0
        STROKE_COLOR #AABBCCDD
        STROKE_WIDTH 2.5
        FILL_TINT
        STROKE_TINT
        PATH_BEGIN
        MOVE_TO 1 2
        LINE_TO 3 4
        QUAD_TO 5 6 7 8
        CUBIC_TO 9 10 11 12 13 14
        CLOSE
        FILL
        STROKE
        FILL_AND_STROKE
        RESTORE

        """
        XCTAssertEqual(visitor.asString(), expected)
    }

    // ---- error surfaces ---------------------------------------------------

    func testBadMagicRejected() {
        let bytes = Data([0x4E, 0x4F, 0x50, 0x45, 0x01, 0x00, DLFormat.opEnd])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.badMagic {
            // ok
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testUnsupportedVersionRejected() {
        let bytes = Data(DLFormat.magic + [DLFormat.version + 1, DLFormat.flagsReserved, DLFormat.opEnd])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.unsupportedVersion(let got) {
            XCTAssertEqual(got, DLFormat.version + 1)
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testNonZeroFlagsRejected() {
        let bytes = Data(DLFormat.magic + [DLFormat.version, 0x01, DLFormat.opEnd])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.unsupportedFlags(let got) {
            XCTAssertEqual(got, 1)
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testHeaderTooShort() {
        let bytes = Data([0x57, 0x53, 0x44])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.headerTooShort {
            // ok
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testTruncatedMidOp() {
        // Header + start of CONCAT but missing the 6 floats.
        var raw: [UInt8] = header() + [DLFormat.opConcat] + Array(repeating: 0, count: 12)
        let bytes = Data(raw)
        _ = raw // silence
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.truncated {
            // ok
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testMissingEndIsTruncated() {
        let bytes = Data(header() + [DLFormat.opSave])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.truncated {
            XCTAssertEqual(v.lines, ["SAVE"]) // SAVE dispatched before truncation
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testReservedOpcodeRejected() {
        // 0x40 = first gradient reservation
        let bytes = Data(header() + [0x40, DLFormat.opEnd])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.unsupportedOpcode(let got) {
            XCTAssertEqual(got, 0x40)
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testUnknownOpcodeRejected() {
        // 0x06 — gap inside container range, not reserved.
        let bytes = Data(header() + [0x06, DLFormat.opEnd])
        var v = DLTraceVisitor()
        do {
            try dlReplay(bytes, into: &v)
            XCTFail("expected throw")
        } catch DLReplayError.unknownOpcode(let got) {
            XCTAssertEqual(got, 0x06)
        } catch {
            XCTFail("wrong error: \(error)")
        }
    }

    func testColorByteOrderIsRgba() throws {
        let bytes = Data(header() + [DLFormat.opPaintFillColor, 0x11, 0x22, 0x33, 0x44, DLFormat.opEnd])
        var v = DLTraceVisitor()
        try dlReplay(bytes, into: &v)
        XCTAssertEqual(v.lines, ["FILL_COLOR #11223344"])
    }

    func testFloatsAreLittleEndian() throws {
        // CONCAT [1 0 0 1 0 0] = identity. f32::to_le_bytes(1.0) ==
        // [0x00 0x00 0x80 0x3F].
        var raw: [UInt8] = header() + [DLFormat.opConcat]
        let one: [UInt8] = [0x00, 0x00, 0x80, 0x3F]
        let zero: [UInt8] = [0x00, 0x00, 0x00, 0x00]
        raw += one + zero + zero + one + zero + zero + [DLFormat.opEnd]
        var v = DLTraceVisitor()
        try dlReplay(Data(raw), into: &v)
        XCTAssertEqual(v.lines, ["CONCAT [1 0 0 1 0 0]"])
    }
}
