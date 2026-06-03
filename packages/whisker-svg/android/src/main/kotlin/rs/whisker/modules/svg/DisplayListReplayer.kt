// Decoder — walks a byte stream conforming to
// `packages/whisker-svg/SPEC.md` and dispatches each opcode to a
// [DLVisitor].
//
// The same `dlReplay` function powers `DisplayListReplayerTests`
// (with a `DLTraceVisitor` that records every dispatched call as a
// text line) and `WhiskerSvgView` (with `CanvasVisitor` that
// actually paints to an `android.graphics.Canvas`). Both visitors
// implement the same interface; neither knows about the other.

package rs.whisker.modules.svg

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Error surface for a malformed or unsupported display-list stream. */
internal sealed class DLReplayError(message: String) : Exception(message) {
    object BadMagic : DLReplayError("bad magic")
    data class UnsupportedVersion(val version: Int) : DLReplayError("unsupported version $version")
    data class UnsupportedFlags(val flags: Int) : DLReplayError("unsupported flags $flags")
    object HeaderTooShort : DLReplayError("header too short")
    object Truncated : DLReplayError("truncated")
    data class UnsupportedOpcode(val op: Int) : DLReplayError("unsupported opcode 0x%02X".format(op))
    data class UnknownOpcode(val op: Int) : DLReplayError("unknown opcode 0x%02X".format(op))
}

/**
 * Sink for decoded opcodes. Every method has a default no-op
 * implementation so partial visitors only override what they
 * care about — matches the `Visitor` trait on the Rust side
 * `crates/whisker-svg-core/src/replay.rs`.
 */
internal interface DLVisitor {
    fun save() {}
    fun restore() {}
    fun concat(t: DLTransform) {}
    fun viewport(minX: Float, minY: Float, width: Float, height: Float) {}
    fun fillColor(c: DLColor) {}
    fun strokeColor(c: DLColor) {}
    fun strokeWidth(w: Float) {}
    fun opacity(a: Float) {}
    fun fillTint() {}
    fun strokeTint() {}
    fun pathBegin() {}
    fun moveTo(x: Float, y: Float) {}
    fun lineTo(x: Float, y: Float) {}
    fun quadTo(cx: Float, cy: Float, x: Float, y: Float) {}
    fun cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float) {}
    fun close() {}
    fun fill() {}
    fun stroke() {}
    fun fillAndStroke() {}
}

/**
 * Walk `bytes` and dispatch each opcode to `visitor`. Returns
 * when the stream's `OP_END` is reached, or throws on the first
 * malformed / unsupported byte.
 */
internal fun dlReplay(bytes: ByteArray, visitor: DLVisitor) {
    if (bytes.size < DLFormat.HEADER_LEN) throw DLReplayError.HeaderTooShort
    for (i in 0..3) {
        if (bytes[i] != DLFormat.MAGIC[i]) throw DLReplayError.BadMagic
    }
    val version = bytes[4].toInt() and 0xFF
    if (version > (DLFormat.VERSION.toInt() and 0xFF)) {
        throw DLReplayError.UnsupportedVersion(version)
    }
    val flags = bytes[5].toInt() and 0xFF
    if (flags != (DLFormat.FLAGS_RESERVED.toInt() and 0xFF)) {
        throw DLReplayError.UnsupportedFlags(flags)
    }

    val cursor = Cursor(bytes, DLFormat.HEADER_LEN)

    while (true) {
        val op = cursor.readU8()
        when (op) {
            DLFormat.OP_END -> return
            DLFormat.OP_SAVE -> visitor.save()
            DLFormat.OP_RESTORE -> visitor.restore()
            DLFormat.OP_CONCAT -> visitor.concat(
                DLTransform(
                    cursor.readF32(), cursor.readF32(),
                    cursor.readF32(), cursor.readF32(),
                    cursor.readF32(), cursor.readF32(),
                ),
            )
            DLFormat.OP_VIEWPORT -> visitor.viewport(
                cursor.readF32(), cursor.readF32(),
                cursor.readF32(), cursor.readF32(),
            )
            DLFormat.OP_PAINT_FILL_COLOR -> visitor.fillColor(cursor.readColor())
            DLFormat.OP_PAINT_STROKE_COLOR -> visitor.strokeColor(cursor.readColor())
            DLFormat.OP_PAINT_STROKE_WIDTH -> visitor.strokeWidth(cursor.readF32())
            DLFormat.OP_PAINT_OPACITY -> visitor.opacity(cursor.readF32())
            DLFormat.OP_PAINT_FILL_TINT -> visitor.fillTint()
            DLFormat.OP_PAINT_STROKE_TINT -> visitor.strokeTint()
            DLFormat.OP_PATH_BEGIN -> visitor.pathBegin()
            DLFormat.OP_PATH_MOVE_TO -> visitor.moveTo(cursor.readF32(), cursor.readF32())
            DLFormat.OP_PATH_LINE_TO -> visitor.lineTo(cursor.readF32(), cursor.readF32())
            DLFormat.OP_PATH_QUAD_TO -> visitor.quadTo(
                cursor.readF32(), cursor.readF32(),
                cursor.readF32(), cursor.readF32(),
            )
            DLFormat.OP_PATH_CUBIC_TO -> visitor.cubicTo(
                cursor.readF32(), cursor.readF32(),
                cursor.readF32(), cursor.readF32(),
                cursor.readF32(), cursor.readF32(),
            )
            DLFormat.OP_PATH_CLOSE -> visitor.close()
            DLFormat.OP_PATH_FILL -> visitor.fill()
            DLFormat.OP_PATH_STROKE -> visitor.stroke()
            DLFormat.OP_PATH_FILL_AND_STROKE -> visitor.fillAndStroke()
            else -> {
                val unsigned = op.toInt() and 0xFF
                if (DLFormat.isReserved(unsigned)) {
                    throw DLReplayError.UnsupportedOpcode(unsigned)
                } else {
                    throw DLReplayError.UnknownOpcode(unsigned)
                }
            }
        }
    }
}

