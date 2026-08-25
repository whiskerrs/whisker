package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.BlurMaskFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF

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
        if (!shadow.inset || shadow.blurRadius != 0f || shadow.spreadRadius != 0f) {
            return@forEach
        }
        val hole = RectF(paddingRect).apply { offset(shadow.offsetX, shadow.offsetY) }
        val ring = Path().apply {
            fillType = Path.FillType.EVEN_ODD
            addPath(paddingPath)
            addPath(roundedPath(hole, paddingRadii))
        }
        paint.color = shadow.color
        canvas.drawPath(ring, paint)
    }
    canvas.restoreToCount(save)
}
