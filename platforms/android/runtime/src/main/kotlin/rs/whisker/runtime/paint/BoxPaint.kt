package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.view.View
import kotlin.math.min

/** Packed Android projection of one protocol `SetBoxPaint` payload. */
internal data class HostBoxPaint(val values: FloatArray, val names: Array<String>)

/** Resolved physical geometry shared by box paint and overflow clipping. */
internal data class ResolvedBoxGeometry(
    val width: Float,
    val height: Float,
    val borderWidths: FloatArray,
    val cornerRadii: FloatArray,
)

/** Applies renderer-independent box paint to the common Host node wrapper. */
internal fun applyBoxPaint(
    node: View,
    paint: HostBoxPaint,
    logicalWidth: Float,
    logicalHeight: Float,
    density: Float,
    backgroundLayers: HostBackgroundLayers? = null,
    imageRendering: HostImageRendering = HostImageRendering.Auto,
    logicalContentBox: RectF = RectF(0f, 0f, logicalWidth, logicalHeight),
): ResolvedBoxGeometry {
    val values = paint.values
    require(values.size >= BOX_PAINT_PACKED_SIZE)
    val background = if (values[0] == 0f) {
        parseNamedColor(paint.names[0])
    } else {
        rgba(values[1], values[2], values[3], values[4])
    }
    val borderWidths = floatArrayOf(
        resolveLength(values[5], values[6], logicalHeight) * density,
        resolveLength(values[7], values[8], logicalWidth) * density,
        resolveLength(values[9], values[10], logicalHeight) * density,
        resolveLength(values[11], values[12], logicalWidth) * density,
    )
    val borderColors = IntArray(4) { index ->
        val offset = 13 + index * 5
        if (values[offset] == 0f) {
            parseNamedColor(paint.names[index + 1])
        } else {
            rgba(values[offset + 1], values[offset + 2], values[offset + 3], values[offset + 4])
        }
    }
    val radii = FloatArray(8) { index ->
        val corner = index / 2
        val horizontal = index % 2 == 0
        val offset = (if (horizontal) RADII_HORIZONTAL_OFFSET else RADII_VERTICAL_OFFSET) + corner * 2
        val axis = if (horizontal) logicalWidth else logicalHeight
        resolveLength(values[offset], values[offset + 1], axis) * density
    }
    val physicalWidth = logicalWidth * density
    val physicalHeight = logicalHeight * density
    val normalizedRadii = normalizeRadii(radii, physicalWidth, physicalHeight)
    val borderStyles = IntArray(4) { index -> values[BORDER_STYLES_OFFSET + index].toInt() }
    val uniformSolidBorder = backgroundLayers == null &&
        borderStyles.all { it == BORDER_STYLE_SOLID } &&
        borderWidths.all { it == borderWidths[0] } &&
        borderColors.all { it == borderColors[0] }
    node.background = if (uniformSolidBorder) {
        GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            setColor(background)
            if (borderWidths[0] > 0f) {
                setStroke(borderWidths[0].toInt().coerceAtLeast(1), borderColors[0])
            }
            cornerRadii = normalizedRadii
        }
    } else {
        WhiskerBoxDrawable(
            background,
            borderWidths,
            borderColors,
            borderStyles,
            normalizedRadii,
            physicalWidth,
            physicalHeight,
            backgroundLayers,
            imageRendering,
            RectF(
                logicalContentBox.left * density,
                logicalContentBox.top * density,
                logicalContentBox.right * density,
                logicalContentBox.bottom * density,
            ),
        )
    }
    return ResolvedBoxGeometry(
        width = physicalWidth,
        height = physicalHeight,
        borderWidths = borderWidths,
        cornerRadii = normalizedRadii,
    )
}

private fun resolveLength(length: Float, fraction: Float, axis: Float): Float =
    length + fraction * axis

