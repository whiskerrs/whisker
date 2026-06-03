// Self-contained Kotlin round-trip tests for the display-list
// replayer. Every opcode constructed in bytes → decoded through
// the visitor → matches its arguments; header validation; error
// surfaces.
//
// Cross-platform conformance (Rust producer ↔ Kotlin decoder on the
// same bytes) is in `ConformanceTest.kt`.

package rs.whisker.modules.svg

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class RoundTripTest {

    private fun header(): ByteArray =
        DLFormat.MAGIC + byteArrayOf(DLFormat.VERSION, DLFormat.FLAGS_RESERVED)

    private fun leBytes(v: Float): ByteArray {
        val bb = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN)
        bb.putFloat(v)
        return bb.array()
    }

    @Test
    fun headerIsSixBytes() {
        val bytes = header() + byteArrayOf(DLFormat.OP_END)
        val v = DLTraceVisitor()
        dlReplay(bytes, v)
        assertTrue(v.lines.isEmpty())
    }

    @Test
    fun everyOpcodeRoundTrips() {
        val out = mutableListOf<Byte>()
        out += header().toList()
        // VIEWPORT 0 0 24 24
        out += DLFormat.OP_VIEWPORT
        out += leBytes(0f).toList() + leBytes(0f).toList() + leBytes(24f).toList() + leBytes(24f).toList()
        // SAVE
        out += DLFormat.OP_SAVE
        // CONCAT [1 0 0 1 5 -3.5]
        out += DLFormat.OP_CONCAT
        out += leBytes(1f).toList() + leBytes(0f).toList() + leBytes(0f).toList() +
            leBytes(1f).toList() + leBytes(5f).toList() + leBytes(-3.5f).toList()
        // OPACITY 0.75
        out += DLFormat.OP_PAINT_OPACITY
        out += leBytes(0.75f).toList()
        // FILL_COLOR #102030C0
        out += DLFormat.OP_PAINT_FILL_COLOR
        out += byteArrayOf(0x10, 0x20, 0x30, 0xC0.toByte()).toList()
        // STROKE_COLOR #AABBCCDD
        out += DLFormat.OP_PAINT_STROKE_COLOR
        out += byteArrayOf(0xAA.toByte(), 0xBB.toByte(), 0xCC.toByte(), 0xDD.toByte()).toList()
        // STROKE_WIDTH 2.5
        out += DLFormat.OP_PAINT_STROKE_WIDTH
        out += leBytes(2.5f).toList()
        // FILL_TINT, STROKE_TINT
        out += DLFormat.OP_PAINT_FILL_TINT
        out += DLFormat.OP_PAINT_STROKE_TINT
        // PATH_BEGIN
        out += DLFormat.OP_PATH_BEGIN
        // MOVE_TO 1 2
        out += DLFormat.OP_PATH_MOVE_TO
        out += leBytes(1f).toList() + leBytes(2f).toList()
        // LINE_TO 3 4
        out += DLFormat.OP_PATH_LINE_TO
        out += leBytes(3f).toList() + leBytes(4f).toList()
        // QUAD_TO 5 6 7 8
        out += DLFormat.OP_PATH_QUAD_TO
        out += leBytes(5f).toList() + leBytes(6f).toList() + leBytes(7f).toList() + leBytes(8f).toList()
        // CUBIC_TO 9 10 11 12 13 14
        out += DLFormat.OP_PATH_CUBIC_TO
        out += leBytes(9f).toList() + leBytes(10f).toList() + leBytes(11f).toList() +
            leBytes(12f).toList() + leBytes(13f).toList() + leBytes(14f).toList()
        // CLOSE, FILL, STROKE, FILL_AND_STROKE
        out += DLFormat.OP_PATH_CLOSE
        out += DLFormat.OP_PATH_FILL
        out += DLFormat.OP_PATH_STROKE
        out += DLFormat.OP_PATH_FILL_AND_STROKE
        // RESTORE, END
        out += DLFormat.OP_RESTORE
        out += DLFormat.OP_END

        val v = DLTraceVisitor()
        dlReplay(out.toByteArray(), v)
        val expected = """
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
        """.trimIndent() + "\n"
        assertEquals(expected, v.asString())
    }

    // ---- error surfaces ---------------------------------------------------

    @Test
    fun badMagicRejected() {
        val bytes = byteArrayOf(0x4E, 0x4F, 0x50, 0x45, 0x01, 0x00, DLFormat.OP_END)
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.BadMagic) {
            // ok
        }
    }

    @Test
    fun unsupportedVersionRejected() {
        val bytes = DLFormat.MAGIC + byteArrayOf(
            (DLFormat.VERSION + 1).toByte(),
            DLFormat.FLAGS_RESERVED,
            DLFormat.OP_END,
        )
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.UnsupportedVersion) {
            assertEquals(2, e.version)
        }
    }

    @Test
    fun nonZeroFlagsRejected() {
        val bytes = DLFormat.MAGIC + byteArrayOf(DLFormat.VERSION, 0x01, DLFormat.OP_END)
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.UnsupportedFlags) {
            assertEquals(1, e.flags)
        }
    }

    @Test
    fun headerTooShort() {
        val bytes = byteArrayOf(0x57, 0x53, 0x44)
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.HeaderTooShort) {
            // ok
        }
    }

    @Test
    fun truncatedMidOp() {
        // Header + start of CONCAT but missing the 6 floats.
        val bytes = header() + byteArrayOf(DLFormat.OP_CONCAT) + ByteArray(12)
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.Truncated) {
            // ok
        }
    }

    @Test
    fun missingEndMarkerIsTruncated() {
        val bytes = header() + byteArrayOf(DLFormat.OP_SAVE)
        val v = DLTraceVisitor()
        try {
            dlReplay(bytes, v)
            fail("expected throw")
        } catch (e: DLReplayError.Truncated) {
            // SAVE was dispatched before truncation was detected.
            assertEquals(listOf("SAVE"), v.lines)
        }
    }

    @Test
    fun reservedOpcodeRejected() {
        val bytes = header() + byteArrayOf(0x40, DLFormat.OP_END)
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.UnsupportedOpcode) {
            assertEquals(0x40, e.op)
        }
    }

    @Test
    fun unknownOpcodeRejected() {
        // 0x06 — gap inside container range, not reserved.
        val bytes = header() + byteArrayOf(0x06, DLFormat.OP_END)
        try {
            dlReplay(bytes, DLTraceVisitor())
            fail("expected throw")
        } catch (e: DLReplayError.UnknownOpcode) {
            assertEquals(0x06, e.op)
        }
    }

    @Test
    fun colorByteOrderIsRgba() {
        val bytes = header() + byteArrayOf(
            DLFormat.OP_PAINT_FILL_COLOR,
            0x11, 0x22, 0x33, 0x44,
            DLFormat.OP_END,
        )
        val v = DLTraceVisitor()
        dlReplay(bytes, v)
        assertEquals(listOf("FILL_COLOR #11223344"), v.lines)
    }

    @Test
    fun floatsAreLittleEndian() {
        // CONCAT [1 0 0 1 0 0] = identity. f32::to_le_bytes(1.0) ==
        // [0x00 0x00 0x80 0x3F].
        val one = byteArrayOf(0x00, 0x00, 0x80.toByte(), 0x3F)
        val zero = byteArrayOf(0x00, 0x00, 0x00, 0x00)
        val bytes = header() + byteArrayOf(DLFormat.OP_CONCAT) +
            one + zero + zero + one + zero + zero + byteArrayOf(DLFormat.OP_END)
        val v = DLTraceVisitor()
        dlReplay(bytes, v)
        assertEquals(listOf("CONCAT [1 0 0 1 0 0]"), v.lines)
    }

    @Test
    fun matrixValuesUseColumnMajorConvention() {
        // Sanity-check that DLTransform.toMatrixValues lays the 9
        // floats in row-major `Matrix.setValues` order. A pure
        // translate(5, 7) maps to scale=1 / translate components.
        val t = DLTransform(1f, 0f, 0f, 1f, 5f, 7f)
        val vals = t.toMatrixValues()
        // android.graphics.Matrix expects:
        //   [scaleX, skewX, transX, skewY, scaleY, transY, persp0, persp1, persp2]
        assertArrayEquals(
            floatArrayOf(1f, 0f, 5f, 0f, 1f, 7f, 0f, 0f, 1f),
            vals,
            0f,
        )
    }
}
