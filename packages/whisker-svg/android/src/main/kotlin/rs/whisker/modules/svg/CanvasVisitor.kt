// `CanvasVisitor` — paints the display-list stream into an
// `android.graphics.Canvas`. Implements the same `DLVisitor`
// interface as the trace recorder, so the decoder byte loop is
// identical between unit tests and production drawing.
//
// Paint state model (mirrors `CGContextVisitor` on iOS — see
// SPEC.md §"Tint propagation"):
//
// * `FILL_COLOR` / `STROKE_COLOR` set a literal colour AND clear
//   the corresponding tint flag.
// * `FILL_TINT` / `STROKE_TINT` set the tint flag — the actual
//   paint substituted at fill/stroke time is the host's
//   `tintColor`.
// * `SAVE` / `RESTORE` push and pop both our paint flags AND
//   delegate to `Canvas.save()` / `Canvas.restore()` so the
//   Canvas's own CTM round-trips too.

package rs.whisker.modules.svg

import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PorterDuff

internal class CanvasVisitor(
    private val canvas: Canvas,
    private val tintArgb: Int,
    private val viewWidth: Float,
    private val viewHeight: Float,
) : DLVisitor {

    // Reusable paint objects so we don't allocate per fill/stroke.
    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val tmpMatrix = Matrix()

    private var currentFillArgb: Int? = 0xFF000000.toInt() // SVG shapes default to black fill.
    private var currentStrokeArgb: Int? = null
    private var fillIsTint = false
    private var strokeIsTint = false
    private var currentStrokeWidth = 1f
    private var currentOpacity = 1f

    private var currentPath: Path? = null

    /**
     * Stack pushed on SAVE / popped on RESTORE. Captures our paint
     * + tint flag state; the Canvas's own state (CTM / clip)
     * round-trips through Android's `Canvas.save/restore`.
     */
    private val savedPaintStates = ArrayDeque<PaintSnapshot>()

    private data class PaintSnapshot(
        val fill: Int?,
        val stroke: Int?,
        val fillIsTint: Boolean,
        val strokeIsTint: Boolean,
        val strokeWidth: Float,
        val opacity: Float,
    )

    // ---- container / state ------------------------------------------------

    override fun save() {
        canvas.save()
        savedPaintStates.addLast(
            PaintSnapshot(
                currentFillArgb,
                currentStrokeArgb,
                fillIsTint,
                strokeIsTint,
                currentStrokeWidth,
                currentOpacity,
            ),
        )
    }

    override fun restore() {
        canvas.restore()
        savedPaintStates.removeLastOrNull()?.let {
            currentFillArgb = it.fill
            currentStrokeArgb = it.stroke
            fillIsTint = it.fillIsTint
            strokeIsTint = it.strokeIsTint
            currentStrokeWidth = it.strokeWidth
            currentOpacity = it.opacity
        }
    }

    override fun concat(t: DLTransform) {
        tmpMatrix.setValues(t.toMatrixValues())
        canvas.concat(tmpMatrix)
    }

    override fun viewport(minX: Float, minY: Float, width: Float, height: Float) {
        // preserveAspectRatio="xMidYMid meet" — see SPEC.md
        // §"Viewport mapping". Emitted exactly once per stream
        // before any drawing op.
        if (width <= 0f || height <= 0f) return
        val scale = minOf(viewWidth / width, viewHeight / height)
        val tx = (viewWidth - width * scale) / 2f - minX * scale
        val ty = (viewHeight - height * scale) / 2f - minY * scale
        tmpMatrix.setValues(floatArrayOf(scale, 0f, tx, 0f, scale, ty, 0f, 0f, 1f))
        canvas.concat(tmpMatrix)
    }

    // ---- paint state ------------------------------------------------------

    override fun fillColor(c: DLColor) {
        currentFillArgb = c.toArgb()
        fillIsTint = false
    }

    override fun strokeColor(c: DLColor) {
        currentStrokeArgb = c.toArgb()
        strokeIsTint = false
    }

    override fun strokeWidth(w: Float) {
        currentStrokeWidth = w
    }

    override fun opacity(a: Float) {
        currentOpacity = a.coerceIn(0f, 1f)
    }

    override fun fillTint() {
        fillIsTint = true
    }

    override fun strokeTint() {
        strokeIsTint = true
    }

    // ---- path -------------------------------------------------------------

    override fun pathBegin() {
        currentPath = Path()
    }

    override fun moveTo(x: Float, y: Float) {
        ensurePath().moveTo(x, y)
    }

    override fun lineTo(x: Float, y: Float) {
        ensurePath().lineTo(x, y)
    }

    override fun quadTo(cx: Float, cy: Float, x: Float, y: Float) {
        ensurePath().quadTo(cx, cy, x, y)
    }

    override fun cubicTo(c1x: Float, c1y: Float, c2x: Float, c2y: Float, x: Float, y: Float) {
        ensurePath().cubicTo(c1x, c1y, c2x, c2y, x, y)
    }

    override fun close() {
        ensurePath().close()
    }

    override fun fill() {
        val path = currentPath ?: return
        val resolved = resolveFillColor() ?: return
        fillPaint.color = applyOpacity(resolved)
        canvas.drawPath(path, fillPaint)
    }

    override fun stroke() {
        val path = currentPath ?: return
        val resolved = resolveStrokeColor() ?: return
        strokePaint.color = applyOpacity(resolved)
        strokePaint.strokeWidth = currentStrokeWidth
        canvas.drawPath(path, strokePaint)
    }

    override fun fillAndStroke() {
        val path = currentPath ?: return
        resolveFillColor()?.let { argb ->
            fillPaint.color = applyOpacity(argb)
            canvas.drawPath(path, fillPaint)
        }
        resolveStrokeColor()?.let { argb ->
            strokePaint.color = applyOpacity(argb)
            strokePaint.strokeWidth = currentStrokeWidth
            canvas.drawPath(path, strokePaint)
        }
    }

    // ---- helpers ----------------------------------------------------------

    private fun ensurePath(): Path {
        var p = currentPath
        if (p == null) {
            p = Path()
            currentPath = p
        }
        return p
    }

    private fun resolveFillColor(): Int? =
        if (fillIsTint) tintArgb else currentFillArgb

    private fun resolveStrokeColor(): Int? =
        if (strokeIsTint) tintArgb else currentStrokeArgb

    /**
     * Multiply the colour's alpha channel by the current group
     * opacity. Avoids touching Canvas's own alpha (which is
     * captured by save/restore) so the SPEC's
     * "OPACITY multiplies into subsequent paints" semantics work
     * even without a SAVE/RESTORE wrapper. Discards the `PorterDuff`
     * import otherwise unused so lint is happy.
     */
    @Suppress("unused")
    private val _porterDuffUsage = PorterDuff.Mode.SRC

    private fun applyOpacity(argb: Int): Int {
        if (currentOpacity >= 1f) return argb
        val origAlpha = (argb ushr 24) and 0xFF
        val newAlpha = (origAlpha * currentOpacity).toInt().coerceIn(0, 255)
        return (argb and 0x00FFFFFF) or (newAlpha shl 24)
    }
}
