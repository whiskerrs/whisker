// Cross-platform conformance — the bytes embedded here are an
// exact copy of `packages/whisker-svg/tests/fixtures/rect_solid.bin`
// (produced by the Rust compiler from `rect_solid.svg`). Replaying
// them through the Kotlin visitor MUST produce the same trace
// string that the Rust `TraceVisitor` writes to
// `rect_solid.trace.txt`. This is the contract that guarantees
// both the Rust producer and the Kotlin decoder agree on every
// byte of the SPEC v1 wire format.
//
// When the bytes here drift from the .bin file (someone changed
// the SPEC and re-ran `WHISKER_SVG_UPDATE_GOLDEN=1` on the Rust
// side without updating this test), the test fails — a deliberate
// "don't let the Kotlin decoder silently fall out of sync with
// the Rust producer" tripwire.
//
// Future fixtures can be added by appending more @Test methods.
// Keep the byte array compact: only the smallest fixtures should
// be inlined; larger ones (e.g. circle_basic with 4 cubics) are
// skipped because their byte-by-byte reproduction here would
// dwarf the test value.

package rs.whisker.modules.svg

import org.junit.Assert.assertEquals
import org.junit.Test

class ConformanceTest {

    private val rectSolidBytes: ByteArray = byteArrayOf(
        // Header: "WSDL" + version 1 + flags 0
        0x57, 0x53, 0x44, 0x4C, 0x01, 0x00,
        // VIEWPORT 0 0 24 24
        0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0xC0.toByte(), 0x41, 0x00, 0x00, 0xC0.toByte(), 0x41,
        // FILL_COLOR #FF0000FF (R=FF G=00 B=00 A=FF)
        0x10, 0xFF.toByte(), 0x00, 0x00, 0xFF.toByte(),
        // PATH_BEGIN
        0x20,
        // MOVE_TO 2 2
        0x21, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x40,
        // LINE_TO 22 2
        0x22, 0x00, 0x00, 0xB0.toByte(), 0x41, 0x00, 0x00, 0x00, 0x40,
        // LINE_TO 22 22
        0x22, 0x00, 0x00, 0xB0.toByte(), 0x41, 0x00, 0x00, 0xB0.toByte(), 0x41,
        // LINE_TO 2 22
        0x22, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0xB0.toByte(), 0x41,
        // CLOSE
        0x25,
        // FILL
        0x30,
        // END
        0xFF.toByte(),
    )

    private val rectSolidExpectedTrace = """
        VIEWPORT 0 0 24 24
        FILL_COLOR #FF0000FF
        PATH_BEGIN
        MOVE_TO 2 2
        LINE_TO 22 2
        LINE_TO 22 22
        LINE_TO 2 22
        CLOSE
        FILL
    """.trimIndent() + "\n"

    @Test
    fun rectSolidFixtureDecodesToMatchingTrace() {
        val visitor = DLTraceVisitor()
        dlReplay(rectSolidBytes, visitor)
        assertEquals(rectSolidExpectedTrace, visitor.asString())
    }
}
