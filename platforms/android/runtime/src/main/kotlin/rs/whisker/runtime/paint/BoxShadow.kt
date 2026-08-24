package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF

internal data class HostBoxShadow(
    val offsetX: Float,
    val offsetY: Float,
    val blurRadius: Float,
    val spreadRadius: Float,
    val color: Int,
    val inset: Boolean,
)

internal fun drawHardBoxShadows(
    canvas: Canvas,
    geometry: ResolvedBoxGeometry?,
    shadows: List<HostBoxShadow>,
) {
    val box = geometry ?: return
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    shadows.asReversed().forEach { shadow ->
        if (shadow.inset || shadow.blurRadius != 0f) return@forEach
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
        canvas.drawPath(roundedPath(rect, radii), paint)
    }
}