/** Draws the CSS box model without relying on Android's uniform stroke. */
private class WhiskerBoxDrawable(
    private val fillColor: Int,
    private val borderWidths: FloatArray,
    private val borderColors: IntArray,
    private val borderStyles: IntArray,
    private val cornerRadii: FloatArray,
    private val backgroundBoxWidth: Float,
    private val backgroundBoxHeight: Float,
    private val backgroundLayers: HostBackgroundLayers?,
    private val imageRendering: HostImageRendering,
    private val localContentBox: RectF,
) : Drawable() {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }

    override fun draw(canvas: Canvas) {
        val box = RectF(bounds)
        if (box.isEmpty) return
        val radii = normalizeRadii(cornerRadii, box.width(), box.height())
        val outer = roundedPath(box, radii)
        val top = borderWidths[0].coerceIn(0f, box.height())
        val right = borderWidths[1].coerceIn(0f, box.width())
        val bottom = borderWidths[2].coerceIn(0f, box.height())
        val left = borderWidths[3].coerceIn(0f, box.width())
        val paddingBox = RectF(
            box.left + left,
            box.top + top,
            box.right - right,
            box.bottom - bottom,
        )
        val backgroundBorderBox = RectF(
            box.left,
            box.top,
            box.left + backgroundBoxWidth,
            box.top + backgroundBoxHeight,
        )
        val backgroundPaddingBox = RectF(
            backgroundBorderBox.left + left,
            backgroundBorderBox.top + top,
            backgroundBorderBox.right - right,
            backgroundBorderBox.bottom - bottom,
        )
        val paddingPath = if (paddingBox.width() > 0f && paddingBox.height() > 0f) {
            roundedPath(
                paddingBox,
                floatArrayOf(
                    (radii[0] - left).coerceAtLeast(0f),
                    (radii[1] - top).coerceAtLeast(0f),
                    (radii[2] - right).coerceAtLeast(0f),
                    (radii[3] - top).coerceAtLeast(0f),
                    (radii[4] - right).coerceAtLeast(0f),
                    (radii[5] - bottom).coerceAtLeast(0f),
                    (radii[6] - left).coerceAtLeast(0f),
                    (radii[7] - bottom).coerceAtLeast(0f),
                ),
            )
        } else {
            Path()
        }
        val contentBox = RectF(
            box.left + localContentBox.left,
            box.top + localContentBox.top,
            box.left + localContentBox.right,
            box.top + localContentBox.bottom,
        )
        val contentPath = if (contentBox.width() > 0f && contentBox.height() > 0f) {
            val contentLeft = (contentBox.left - box.left).coerceAtLeast(0f)
            val contentTop = (contentBox.top - box.top).coerceAtLeast(0f)
            val contentRight = (box.right - contentBox.right).coerceAtLeast(0f)
            val contentBottom = (box.bottom - contentBox.bottom).coerceAtLeast(0f)
            val contentRadii = normalizeRadii(
                floatArrayOf(
                    (radii[0] - contentLeft).coerceAtLeast(0f),
                    (radii[1] - contentTop).coerceAtLeast(0f),
                    (radii[2] - contentRight).coerceAtLeast(0f),
                    (radii[3] - contentTop).coerceAtLeast(0f),
                    (radii[4] - contentRight).coerceAtLeast(0f),
                    (radii[5] - contentBottom).coerceAtLeast(0f),
                    (radii[6] - contentLeft).coerceAtLeast(0f),
                    (radii[7] - contentBottom).coerceAtLeast(0f),
                ),
                contentBox.width(),
                contentBox.height(),
            )
            roundedPath(contentBox, contentRadii)
        } else {
            Path()
        }
        paint.color = fillColor
        canvas.drawPath(outer, paint)
        drawBackgroundLayers(
            canvas,
            HostBackgroundPaintBoxes(
                border = HostBackgroundPaintBox(backgroundBorderBox, outer),
                padding = HostBackgroundPaintBox(backgroundPaddingBox, paddingPath),
                content = HostBackgroundPaintBox(contentBox, contentPath),
                borderArea = HostBackgroundPaintBox(
                    backgroundBorderBox,
                    Path(outer).apply {
                        if (!paddingPath.isEmpty) op(paddingPath, Path.Op.DIFFERENCE)
                    },
                ),
            ),
            backgroundLayers,
            paint,
            imageRendering,
        )

        val widths = floatArrayOf(top, right, bottom, left)
        if (top == 0f && right == 0f && bottom == 0f && left == 0f) return

        if (radii.all { it == 0f }) {
            drawRectangularEdges(canvas, box, top, right, bottom, left)
            return
        }

        val inner = paddingBox
        val innerPath = if (paddingPath.isEmpty) null else paddingPath
        val sidePaths = arrayOf(
            Path().apply {
                moveTo(box.left, box.top)
                lineTo(box.right, box.top)
                lineTo(inner.right, inner.top)
                lineTo(inner.left, inner.top)
                close()
            },
            Path().apply {
                moveTo(box.right, box.top)
                lineTo(box.right, box.bottom)
                lineTo(inner.right, inner.bottom)
                lineTo(inner.right, inner.top)
                close()
            },
            Path().apply {
                moveTo(box.right, box.bottom)
                lineTo(box.left, box.bottom)
                lineTo(inner.left, inner.bottom)
                lineTo(inner.right, inner.bottom)
                close()
            },
            Path().apply {
                moveTo(box.left, box.bottom)
                lineTo(box.left, box.top)
                lineTo(inner.left, inner.top)
                lineTo(inner.left, inner.bottom)
                close()
            },
        )
        repeat(4) { side ->
            if (!paintsSide(side) || borderWidths[side] <= 0f) return@repeat
            val save = canvas.save()
            @Suppress("DEPRECATION")
            canvas.clipPath(outer)
            if (innerPath != null) {
                @Suppress("DEPRECATION")
                canvas.clipPath(innerPath, android.graphics.Region.Op.DIFFERENCE)
            }
            paint.color = borderColors[side]
            when (borderStyles[side]) {
                BORDER_STYLE_SOLID -> canvas.drawPath(sidePaths[side], paint)
                BORDER_STYLE_DASHED, BORDER_STYLE_DOTTED, BORDER_STYLE_DOUBLE -> {
                    canvas.clipPath(sidePaths[side])
                    drawPatternedEdge(
                        canvas,
                        edgeRect(box, side, widths[side]),
                        side,
                        borderWidths[side],
                        borderStyles[side],
                    )
                }
                BORDER_STYLE_GROOVE, BORDER_STYLE_RIDGE,
                BORDER_STYLE_INSET, BORDER_STYLE_OUTSET -> {
                    canvas.clipPath(sidePaths[side])
                    drawReliefEdge(
                        canvas,
                        edgeRect(box, side, widths[side]),
                        side,
                        borderStyles[side],
                        borderColors[side],
                    )
                }
            }
            canvas.restoreToCount(save)
        }
    }

    private fun drawRectangularEdges(
        canvas: Canvas,
        box: RectF,
        top: Float,
        right: Float,
        bottom: Float,
        left: Float,
    ) {
        val widths = floatArrayOf(top, right, bottom, left)
        val edges = arrayOf(
            RectF(box.left, box.top, box.right, box.top + top),
            RectF(box.right - right, box.top, box.right, box.bottom),
            RectF(box.left, box.bottom - bottom, box.right, box.bottom),
            RectF(box.left, box.top, box.left + left, box.bottom),
        )
        repeat(4) { side ->
            if (!paintsSide(side) || widths[side] <= 0f) return@repeat
            paint.color = borderColors[side]
            when (borderStyles[side]) {
                BORDER_STYLE_SOLID -> canvas.drawRect(edges[side], paint)
                BORDER_STYLE_DASHED, BORDER_STYLE_DOTTED, BORDER_STYLE_DOUBLE ->
                    drawPatternedEdge(canvas, edges[side], side, widths[side], borderStyles[side])
                BORDER_STYLE_GROOVE, BORDER_STYLE_RIDGE,
                BORDER_STYLE_INSET, BORDER_STYLE_OUTSET ->
                    drawReliefEdge(
                        canvas,
                        edges[side],
                        side,
                        borderStyles[side],
                        borderColors[side],
                    )
            }
        }
    }

    private fun paintsSide(side: Int): Boolean = when (borderStyles[side]) {
        BORDER_STYLE_SOLID, BORDER_STYLE_DASHED, BORDER_STYLE_DOTTED, BORDER_STYLE_DOUBLE,
        BORDER_STYLE_GROOVE, BORDER_STYLE_RIDGE, BORDER_STYLE_INSET, BORDER_STYLE_OUTSET -> true
        else -> false
    }

    private fun edgeRect(box: RectF, side: Int, width: Float): RectF = when (side) {
        0 -> RectF(box.left, box.top, box.right, box.top + width)
        1 -> RectF(box.right - width, box.top, box.right, box.bottom)
        2 -> RectF(box.left, box.bottom - width, box.right, box.bottom)
        else -> RectF(box.left, box.top, box.left + width, box.bottom)
    }

    private fun drawPatternedEdge(
        canvas: Canvas,
        edge: RectF,
        side: Int,
        width: Float,
        style: Int,
    ) {
        if (width <= 0f || edge.isEmpty) return
        if (style == BORDER_STYLE_DOUBLE) {
            drawDoubleEdge(canvas, edge, side, width)
            return
        }
        val horizontal = side == 0 || side == 2
        val start = if (horizontal) edge.left else edge.top
        val end = if (horizontal) edge.right else edge.bottom
        val center = if (horizontal) edge.centerY() else edge.centerX()
        val save = canvas.save()
        canvas.clipRect(edge)
        if (style == BORDER_STYLE_DASHED) {
            val dash = width * 3f
            val period = width * 4f
            var position = start
            while (position < end) {
                val dashEnd = min(position + dash, end)
                if (horizontal) {
                    canvas.drawRect(position, edge.top, dashEnd, edge.bottom, paint)
                } else {
                    canvas.drawRect(edge.left, position, edge.right, dashEnd, paint)
                }
                position += period
            }
        } else {
            val radius = width / 2f
            var position = start + width
            while (position - radius < end) {
                if (horizontal) {
                    canvas.drawCircle(position, center, radius, paint)
                } else {
                    canvas.drawCircle(center, position, radius, paint)
                }
                position += width * 2f
            }
        }
        canvas.restoreToCount(save)
    }

    private fun drawDoubleEdge(
        canvas: Canvas,
        edge: RectF,
        side: Int,
        width: Float,
    ) {
        val band = width / 3f
        val outer: RectF
        val inner: RectF
        when (side) {
            0 -> {
                outer = RectF(edge.left, edge.top, edge.right, edge.top + band)
                inner = RectF(edge.left, edge.bottom - band, edge.right, edge.bottom)
            }
            1 -> {
                outer = RectF(edge.right - band, edge.top, edge.right, edge.bottom)
                inner = RectF(edge.left, edge.top, edge.left + band, edge.bottom)
            }
            2 -> {
                outer = RectF(edge.left, edge.bottom - band, edge.right, edge.bottom)
                inner = RectF(edge.left, edge.top, edge.right, edge.top + band)
            }
            else -> {
                outer = RectF(edge.left, edge.top, edge.left + band, edge.bottom)
                inner = RectF(edge.right - band, edge.top, edge.right, edge.bottom)
            }
        }
        canvas.drawRect(outer, paint)
        canvas.drawRect(inner, paint)
    }

    private fun drawReliefEdge(
        canvas: Canvas,
        edge: RectF,
        side: Int,
        style: Int,
        color: Int,
    ) {
        if (edge.isEmpty) return
        val topOrLeft = side == 0 || side == 3
        when (style) {
            BORDER_STYLE_INSET -> {
                paint.color = shadedColor(color, lighter = !topOrLeft)
                canvas.drawRect(edge, paint)
            }
            BORDER_STYLE_OUTSET -> {
                paint.color = shadedColor(color, lighter = topOrLeft)
                canvas.drawRect(edge, paint)
            }
            BORDER_STYLE_GROOVE, BORDER_STYLE_RIDGE -> {
                val (outer, inner) = splitEdge(edge, side)
                val outerIsLighter = if (style == BORDER_STYLE_GROOVE) !topOrLeft else topOrLeft
                paint.color = shadedColor(color, lighter = outerIsLighter)
                canvas.drawRect(outer, paint)
                paint.color = shadedColor(color, lighter = !outerIsLighter)
                canvas.drawRect(inner, paint)
            }
        }
    }

    /** Splits a border across its depth, preserving CSS outer-to-inner order. */
    private fun splitEdge(edge: RectF, side: Int): Pair<RectF, RectF> = when (side) {
        0 -> {
            val middle = edge.centerY()
            RectF(edge.left, edge.top, edge.right, middle) to
                RectF(edge.left, middle, edge.right, edge.bottom)
        }
        1 -> {
            val middle = edge.centerX()
            RectF(middle, edge.top, edge.right, edge.bottom) to
                RectF(edge.left, edge.top, middle, edge.bottom)
        }
        2 -> {
            val middle = edge.centerY()
            RectF(edge.left, middle, edge.right, edge.bottom) to
                RectF(edge.left, edge.top, edge.right, middle)
        }
        else -> {
            val middle = edge.centerX()
            RectF(edge.left, edge.top, middle, edge.bottom) to
                RectF(middle, edge.top, edge.right, edge.bottom)
        }
    }

    private fun shadedColor(color: Int, lighter: Boolean): Int {
        fun shade(channel: Int): Int = if (lighter) {
            channel + ((255 - channel) * RELIEF_SHADE_FACTOR).toInt()
        } else {
            (channel * (1f - RELIEF_SHADE_FACTOR)).toInt()
        }
        return Color.argb(
            Color.alpha(color),
            shade(Color.red(color)),
            shade(Color.green(color)),
            shade(Color.blue(color)),
        )
    }

    @Deprecated("Deprecated in Java")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT

    override fun setAlpha(alpha: Int) {
        paint.alpha = alpha
    }

    override fun setColorFilter(colorFilter: ColorFilter?) {
        paint.colorFilter = colorFilter
    }
}

