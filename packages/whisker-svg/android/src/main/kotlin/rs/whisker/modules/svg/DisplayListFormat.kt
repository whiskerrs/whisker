// Wire-format constants — the literal Kotlin mirror of
// `packages/whisker-svg/SPEC.md`.
//
// Every value here MUST match the SPEC. The Rust producer in
// `packages/whisker-svg/src/format.rs` is the parallel
// authority; any change in either file without the matching
// counterpart edit (and SPEC bump) is a protocol break.

package rs.whisker.modules.svg

internal object DLFormat {
    /** Magic bytes (`"WSDL"` — 0x57 0x53 0x44 0x4C). */
    val MAGIC: ByteArray = byteArrayOf(0x57, 0x53, 0x44, 0x4C)

    /** Current wire-format version. */
    const val VERSION: Byte = 1

    /** `flags` byte — reserved in v1, MUST be zero. */
    const val FLAGS_RESERVED: Byte = 0

    /** Size of the 6-byte stream header (magic + version + flags). */
    const val HEADER_LEN: Int = 6

    // ---- container / state (0x00 – 0x0F) ---------------------------------

    const val OP_SAVE: Byte = 0x01
    const val OP_RESTORE: Byte = 0x02
    const val OP_CONCAT: Byte = 0x03
    const val OP_VIEWPORT: Byte = 0x04

    // ---- paint state (0x10 – 0x1F) ---------------------------------------

    const val OP_PAINT_FILL_COLOR: Byte = 0x10
    const val OP_PAINT_STROKE_COLOR: Byte = 0x11
    const val OP_PAINT_STROKE_WIDTH: Byte = 0x12
    const val OP_PAINT_OPACITY: Byte = 0x13
    const val OP_PAINT_FILL_TINT: Byte = 0x14
    const val OP_PAINT_STROKE_TINT: Byte = 0x15

    // ---- path commands (0x20 – 0x2F) -------------------------------------

    const val OP_PATH_BEGIN: Byte = 0x20
    const val OP_PATH_MOVE_TO: Byte = 0x21
    const val OP_PATH_LINE_TO: Byte = 0x22
    const val OP_PATH_QUAD_TO: Byte = 0x23
    const val OP_PATH_CUBIC_TO: Byte = 0x24
    const val OP_PATH_CLOSE: Byte = 0x25

    // ---- path execution (0x30 – 0x3F) ------------------------------------

    const val OP_PATH_FILL: Byte = 0x30
    const val OP_PATH_STROKE: Byte = 0x32
    const val OP_PATH_FILL_AND_STROKE: Byte = 0x33

    // ---- end marker ------------------------------------------------------

    const val OP_END: Byte = 0xFF.toByte()

    /**
     * `true` if the opcode falls in any reserved-for-future-use
     * range. A v1 replayer MUST reject these (SPEC §"Opcode space")
     * so a v2 stream silently degrades to a clear error rather
     * than partial draw.
     */
    fun isReserved(op: Int): Boolean {
        val u = op and 0xFF
        return u == 0x31 ||
            (u in 0x40..0x4F) || // gradients
            (u in 0x50..0x5F) || // clipping
            (u in 0x60..0x6F) || // masking
            (u in 0x70..0x7F) || // text
            (u in 0x80..0x8F) || // images
            (u in 0x90..0xEF) || // future
            (u in 0xF0..0xFE)    // markers / control
    }
}

/**
 * 32-bit RGBA colour matching the wire byte order. Components are
 * 0..255 (Int representations of unsigned bytes), straight-alpha.
 */
internal data class DLColor(val r: Int, val g: Int, val b: Int, val a: Int) {
    /** Packed Android colour int (ARGB) for direct
     *  `Paint.setColor(this.toArgb())` use. */
    fun toArgb(): Int =
        (a and 0xFF shl 24) or (r and 0xFF shl 16) or (g and 0xFF shl 8) or (b and 0xFF)
}

/**
 * 2 × 3 affine transform in `[a, b, c, d, tx, ty]` column-major
 * CoreGraphics / Android `Matrix.setValues` convention. (Android
 * uses row-major in its public API, but `setValues` expects a
 * `FloatArray` ordered as `[scaleX, skewX, transX, skewY, scaleY,
 * transY, persp0, persp1, persp2]` — the [`toMatrixValues`] helper
 * adapts our column-major struct to that layout.)
 */
internal data class DLTransform(
    val a: Float,
    val b: Float,
    val c: Float,
    val d: Float,
    val tx: Float,
    val ty: Float,
) {
    /** Convert to a 9-element `FloatArray` for `Matrix.setValues`. */
    fun toMatrixValues(): FloatArray = floatArrayOf(
        a, c, tx,
        b, d, ty,
        0f, 0f, 1f,
    )

    companion object {
        val IDENTITY = DLTransform(1f, 0f, 0f, 1f, 0f, 0f)
    }
}
