package rs.whisker.runtime.paint

import android.graphics.Bitmap
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
import rs.whisker.runtime.bridge.MobileAbi
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.sin
import kotlin.math.sqrt

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
    val shape: HostRadialShape,
    val extent: HostRadialExtent,
    val centerX: HostPaintCoordinate,
    val centerY: HostPaintCoordinate,
    val radiusX: HostPaintCoordinate,
    val radiusY: HostPaintCoordinate,
    val stops: List<HostGradientStop>,
)

internal enum class HostRadialShape(val wireValue: Int) {
    Circle(MobileAbi.RADIAL_SHAPE_CIRCLE),
    Ellipse(MobileAbi.RADIAL_SHAPE_ELLIPSE);

    companion object {
        fun fromWire(value: Int): HostRadialShape? = entries.firstOrNull {
            it.wireValue == value
        }
    }
}

internal enum class HostRadialExtent(val wireValue: Int) {
    ClosestSide(MobileAbi.RADIAL_EXTENT_CLOSEST_SIDE),
    FarthestSide(MobileAbi.RADIAL_EXTENT_FARTHEST_SIDE),
    ClosestCorner(MobileAbi.RADIAL_EXTENT_CLOSEST_CORNER),
    FarthestCorner(MobileAbi.RADIAL_EXTENT_FARTHEST_CORNER),
    Explicit(MobileAbi.RADIAL_EXTENT_EXPLICIT);

    companion object {
        fun fromWire(value: Int): HostRadialExtent? = entries.firstOrNull {
            it.wireValue == value
        }
    }
}

internal data class HostConicGradient(
    val fromDegrees: Float,
    val centerX: HostPaintCoordinate,
    val centerY: HostPaintCoordinate,
    val stops: List<HostGradientStop>,
)

/** One retained projection of the currently supported background-image subset. */
internal data class HostBackgroundLayer(
    val linearGradient: HostLinearGradient?,
    val radialGradient: HostRadialGradient? = null,
    val conicGradient: HostConicGradient? = null,
    val rasterBitmap: Bitmap? = null,
    val intrinsicWidth: Float? = null,
    val intrinsicHeight: Float? = null,
    val geometry: HostBackgroundGeometry = HostBackgroundGeometry(),
)

/** CSS-ordered layers. The first entry is painted nearest the user. */
internal data class HostBackgroundLayers(val layers: List<HostBackgroundLayer>)

internal enum class HostImageRendering(val wireValue: Int) {
    Auto(0),
    Pixelated(1),
    CrispEdges(2);

    val usesNearestSampling: Boolean
        get() = this != Auto

    companion object {
        fun fromWire(value: Int): HostImageRendering? = entries.firstOrNull {
            it.wireValue == value
        }
    }
}

internal fun drawBackgroundLayers(
    canvas: Canvas,
    boxes: HostBackgroundPaintBoxes,
    layers: HostBackgroundLayers?,
    paint: Paint,
    imageRendering: HostImageRendering,
) {
    layers?.layers?.asReversed()?.forEach { layer ->
        drawBackgroundLayer(canvas, boxes, layer, paint, imageRendering)
    }
}

private fun drawBackgroundLayer(
    canvas: Canvas,
    boxes: HostBackgroundPaintBoxes,
    layer: HostBackgroundLayer,
    paint: Paint,
    imageRendering: HostImageRendering,
) {
    val geometry = layer.geometry
    val positioningBox = boxes.select(geometry.origin).rect
    val painting = boxes.select(geometry.clip)
    val saveCount = canvas.save().also {
        canvas.clipPath(painting.clip)
    }
    try {
        geometry.forEachImageBox(
            positioningBox,
            painting.rect,
            layer.intrinsicWidth,
            layer.intrinsicHeight,
        ) { imageBox ->
            val tileSaveCount = canvas.save().also { canvas.clipRect(imageBox) }
            try {
                drawBackgroundImage(canvas, painting.clip, imageBox, layer, paint, imageRendering)
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
    layer: HostBackgroundLayer,
    paint: Paint,
    imageRendering: HostImageRendering,
) {
    layer.rasterBitmap?.let { bitmap ->
        paint.color = Color.WHITE
        paint.shader = null
        paint.isFilterBitmap = !imageRendering.usesNearestSampling
        canvas.drawBitmap(bitmap, null, imageBox, paint)
        return
    }
    layer.linearGradient?.let { gradient ->
        drawLinearGradient(canvas, clip, imageBox, gradient, paint)
        return
    }
    layer.radialGradient?.let { gradient ->
        drawRadialGradient(canvas, clip, imageBox, gradient, paint)
        return
    }
    layer.conicGradient?.let { gradient ->
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
    val radii = resolveRadialRadii(box.width(), box.height(), gradient)
    val radiusX = radii.x
    val radiusY = radii.y
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

@JvmInline
internal value class HostRadialRadii private constructor(private val packed: Long) {
    val x: Float get() = Float.fromBits((packed ushr 32).toInt())
    val y: Float get() = Float.fromBits(packed.toInt())

    companion object {
        fun of(x: Float, y: Float): HostRadialRadii = HostRadialRadii(
            (x.toRawBits().toLong() shl 32) or (y.toRawBits().toLong() and 0xffff_ffffL),
        )
    }
}

internal fun resolveRadialRadii(
    width: Float,
    height: Float,
    gradient: HostRadialGradient,
): HostRadialRadii {
    val centerX = gradient.centerX.resolve(width)
    val centerY = gradient.centerY.resolve(height)
    if (gradient.extent == HostRadialExtent.Explicit) {
        val radiusX = gradient.radiusX.resolve(width)
        val radiusY = if (gradient.shape == HostRadialShape.Circle) {
            radiusX
        } else {
            gradient.radiusY.resolve(height)
        }
        return HostRadialRadii.of(radiusX, radiusY)
    }

    val nearX = minOf(centerX, width - centerX).coerceAtLeast(0f)
    val farX = maxOf(centerX, width - centerX).coerceAtLeast(0f)
    val nearY = minOf(centerY, height - centerY).coerceAtLeast(0f)
    val farY = maxOf(centerY, height - centerY).coerceAtLeast(0f)
    val x = if (
        gradient.extent == HostRadialExtent.ClosestSide ||
        gradient.extent == HostRadialExtent.ClosestCorner
    ) nearX else farX
    val y = if (
        gradient.extent == HostRadialExtent.ClosestSide ||
        gradient.extent == HostRadialExtent.ClosestCorner
    ) nearY else farY
    val corner = gradient.extent == HostRadialExtent.ClosestCorner ||
        gradient.extent == HostRadialExtent.FarthestCorner
    if (gradient.shape == HostRadialShape.Circle) {
        val radius = when {
            corner -> hypot(x.toDouble(), y.toDouble()).toFloat()
            gradient.extent == HostRadialExtent.ClosestSide -> minOf(x, y)
            else -> maxOf(x, y)
        }
        return HostRadialRadii.of(radius, radius)
    }
    val scale = if (corner) sqrt(2f) else 1f
    return HostRadialRadii.of(x * scale, y * scale)
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
