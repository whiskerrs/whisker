package rs.whisker.runtime.paint

import android.graphics.RectF
import android.graphics.Path

internal enum class HostBackgroundRepeat {
    Repeat,
    NoRepeat,
}

internal enum class HostBackgroundBox {
    Border,
    Padding,
}

internal data class HostBackgroundPaintBox(
    val rect: RectF,
    val clip: Path,
)

internal data class HostBackgroundPaintBoxes(
    val border: HostBackgroundPaintBox,
    val padding: HostBackgroundPaintBox,
) {
    fun select(box: HostBackgroundBox): HostBackgroundPaintBox = when (box) {
        HostBackgroundBox.Border -> border
        HostBackgroundBox.Padding -> padding
    }
}

/** Retained geometry for one background image layer. */
internal data class HostBackgroundGeometry(
    val positionX: HostPaintCoordinate = HostPaintCoordinate(0f, 0f),
    val positionY: HostPaintCoordinate = HostPaintCoordinate(0f, 0f),
    val sizeWidth: HostPaintCoordinate? = null,
    val sizeHeight: HostPaintCoordinate? = null,
    val repeatX: HostBackgroundRepeat = HostBackgroundRepeat.Repeat,
    val repeatY: HostBackgroundRepeat = HostBackgroundRepeat.Repeat,
    val origin: HostBackgroundBox = HostBackgroundBox.Padding,
    val clip: HostBackgroundBox = HostBackgroundBox.Border,
) {
    fun imageBox(positioningBox: RectF): RectF {
        val width = sizeWidth?.resolve(positioningBox.width()) ?: positioningBox.width()
        val height = sizeHeight?.resolve(positioningBox.height()) ?: positioningBox.height()
        val left = positioningBox.left +
            positionX.length + positionX.fraction * (positioningBox.width() - width)
        val top = positioningBox.top +
            positionY.length + positionY.fraction * (positioningBox.height() - height)
        return RectF(left, top, left + width, top + height)
    }

    val clipsToSingleImage: Boolean
        get() = repeatX == HostBackgroundRepeat.NoRepeat && repeatY == HostBackgroundRepeat.NoRepeat
}
