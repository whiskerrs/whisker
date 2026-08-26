package rs.whisker.runtime.paint

import android.graphics.Path
import android.graphics.RectF
import kotlin.math.floor

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

    fun forEachImageBox(
        positioningBox: RectF,
        paintingBox: RectF,
        draw: (RectF) -> Unit,
    ) {
        val base = imageBox(positioningBox)
        if (base.isEmpty || paintingBox.isEmpty) return
        val firstX = firstTileOrigin(base.left, base.width(), paintingBox.left, repeatX)
        val firstY = firstTileOrigin(base.top, base.height(), paintingBox.top, repeatY)
        var tileCount = 0
        var y = firstY
        do {
            var x = firstX
            do {
                draw(RectF(x, y, x + base.width(), y + base.height()))
                tileCount += 1
                // Bound adversarial sub-pixel tiles so paint cannot monopolize
                // the Host UI thread. Normal viewport-sized CSS tiling remains
                // well below this ceiling.
                if (tileCount >= MAX_BACKGROUND_TILES) return
                x += base.width()
            } while (repeatX == HostBackgroundRepeat.Repeat && x < paintingBox.right)
            y += base.height()
        } while (repeatY == HostBackgroundRepeat.Repeat && y < paintingBox.bottom)
    }

    private fun firstTileOrigin(
        base: Float,
        tileSize: Float,
        paintStart: Float,
        repeat: HostBackgroundRepeat,
    ): Float = if (repeat == HostBackgroundRepeat.Repeat) {
        base + floor((paintStart - base) / tileSize) * tileSize
    } else {
        base
    }

    private companion object {
        const val MAX_BACKGROUND_TILES = 16_384
    }
}
