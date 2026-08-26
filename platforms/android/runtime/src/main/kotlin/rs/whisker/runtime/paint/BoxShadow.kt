package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.BlurMaskFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import kotlin.math.abs
import kotlin.math.max

internal data class HostBoxShadow(
    val offsetX: Float,
    val offsetY: Float,
    val blurRadius: Float,
    val spreadRadius: Float,
    val color: Int,
    val inset: Boolean,
)

internal fun drawOuterBoxShadows(
    canvas: Canvas,
    geometry: ResolvedBoxGeometry?,
    shadows: List<HostBoxShadow>,
) {
    val box = geometry ?: return
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    shadows.asReversed().forEach { shadow ->
        if (shadow.inset) return@forEach
        val spread = shadow.spreadRadius
        val rect = RectF(
            shadow.offsetX - spread,
            shadow.offsetY - spread,
            box.width + shadow.offsetX + spread,
            box.height + shadow.offsetY + spread,
        )
        if (rect.isEmpty) return@forEach
        val radii = normalizeRadii(
            box.cornerRadii.map { (it + spread).coerceAtLeast(0f) }.toFloatArray(),
            rect.width(),
            rect.height(),
        )
        paint.color = shadow.color
        paint.maskFilter = if (shadow.blurRadius > 0f) {
            BlurMaskFilter(shadow.blurRadius / 2f, BlurMaskFilter.Blur.NORMAL)
        } else {
            null
        }
        canvas.drawPath(roundedPath(rect, radii), paint)
    }
    paint.maskFilter = null
}

internal fun drawInsetBoxShadows(
    canvas: Canvas,
    geometry: ResolvedBoxGeometry?,
    shadows: List<HostBoxShadow>,
) {
    val box = geometry ?: return
    val top = box.borderWidths[0].coerceIn(0f, box.height)
    val right = box.borderWidths[1].coerceIn(0f, box.width)
    val bottom = box.borderWidths[2].coerceIn(0f, box.height)
    val left = box.borderWidths[3].coerceIn(0f, box.width)
    val paddingRect = RectF(left, top, box.width - right, box.height - bottom)
    if (paddingRect.isEmpty) return
    val outer = box.cornerRadii
    val paddingRadii = normalizeRadii(
        floatArrayOf(
            (outer[0] - left).coerceAtLeast(0f),
            (outer[1] - top).coerceAtLeast(0f),
            (outer[2] - right).coerceAtLeast(0f),
            (outer[3] - top).coerceAtLeast(0f),
            (outer[4] - right).coerceAtLeast(0f),
            (outer[5] - bottom).coerceAtLeast(0f),
            (outer[6] - left).coerceAtLeast(0f),
            (outer[7] - bottom).coerceAtLeast(0f),
        ),
        paddingRect.width(),
        paddingRect.height(),
    )
    val paddingPath = roundedPath(paddingRect, paddingRadii)
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    val save = canvas.save()
    canvas.clipPath(paddingPath)
    shadows.asReversed().forEach { shadow ->
        if (!shadow.inset) return@forEach
        val spread = shadow.spreadRadius
        val hole = RectF(paddingRect).apply {
            inset(spread, spread)
            offset(shadow.offsetX, shadow.offsetY)
        }
        val extent = max(box.width, box.height) + abs(shadow.offsetX) +
            abs(shadow.offsetY) + abs(spread) + shadow.blurRadius * 2f
        val exterior = RectF(paddingRect).apply { inset(-extent, -extent) }
        val ring = Path().apply {
            fillType = Path.FillType.EVEN_ODD
            addRect(exterior, Path.Direction.CW)
            if (!hole.isEmpty) {
                val holeRadii = normalizeRadii(
                    paddingRadii.map { (it - spread).coerceAtLeast(0f) }.toFloatArray(),
                    hole.width(),
                    hole.height(),
                )
                addPath(roundedPath(hole, holeRadii))
            }
        }
        paint.color = shadow.color
        paint.maskFilter = if (shadow.blurRadius > 0f) {
            BlurMaskFilter(shadow.blurRadius / 2f, BlurMaskFilter.Blur.NORMAL)
        } else {
            null
        }
        canvas.drawPath(ring, paint)
    }
    paint.maskFilter = null
    canvas.restoreToCount(save)
}