/** Internal byte cursor — fans out checked little-endian reads. */
private class Cursor(val bytes: ByteArray, var pos: Int) {
    fun readU8(): Byte {
        if (pos >= bytes.size) throw DLReplayError.Truncated
        return bytes[pos++]
    }

    fun readF32(): Float {
        if (pos + 4 > bytes.size) throw DLReplayError.Truncated
        val v = ByteBuffer.wrap(bytes, pos, 4).order(ByteOrder.LITTLE_ENDIAN).float
        pos += 4
        return v
    }

    fun readColor(): DLColor {
        if (pos + 4 > bytes.size) throw DLReplayError.Truncated
        val c = DLColor(
            bytes[pos].toInt() and 0xFF,
            bytes[pos + 1].toInt() and 0xFF,
            bytes[pos + 2].toInt() and 0xFF,
            bytes[pos + 3].toInt() and 0xFF,
        )
        pos += 4
        return c
    }
}

// ---- trace recorder --------------------------------------------------------

/**
 * `DLVisitor` that records every dispatched call as a text line.
 * Used by `DisplayListReplayerTests` to assert the Kotlin replayer
 * produces the same trace as the Rust `TraceVisitor` does for
 * identical input bytes — the cross-platform conformance check.
 *
 * Line format mirrors `crates/whisker-svg-core/src/replay.rs`
 * `TraceVisitor` exactly, so the produced trace string is
 * byte-for-byte the matching `*.trace.txt` from
 * `crates/whisker-svg-core/tests/fixtures/`.
 */
internal class DLTraceVisitor : DLVisitor {
    val lines: MutableList<String> = mutableListOf()

    override fun save() { lines.add("SAVE") }
    override fun restore() { lines.add("RESTORE") }
    override fun concat(t: DLTransform) {
        lines.add("CONCAT [${fmtF(t.a)} ${fmtF(t.b)} ${fmtF(t.c)} ${fmtF(t.d)} ${fmtF(t.tx)} ${fmtF(t.ty)}]")
    }
    override fun viewport(minX: Float, minY: Float, width: Float, height: Float) {
        lines.add("VIEWPORT ${fmtF(minX)} ${fmtF(minY)} ${fmtF(width)} ${fmtF(height)}")
    }
    override fun fillColor(c: DLColor) {
        lines.add("FILL_COLOR #%02X%02X%02X%02X".format(c.r, c.g, c.b, c.a))
    }
    override fun strokeColor(c: DLColor) {
        lines.add("STROKE_COLOR #%02X%02X%02X%02X".format(c.r, c.g, c.b, c.a))
    }
    override fun strokeWidth(w: Float) { lines.add("STROKE_WIDTH ${fmtF(w)}") }
    override fun opacity(a: Float) { lines.add("OPACITY ${fmtF(a)}") }
    override fun fillTint() { lines.add("FILL_TINT") }
    override fun strokeTint() { lines.add("STROKE_TINT") }
    override fun pathBegin() { lines.add("PATH_BEGIN") }
    override fun moveTo(x: Float, y: Float) { lines.add("MOVE_TO ${fmtF(x)} ${fmtF(y)}") }
    override fun lineTo(x: Float, y: Float) { lines.add("LINE_TO ${fmtF(x)} ${fmtF(y)}") }
    override fun quadTo(cx: Float, cy: Float, x: Float, y: Float) {
        lines.add("QUAD_TO ${fmtF(cx)} ${fmtF(cy)} ${fmtF(x)} ${fmtF(y)}")
    }
    override fun cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float) {
        lines.add("CUBIC_TO ${fmtF(c1x)} ${fmtF(c1y)} ${fmtF(c2x)} ${fmtF(c2y)} ${fmtF(x)} ${fmtF(y)}")
    }
    override fun close() { lines.add("CLOSE") }
    override fun fill() { lines.add("FILL") }
    override fun stroke() { lines.add("STROKE") }
    override fun fillAndStroke() { lines.add("FILL_AND_STROKE") }

    fun asString(): String = lines.joinToString(separator = "\n") + "\n"
}

/**
 * Compact float formatter that matches the Rust `fmt_f` in
 * `replay.rs`: drops a trailing `.0` so `42.0` prints as `42`,
 * making `*.trace.txt` golden files diff-friendly. Negative-zero
 * normalises to `0` to match Rust's display.
 */
private fun fmtF(v: Float): String {
    if (v.isFinite() && v % 1.0f == 0.0f) {
        val i = v.toLong()
        return i.toString()
    }
    // Match Rust's `{}` float formatter — minimum-digit
    // round-trippable. Kotlin's `Float.toString()` gives that.
    return v.toString()
}
