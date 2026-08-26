package rs.whisker.runtime.paint

import android.graphics.Path
import android.graphics.RectF
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.roundToInt

internal enum class HostBackgroundRepeat {
    Repeat,
    NoRepeat,
    Space,
    Round,
}

internal enum class HostBackgroundBox {
    Border,
    Padding,
    Content,
    BorderArea,
}

internal enum class HostBackgroundSize {
    Auto,
    Explicit,
    Cover,
    Contain,
    Width,
    Height,
}

internal data class HostBackgroundPaintBox(
    val rect: RectF,
    val clip: Path,
)

internal data class HostBackgroundPaintBoxes(
    val border: HostBackgroundPaintBox,
    val padding: HostBackgroundPaintBox,
    val content: HostBackgroundPaintBox,
    val borderArea: HostBackgroundPaintBox,
) {
    fun select(box: HostBackgroundBox): HostBackgroundPaintBox = when (box) {
        HostBackgroundBox.Border -> border
        HostBackgroundBox.Padding -> padding
        HostBackgroundBox.Content -> content
        HostBackgroundBox.BorderArea -> borderArea
    }
}

/** Retained geometry for one background image layer. */
internal data class HostBackgroundGeometry(
    val positionX: HostPaintCoordinate = HostPaintCoordinate(0f, 0f),
    val positionY: HostPaintCoordinate = HostPaintCoordinate(0f, 0f),
    val sizeWidth: HostPaintCoordinate? = null,
    val sizeHeight: HostPaintCoordinate? = null,
    val size: HostBackgroundSize = HostBackgroundSize.Auto,
    val repeatX: HostBackgroundRepeat = HostBackgroundRepeat.Repeat,
    val repeatY: HostBackgroundRepeat = HostBackgroundRepeat.Repeat,
    val origin: HostBackgroundBox = HostBackgroundBox.Padding,
    val clip: HostBackgroundBox = HostBackgroundBox.Border,
) {
    private fun imageBox(
        positioningBox: RectF,
        width: Float,
        height: Float,
    ): RectF {
        val left = positioningBox.left +
            positionX.length + positionX.fraction * (positioningBox.width() - width)
        val top = positioningBox.top +
            positionY.length + positionY.fraction * (positioningBox.height() - height)
        return RectF(left, top, left + width, top + height)
    }

    fun forEachImageBox(
        positioningBox: RectF,
        paintingBox: RectF,
        intrinsicWidth: Float?,
        intrinsicHeight: Float?,
        draw: (RectF) -> Unit,
    ) {
        val (originalWidth, originalHeight) = originalTileSize(
            positioningBox,
            intrinsicWidth,
            intrinsicHeight,
        )
        if (originalWidth <= 0f || originalHeight <= 0f) return
        var tileWidth = adjustedTileSize(originalWidth, positioningBox.width(), repeatX)
        var tileHeight = adjustedTileSize(originalHeight, positioningBox.height(), repeatY)
        val hasIntrinsicAspectRatio = intrinsicWidth != null && intrinsicHeight != null &&
            intrinsicWidth > 0f && intrinsicHeight > 0f &&
            intrinsicWidth.isFinite() && intrinsicHeight.isFinite()
        if (
            hasIntrinsicAspectRatio && size == HostBackgroundSize.Width &&
            repeatX == HostBackgroundRepeat.Round && repeatY != HostBackgroundRepeat.Round
        ) {
            tileHeight = originalHeight * tileWidth / originalWidth
        } else if (
            hasIntrinsicAspectRatio && size == HostBackgroundSize.Height &&
            repeatY == HostBackgroundRepeat.Round && repeatX != HostBackgroundRepeat.Round
        ) {
            tileWidth = originalWidth * tileHeight / originalHeight
        }
        val base = imageBox(positioningBox, tileWidth, tileHeight)
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

    private fun originalTileSize(
        positioningBox: RectF,
        intrinsicWidth: Float?,
        intrinsicHeight: Float?,
    ): Pair<Float, Float> {
        val boxWidth = positioningBox.width()
        val boxHeight = positioningBox.height()
        val intrinsicWidthValue = intrinsicWidth ?: 0f
        val intrinsicHeightValue = intrinsicHeight ?: 0f
        val hasIntrinsicSize = intrinsicWidthValue > 0f && intrinsicHeightValue > 0f &&
            intrinsicWidthValue.isFinite() && intrinsicHeightValue.isFinite()
        return when (size) {
            HostBackgroundSize.Auto -> if (hasIntrinsicSize) {
                intrinsicWidthValue to intrinsicHeightValue
            } else {
                boxWidth to boxHeight
            }
            HostBackgroundSize.Explicit -> {
                checkNotNull(sizeWidth).resolve(boxWidth) to
                    checkNotNull(sizeHeight).resolve(boxHeight)
            }
            HostBackgroundSize.Width -> {
                val width = checkNotNull(sizeWidth).resolve(boxWidth)
                width to if (hasIntrinsicSize) {
                    width * intrinsicHeightValue / intrinsicWidthValue
                } else {
                    boxHeight
                }
            }
            HostBackgroundSize.Height -> {
                val height = checkNotNull(sizeHeight).resolve(boxHeight)
                if (hasIntrinsicSize) {
                    height * intrinsicWidthValue / intrinsicHeightValue
                } else {
                    boxWidth
                } to height
            }
            HostBackgroundSize.Cover,
            HostBackgroundSize.Contain,
            -> if (hasIntrinsicSize) {
                val widthScale = boxWidth / intrinsicWidthValue
                val heightScale = boxHeight / intrinsicHeightValue
                val scale = if (size == HostBackgroundSize.Cover) {
                    maxOf(widthScale, heightScale)
                } else {
                    minOf(widthScale, heightScale)
                }
                intrinsicWidthValue * scale to intrinsicHeightValue * scale
            } else {
                boxWidth to boxHeight
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
        HostBackgroundRepeat.Repeat, HostBackgroundRepeat.Round -> {
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

    private fun adjustedTileSize(
        originalSize: Float,
        positioningSize: Float,
        repeat: HostBackgroundRepeat,
    ): Float {
        if (repeat != HostBackgroundRepeat.Round) return originalSize
        val count = (positioningSize / originalSize)
            .roundToInt()
            .coerceIn(1, MAX_BACKGROUND_TILES)
        return positioningSize / count
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
