package rs.whisker.runtime.paint

import android.graphics.Path
import android.graphics.RectF
import kotlin.math.ceil
import kotlin.math.floor

internal enum class HostBackgroundRepeat {
    Repeat,
    NoRepeat,
    Space,
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
        if (
            base.isEmpty || paintingBox.isEmpty ||
            !base.width().isFinite() || !base.height().isFinite()
        ) {
            return
        }
        val xAxis = tileAxis(
            base.left,
            base.width(),
            positioningBox.left,
            positioningBox.width(),
            paintingBox.left,
            paintingBox.right,
            repeatX,
        )
        val yAxis = tileAxis(
            base.top,
            base.height(),
            positioningBox.top,
            positioningBox.height(),
            paintingBox.top,
            paintingBox.bottom,
            repeatY,
        )
        var tileCount = 0
        repeat(yAxis.count) { row ->
            val y = yAxis.first + row * yAxis.stride
            repeat(xAxis.count) { column ->
                val x = xAxis.first + column * xAxis.stride
                draw(RectF(x, y, x + base.width(), y + base.height()))
                tileCount += 1
                // Bound adversarial sub-pixel tiles so paint cannot monopolize
                // the Host UI thread. Normal viewport-sized CSS tiling remains
                // well below this ceiling.
                if (tileCount >= MAX_BACKGROUND_TILES) return
            }
        }
    }

    private fun tileAxis(
        base: Float,
        tileSize: Float,
        positioningStart: Float,
        positioningSize: Float,
        paintingStart: Float,
        paintingEnd: Float,
        repeat: HostBackgroundRepeat,
    ): TileAxis = when (repeat) {
        HostBackgroundRepeat.NoRepeat -> TileAxis(base, tileSize, 1)
        HostBackgroundRepeat.Repeat -> {
            val first = base + floor((paintingStart - base) / tileSize) * tileSize
            val count = ceil((paintingEnd - first) / tileSize)
                .toInt()
                .coerceIn(1, MAX_BACKGROUND_TILES)
            TileAxis(first, tileSize, count)
        }
        HostBackgroundRepeat.Space -> {
            val count = floor(positioningSize / tileSize)
                .toInt()
                .coerceIn(0, MAX_BACKGROUND_TILES)
            if (count >= 2) {
                TileAxis(
                    first = positioningStart,
                    stride = (positioningSize - tileSize) / (count - 1),
                    count = count,
                )
            } else {
                // CSS falls back to normal background-position when fewer
                // than two whole images fit on a space-repeated axis.
                TileAxis(base, tileSize, 1)
            }
        }
    }

    private data class TileAxis(
        val first: Float,
        val stride: Float,
        val count: Int,
    )

    private companion object {
        const val MAX_BACKGROUND_TILES = 16_384
    }
}
