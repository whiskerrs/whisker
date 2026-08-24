package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Shader
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.sin

internal data class HostGradientStop(
    val color: Int,
    val length: Float,
    val fraction: Float,
)

internal data class HostLinearGradient(
    val angleDegrees: Float,
    val stops: List<HostGradientStop>,
)

/** Retained projection of the currently supported SetBackgroundLayers subset. */
internal data class HostBackgroundLayers(
    val linearGradient: HostLinearGradient?,
)

internal fun drawBackgroundLayers(
    canvas: Canvas,
    clip: Path,
    box: RectF,
    layers: HostBackgroundLayers?,
    paint: Paint,
) {
    val gradient = layers?.linearGradient ?: return
    if (gradient.stops.size < 2) return
    val radians = Math.toRadians(gradient.angleDegrees.toDouble())
    val directionX = sin(radians).toFloat()
    val directionY = -cos(radians).toFloat()
    val halfLength = (abs(directionX) * box.width() + abs(directionY) * box.height()) / 2f
    val centerX = box.centerX()
    val centerY = box.centerY()
    val gradientLength = halfLength * 2f
    var previousPosition = Float.NEGATIVE_INFINITY
    val resolved = gradient.stops.map { stop ->
        val position = if (gradientLength > 0f) {
            stop.length / gradientLength + stop.fraction
        } else {
            stop.fraction
        }
        Pair(stop.color, position.coerceAtLeast(previousPosition).also {
            previousPosition = it
        })
    }.toMutableList()
    val domainStart = minOf(0f, resolved.first().second)
    val domainEnd = maxOf(1f, resolved.last().second)
    val domainLength = domainEnd - domainStart
    if (domainLength <= 0f) return
    if (resolved.first().second > domainStart) {
        resolved.add(0, Pair(resolved.first().first, domainStart))
    }
    if (resolved.last().second < domainEnd) {
        resolved.add(Pair(resolved.last().first, domainEnd))
    }
    val positions = resolved.map { (_, position) ->
        (position - domainStart) / domainLength
    }.toFloatArray()
    // Shader colors are modulated by the Paint color/alpha left by the box
    // background, which may itself be transparent.
    paint.color = Color.WHITE
    paint.shader = LinearGradient(
        centerX + directionX * gradientLength * (domainStart - 0.5f),
        centerY + directionY * gradientLength * (domainStart - 0.5f),
        centerX + directionX * gradientLength * (domainEnd - 0.5f),
        centerY + directionY * gradientLength * (domainEnd - 0.5f),
        resolved.map { it.first }.toIntArray(),
        positions,
        Shader.TileMode.CLAMP,
    )
    canvas.drawPath(clip, paint)
    paint.shader = null
}
