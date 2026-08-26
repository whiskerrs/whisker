package rs.whisker.runtime.paint

import android.graphics.RectF

internal enum class HostBackgroundRepeat {
    Repeat,
    NoRepeat,
}

/** Retained geometry for one background image layer. */
internal data class HostBackgroundGeometry(
    val positionX: HostPaintCoordinate = HostPaintCoordinate(0f, 0f),
    val positionY: HostPaintCoordinate = HostPaintCoordinate(0f, 0f),
    val sizeWidth: HostPaintCoordinate? = null,
    val sizeHeight: HostPaintCoordinate? = null,
    val repeatX: HostBackgroundRepeat = HostBackgroundRepeat.Repeat,
    val repeatY: HostBackgroundRepeat = HostBackgroundRepeat.Repeat,
) {
    fun imageBox(positioningBox: RectF): RectF {
        val width = sizeWidth?.resolve(positioningBox.width()) ?: positioningBox.width()
        val height = sizeHeight?.resolve(positioningBox.height()) ?: positioningBox.height()
        val left = positioningBox.left + positionX.resolve(positioningBox.width())
        val top = positioningBox.top + positionY.resolve(positioningBox.height())
        return RectF(left, top, left + width, top + height)
    }

    val clipsToSingleImage: Boolean
        get() = repeatX == HostBackgroundRepeat.NoRepeat && repeatY == HostBackgroundRepeat.NoRepeat
}
