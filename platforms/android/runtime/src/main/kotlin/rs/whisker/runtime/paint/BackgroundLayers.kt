package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.RadialGradient
import android.graphics.Shader
import android.graphics.SweepGradient
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

internal data class HostPaintCoordinate(
    val length: Float,
    val fraction: Float,
) {
    fun resolve(axis: Float): Float = length + fraction * axis
}

internal data class HostRadialGradient(
    val centerX: HostPaintCoordinate,
    val centerY: HostPaintCoordinate,
    val radiusX: HostPaintCoordinate,
    val radiusY: HostPaintCoordinate,
    val stops: List<HostGradientStop>,
)

internal data class HostConicGradient(
    val fromDegrees: Float,
    val centerX: HostPaintCoordinate,
    val centerY: HostPaintCoordinate,
    val stops: List<HostGradientStop>,
)

/** Retained projection of the currently supported SetBackgroundLayers subset. */
internal data class HostBackgroundLayers(
    val linearGradient: HostLinearGradient?,
    val radialGradient: HostRadialGradient? = null,
    val conicGradient: HostConicGradient? = null,
    val geometry: HostBackgroundGeometry = HostBackgroundGeometry(),
)

internal fun drawBackgroundLayers(
    canvas: Canvas,
    boxes: HostBackgroundPaintBoxes,
    layers: HostBackgroundLayers?,
    paint: Paint,
) {
    val geometry = layers?.geometry ?: return
    val positioningBox = boxes.select(geometry.origin).rect
    val painting = boxes.select(geometry.clip)
    val saveCount = canvas.save().also {
        canvas.clipPath(painting.clip)
    }
    try {
        geometry.forEachImageBox(positioningBox, painting.rect) { imageBox ->
            val tileSaveCount = canvas.save().also { canvas.clipRect(imageBox) }
            try {
                drawBackgroundImage(canvas, painting.clip, imageBox, layers, paint)
            } finally {
                canvas.restoreToCount(tileSaveCount)
            }
        }
    } finally {
        canvas.restoreToCount(saveCount)
    }
}

private fun drawBackgroundImage(
    canvas: Canvas,
    clip: Path,
    imageBox: RectF,
    layers: HostBackgroundLayers,
    paint: Paint,
) {
    layers.linearGradient?.let { gradient ->
        drawLinearGradient(canvas, clip, imageBox, gradient, paint)
        return
    }
    layers.radialGradient?.let { gradient ->
        drawRadialGradient(canvas, clip, imageBox, gradient, paint)
        return
    }
    layers.conicGradient?.let { gradient ->
        drawConicGradient(canvas, clip, imageBox, gradient, paint)
    }
}

private fun drawLinearGradient(
    canvas: Canvas,
    clip: Path,
    box: RectF,
    gradient: HostLinearGradient,
    paint: Paint,
) {
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

private fun drawRadialGradient(
    canvas: Canvas,
    clip: Path,
    box: RectF,
    gradient: HostRadialGradient,
    paint: Paint,
) {
    if (gradient.stops.size < 2) return
    val centerX = box.left + gradient.centerX.resolve(box.width())
    val centerY = box.top + gradient.centerY.resolve(box.height())
    val radiusX = gradient.radiusX.resolve(box.width())
    val radiusY = gradient.radiusY.resolve(box.height())
    if (radiusX <= 0f || radiusY <= 0f) return
    var previousPosition = 0f
    val positions = gradient.stops.mapIndexed { index, stop ->
        val resolved = (stop.length / radiusX + stop.fraction).coerceIn(0f, 1f)
        resolved.coerceAtLeast(if (index == 0) 0f else previousPosition).also {
            previousPosition = it
        }
    }.toFloatArray()
    paint.color = Color.WHITE
    paint.shader = RadialGradient(
        centerX,
        centerY,
        radiusX,
        gradient.stops.map(HostGradientStop::color).toIntArray(),
        positions,
        Shader.TileMode.CLAMP,
    ).apply {
        setLocalMatrix(Matrix().apply {
            setScale(1f, radiusY / radiusX, centerX, centerY)
        })
    }
    canvas.drawPath(clip, paint)
    paint.shader = null
}

private fun drawConicGradient(
    canvas: Canvas,
    clip: Path,
    box: RectF,
    gradient: HostConicGradient,
    paint: Paint,
) {
    if (gradient.stops.size < 2) return
    val centerX = box.left + gradient.centerX.resolve(box.width())
    val centerY = box.top + gradient.centerY.resolve(box.height())
    var previousPosition = 0f
    val positions = gradient.stops.mapIndexed { index, stop ->
        stop.fraction.coerceIn(0f, 1f)
            .coerceAtLeast(if (index == 0) 0f else previousPosition)
            .also { previousPosition = it }
    }.toFloatArray()
    paint.color = Color.WHITE
    paint.shader = SweepGradient(
        centerX,
        centerY,
        gradient.stops.map(HostGradientStop::color).toIntArray(),
        positions,
    ).apply {
        // Android starts a sweep at +x. CSS/protocol starts at the positive
        // vertical axis, with both advancing clockwise in screen space.
        setLocalMatrix(Matrix().apply {
            setRotate(gradient.fromDegrees - 90f, centerX, centerY)
        })
    }
    canvas.drawPath(clip, paint)
    paint.shader = null
}