internal fun roundedPath(rect: RectF, radii: FloatArray): Path = Path().apply {
    addRoundRect(rect, radii, Path.Direction.CW)
}

internal fun normalizeRadii(radii: FloatArray, width: Float, height: Float): FloatArray {
    val result = radii.map { it.coerceAtLeast(0f) }.toFloatArray()
    val denominators = floatArrayOf(
        result[0] + result[2],
        result[6] + result[4],
        result[1] + result[7],
        result[3] + result[5],
    )
    val limits = floatArrayOf(width, width, height, height)
    var scale = 1f
    repeat(4) { index ->
        if (denominators[index] > 0f) scale = min(scale, limits[index] / denominators[index])
    }
    if (scale < 1f) repeat(result.size) { result[it] *= scale }
    return result
}

private const val BORDER_STYLE_SOLID = 2
private const val BORDER_STYLE_DASHED = 3
private const val BORDER_STYLE_DOTTED = 4
private const val BORDER_STYLE_DOUBLE = 5
private const val BORDER_STYLE_GROOVE = 6
private const val BORDER_STYLE_RIDGE = 7
private const val BORDER_STYLE_INSET = 8
private const val BORDER_STYLE_OUTSET = 9
private const val RELIEF_SHADE_FACTOR = 0.4f
private const val RADII_HORIZONTAL_OFFSET = 33
private const val RADII_VERTICAL_OFFSET = 41
private const val BORDER_STYLES_OFFSET = 49
private const val BOX_PAINT_PACKED_SIZE = 53
