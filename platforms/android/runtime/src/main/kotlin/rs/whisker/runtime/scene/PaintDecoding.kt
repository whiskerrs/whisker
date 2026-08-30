package rs.whisker.runtime.scene

import android.content.Context
import android.os.Build
import android.graphics.RectF
import android.util.Log
import android.view.View
import android.view.ViewGroup
import java.util.ArrayDeque
import rs.whisker.runtime.WhiskerChildPolicy
import rs.whisker.runtime.WhiskerContainerView
import rs.whisker.runtime.WhiskerView
import rs.whisker.runtime.WhiskerElementRegistry
import rs.whisker.runtime.WhiskerTextContent
import rs.whisker.runtime.WhiskerFontStyle
import rs.whisker.runtime.WhiskerFontFeature
import rs.whisker.runtime.WhiskerFontOpticalSizing
import rs.whisker.runtime.WhiskerFontVariation
import rs.whisker.runtime.WhiskerTextDecoration
import rs.whisker.runtime.WhiskerTextDecorationLine
import rs.whisker.runtime.WhiskerTextDecorationStyle
import rs.whisker.runtime.WhiskerTextAlignment
import rs.whisker.runtime.WhiskerTextDirection
import rs.whisker.runtime.WhiskerTextIndent
import rs.whisker.runtime.WhiskerTextOverflow
import rs.whisker.runtime.WhiskerTextShadow
import rs.whisker.runtime.WhiskerTextWordBreak
import rs.whisker.runtime.WhiskerValue
import rs.whisker.runtime.bridge.MobileAbi
import rs.whisker.runtime.styleSnapshot
import rs.whisker.runtime.paint.HostBackgroundGeometry
import rs.whisker.runtime.paint.HostBackgroundBox
import rs.whisker.runtime.paint.HostBackgroundLayer
import rs.whisker.runtime.paint.HostBoxPaint
import rs.whisker.runtime.paint.HostBoxShadow
import rs.whisker.runtime.paint.HostBackgroundLayers
import rs.whisker.runtime.paint.HostBackgroundRepeat
import rs.whisker.runtime.paint.HostBackgroundSize
import rs.whisker.runtime.paint.HostConicGradient
import rs.whisker.runtime.paint.HostGradientStop
import rs.whisker.runtime.paint.HostClipReferenceBox
import rs.whisker.runtime.paint.HostCircleClipPath
import rs.whisker.runtime.paint.HostEllipseClipPath
import rs.whisker.runtime.paint.HostInsetClipPath
import rs.whisker.runtime.paint.HostImageRendering
import rs.whisker.runtime.paint.HostPathClipPath
import rs.whisker.runtime.paint.HostPathCommand
import rs.whisker.runtime.paint.HostLinearGradient
import rs.whisker.runtime.paint.HostPaintCoordinate
import rs.whisker.runtime.paint.HostRadialGradient
import rs.whisker.runtime.paint.HostRasterResourceStore
import rs.whisker.runtime.paint.applyBoxPaint
import rs.whisker.runtime.paint.parseNamedColor
import rs.whisker.runtime.paint.rgba
import rs.whisker.runtime.paint.resolveClipPath
import kotlin.math.min

internal fun validClipPath(
    operation: HostSceneOperation,
    existing: Set<Long>,
): Boolean {
    if (operation.node !in existing) return false
    val values = operation.numbers ?: return true
    val shape = values.getOrNull(1)?.toInt() ?: return false
    val expectedSize = when (shape) {
        CLIP_SHAPE_INSET -> CLIP_PATH_INSET_PACKED_SIZE
        CLIP_SHAPE_CIRCLE -> CLIP_PATH_CIRCLE_PACKED_SIZE
        CLIP_SHAPE_ELLIPSE -> CLIP_PATH_ELLIPSE_PACKED_SIZE
        CLIP_SHAPE_PATH -> {
            val commandCount = values.getOrNull(3)?.toInt() ?: return false
            if (commandCount <= 0 || commandCount > MAX_PATH_COMMANDS ||
                values[3] != commandCount.toFloat()
            ) return false
            CLIP_PATH_HEADER_SIZE + commandCount * PATH_COMMAND_PACKED_SIZE
        }
        else -> return false
    }
    return values.size == expectedSize &&
        values.all(Float::isFinite) &&
        values[0].toInt() in BACKGROUND_BOX_BORDER..BACKGROUND_BOX_CONTENT &&
        values[0] == values[0].toInt().toFloat() &&
        values[1] == shape.toFloat() &&
        when (shape) {
            CLIP_SHAPE_INSET -> (10 until expectedSize).all { values[it] >= 0f }
            CLIP_SHAPE_CIRCLE -> values[2] >= 0f && values[3] >= 0f
            CLIP_SHAPE_ELLIPSE -> values[2] >= 0f && values[3] >= 0f && values[4] >= 0f && values[5] >= 0f
            else -> {
                values[2].toInt() in FILL_RULE_NON_ZERO..FILL_RULE_EVEN_ODD &&
                    values[2] == values[2].toInt().toFloat() &&
                    (CLIP_PATH_HEADER_SIZE until expectedSize step PATH_COMMAND_PACKED_SIZE).all { offset ->
                        values[offset].toInt() in PATH_MOVE_TO..PATH_CLOSE &&
                            values[offset] == values[offset].toInt().toFloat()
                    }
            }
        }
}

internal fun validBoxShadows(
    operation: HostSceneOperation,
    existing: Set<Long>,
): Boolean {
    val values = operation.numbers ?: return false
    val names = operation.names ?: return false
    return operation.node in existing &&
        values.size % BOX_SHADOW_PACKED_SIZE == 0 &&
        names.size == values.size / BOX_SHADOW_PACKED_SIZE &&
        values.all(Float::isFinite) &&
        values.indices.step(BOX_SHADOW_PACKED_SIZE).all { offset ->
            val blur = values[offset + 2]
            val inset = values[offset + 4]
            blur >= 0f && (inset == 0f || inset == 1f) &&
                (values[offset + 5] == 0f || values[offset + 5] == 1f)
        }
}

internal fun decodeBackgroundLayer(
    operation: HostSceneOperation,
    density: Float,
    rasterResources: HostRasterResourceStore,
): HostBackgroundLayer {
    val numbers = requireNotNull(operation.numbers)
    val names = requireNotNull(operation.names)
    val imageOffset = BACKGROUND_GEOMETRY_PACKED_SIZE
    val stopOffset = imageOffset + when (operation.flags) {
        BACKGROUND_RADIAL -> 8
        BACKGROUND_CONIC -> 4
        BACKGROUND_RESOURCE -> BACKGROUND_RESOURCE_ID_WORDS
        else -> 0
    }
    val stops = if (operation.flags == BACKGROUND_RESOURCE) {
        emptyList()
    } else {
        decodeGradientStops(numbers, stopOffset, names, density)
    }
    fun coordinate(offset: Int) = HostPaintCoordinate(
        length = numbers[offset] * density,
        fraction = numbers[offset + 1],
    )
    val geometry = HostBackgroundGeometry(
        positionX = coordinate(0),
        positionY = coordinate(2),
        sizeWidth = if (
            numbers[8] == BACKGROUND_SIZE_EXPLICIT.toFloat() ||
            numbers[8] == BACKGROUND_SIZE_WIDTH.toFloat()
        ) {
            coordinate(4)
        } else {
            null
        },
        sizeHeight = if (
            numbers[8] == BACKGROUND_SIZE_EXPLICIT.toFloat() ||
            numbers[8] == BACKGROUND_SIZE_HEIGHT.toFloat()
        ) {
            coordinate(6)
        } else {
            null
        },
        size = backgroundSize(numbers[8]),
        repeatX = backgroundRepeat(numbers[9]),
        repeatY = backgroundRepeat(numbers[10]),
        origin = backgroundBox(numbers[11]),
        clip = backgroundBox(numbers[12]),
    )
    return if (operation.flags == BACKGROUND_RESOURCE) {
        val resourceId = requireNotNull(decodeResourceId(numbers, imageOffset))
        val bitmap = requireNotNull(rasterResources.resolve(resourceId))
        HostBackgroundLayer(
            linearGradient = null,
            rasterBitmap = bitmap,
            intrinsicWidth = bitmap.width * density,
            intrinsicHeight = bitmap.height * density,
            geometry = geometry,
        )
    } else if (operation.flags == BACKGROUND_RADIAL) {
        HostBackgroundLayer(
            linearGradient = null,
            radialGradient = HostRadialGradient(
                centerX = coordinate(imageOffset),
                centerY = coordinate(imageOffset + 2),
                radiusX = coordinate(imageOffset + 4),
                radiusY = coordinate(imageOffset + 6),
                stops = stops,
            ),
            geometry = geometry,
        )
    } else if (operation.flags == BACKGROUND_CONIC) {
        HostBackgroundLayer(
            linearGradient = null,
            conicGradient = HostConicGradient(
                fromDegrees = operation.scalar,
                centerX = coordinate(imageOffset),
                centerY = coordinate(imageOffset + 2),
                stops = stops,
            ),
            geometry = geometry,
        )
    } else {
        HostBackgroundLayer(
            linearGradient = HostLinearGradient(operation.scalar, stops),
            geometry = geometry,
        )
    }
}

internal fun projectedBackgroundLayerOperations(
    operation: HostSceneOperation,
): List<HostSceneOperation>? {
    if (operation.flags != BACKGROUND_PACKED_LAYERS) return listOf(operation)
    val packed = operation.numbers ?: return null
    val names = operation.names ?: return null
    if (packed.isEmpty()) return null
    val layerCount = packedCount(packed[0], MAX_BACKGROUND_LAYERS) ?: return null
    if (layerCount == 0) return null
    var cursor = 1
    var nameCursor = 0
    val result = ArrayList<HostSceneOperation>(layerCount)
    repeat(layerCount) {
        if (cursor + BACKGROUND_PACKED_LAYER_HEADER_SIZE > packed.size) return null
        val kind = packedCount(packed[cursor], BACKGROUND_RESOURCE) ?: return null
        val scalar = packed[cursor + 1]
        val valueCount = packedCount(packed[cursor + 2], packed.size) ?: return null
        cursor += BACKGROUND_PACKED_LAYER_HEADER_SIZE
        if (valueCount > packed.size - cursor) return null
        val values = packed.copyOfRange(cursor, cursor + valueCount)
        cursor += valueCount
        val imagePrefix = when (kind) {
            BACKGROUND_RADIAL -> 8
            BACKGROUND_CONIC -> 4
            BACKGROUND_RESOURCE -> BACKGROUND_RESOURCE_ID_WORDS
            else -> 0
        }
        val stopValues = valueCount - BACKGROUND_GEOMETRY_PACKED_SIZE - imagePrefix
        if (stopValues < 0 || stopValues % BACKGROUND_GRADIENT_STOP_PACKED_SIZE != 0) return null
        val nameCount = stopValues / BACKGROUND_GRADIENT_STOP_PACKED_SIZE
        if (nameCount > names.size - nameCursor) return null
        val layerNames = names.copyOfRange(nameCursor, nameCursor + nameCount)
        nameCursor += nameCount
        result += operation.copy(
            flags = kind,
            scalar = scalar,
            numbers = values,
            names = layerNames,
        )
    }
    return result.takeIf { cursor == packed.size && nameCursor == names.size }
}

internal fun packedCount(value: Float, maximum: Int): Int? {
    if (!value.isFinite() || value < 0f || value > maximum.toFloat()) return null
    val integer = value.toInt()
    return integer.takeIf { it.toFloat() == value }
}

internal fun decodeResourceId(numbers: FloatArray, offset: Int): Long? {
    if (offset + BACKGROUND_RESOURCE_ID_WORDS > numbers.size) return null
    var resourceId = 0L
    repeat(BACKGROUND_RESOURCE_ID_WORDS) { wordIndex ->
        val word = packedCount(numbers[offset + wordIndex], RESOURCE_ID_WORD_MAX) ?: return null
        resourceId = resourceId or (word.toLong() shl (wordIndex * RESOURCE_ID_WORD_BITS))
    }
    return resourceId.takeIf { it != 0L }
}

internal fun decodeGradientStops(
    numbers: FloatArray,
    start: Int,
    names: Array<String>,
    density: Float,
): List<HostGradientStop> = List((numbers.size - start) / 7) { index ->
    val offset = start + index * 7
    HostGradientStop(
        color = if (numbers[offset] == 0f) {
            parseNamedColor(names[index])
        } else {
            rgba(
                numbers[offset + 1],
                numbers[offset + 2],
                numbers[offset + 3],
                numbers[offset + 4],
            )
        },
        length = numbers[offset + 5] * density,
        fraction = numbers[offset + 6],
    )
}

internal fun backgroundRepeat(value: Float): HostBackgroundRepeat =
    when (value) {
        BACKGROUND_REPEAT.toFloat() -> HostBackgroundRepeat.Repeat
        BACKGROUND_NO_REPEAT.toFloat() -> HostBackgroundRepeat.NoRepeat
        BACKGROUND_SPACE.toFloat() -> HostBackgroundRepeat.Space
        else -> HostBackgroundRepeat.Round
    }

internal fun backgroundSize(value: Float): HostBackgroundSize =
    when (value) {
        BACKGROUND_SIZE_EXPLICIT.toFloat() -> HostBackgroundSize.Explicit
        BACKGROUND_SIZE_COVER.toFloat() -> HostBackgroundSize.Cover
        BACKGROUND_SIZE_CONTAIN.toFloat() -> HostBackgroundSize.Contain
        BACKGROUND_SIZE_WIDTH.toFloat() -> HostBackgroundSize.Width
        BACKGROUND_SIZE_HEIGHT.toFloat() -> HostBackgroundSize.Height
        else -> HostBackgroundSize.Auto
    }

internal fun backgroundBox(value: Float): HostBackgroundBox =
    when (value) {
        BACKGROUND_BOX_BORDER.toFloat() -> HostBackgroundBox.Border
        BACKGROUND_BOX_PADDING.toFloat() -> HostBackgroundBox.Padding
        BACKGROUND_BOX_BORDER_AREA.toFloat() -> HostBackgroundBox.BorderArea
        else -> HostBackgroundBox.Content
    }

internal fun validBackgroundLayers(
    operation: HostSceneOperation,
    existing: Set<Long>,
    rasterResources: HostRasterResourceStore,
): Boolean {
    if (operation.node !in existing) return false
    val numbers = operation.numbers ?: FloatArray(0)
    val names = operation.names ?: emptyArray()
    if (numbers.isEmpty()) {
        return operation.flags == BACKGROUND_LINEAR &&
            operation.scalar.isFinite() && names.isEmpty()
    }
    return projectedBackgroundLayerOperations(operation)
        ?.all { validBackgroundLayer(it, rasterResources) } == true
}

internal fun validBackgroundLayer(
    operation: HostSceneOperation,
    rasterResources: HostRasterResourceStore,
): Boolean {
    if (
        operation.flags !in BACKGROUND_LINEAR..BACKGROUND_RESOURCE ||
        !operation.scalar.isFinite()
    ) return false
    val numbers = operation.numbers ?: return false
    val names = operation.names ?: return false
    if (numbers.size < BACKGROUND_GEOMETRY_PACKED_SIZE || !validBackgroundGeometry(numbers)) {
        return false
    }
    val stopOffset = BACKGROUND_GEOMETRY_PACKED_SIZE + when (operation.flags) {
        BACKGROUND_RADIAL -> 8
        BACKGROUND_CONIC -> 4
        BACKGROUND_RESOURCE -> BACKGROUND_RESOURCE_ID_WORDS
        else -> 0
    }
    if (operation.flags == BACKGROUND_RESOURCE) {
        return numbers.size == stopOffset && names.isEmpty() &&
            numbers.indices.all { numbers[it].isFinite() } &&
            decodeResourceId(numbers, BACKGROUND_GEOMETRY_PACKED_SIZE)
                ?.let(rasterResources::resolve) != null
    }
    if (
        numbers.size < stopOffset + 14 || (numbers.size - stopOffset) % 7 != 0 ||
        names.size != (numbers.size - stopOffset) / 7
    ) {
        return false
    }
    return numbers.indices.all { index -> numbers[index].isFinite() } &&
        (stopOffset until numbers.size step 7).all { offset ->
            numbers[offset] == 0f || numbers[offset] == 1f
        }
}

internal fun validBackgroundGeometry(numbers: FloatArray): Boolean {
    val sizeKind = numbers[8]
    val repeatX = numbers[9]
    val repeatY = numbers[10]
    return validBackgroundSize(sizeKind) &&
        validBackgroundRepeat(repeatX) &&
        validBackgroundRepeat(repeatY) &&
        validBackgroundOrigin(numbers[11]) &&
        validBackgroundClip(numbers[12]) &&
        numbers[13] == BACKGROUND_ATTACHMENT_SCROLL.toFloat() &&
        numbers[14] == BACKGROUND_BLEND_NORMAL.toFloat()
}

internal fun validBackgroundSize(value: Float): Boolean =
    value == BACKGROUND_SIZE_AUTO.toFloat() ||
        value == BACKGROUND_SIZE_EXPLICIT.toFloat() ||
        value == BACKGROUND_SIZE_COVER.toFloat() ||
        value == BACKGROUND_SIZE_CONTAIN.toFloat() ||
        value == BACKGROUND_SIZE_WIDTH.toFloat() ||
        value == BACKGROUND_SIZE_HEIGHT.toFloat()

internal fun validBackgroundRepeat(value: Float): Boolean =
    value == BACKGROUND_REPEAT.toFloat() ||
        value == BACKGROUND_NO_REPEAT.toFloat() ||
        value == BACKGROUND_SPACE.toFloat() ||
        value == BACKGROUND_ROUND.toFloat()

internal fun validBackgroundOrigin(value: Float): Boolean =
    value == BACKGROUND_BOX_BORDER.toFloat() ||
        value == BACKGROUND_BOX_PADDING.toFloat() ||
        value == BACKGROUND_BOX_CONTENT.toFloat()

internal fun validBackgroundClip(value: Float): Boolean =
    validBackgroundOrigin(value) || value == BACKGROUND_BOX_BORDER_AREA.toFloat()

internal const val OP_CREATE = MobileAbi.OP_CREATE
internal const val OP_DELETE = MobileAbi.OP_DELETE
internal const val OP_INSERT = MobileAbi.OP_INSERT
internal const val OP_REMOVE = MobileAbi.OP_REMOVE
internal const val OP_MOVE = MobileAbi.OP_MOVE
internal const val OP_LAYOUT = MobileAbi.OP_LAYOUT
internal const val OP_PAINT = MobileAbi.OP_PAINT
internal const val OP_CLIP = MobileAbi.OP_CLIP
internal const val OP_TRANSFORM = MobileAbi.OP_TRANSFORM
internal const val OP_OPACITY = MobileAbi.OP_OPACITY
internal const val OP_VISIBILITY = MobileAbi.OP_VISIBILITY
internal const val OP_Z_ORDER = MobileAbi.OP_Z_ORDER
internal const val OP_TEXT = MobileAbi.OP_TEXT
internal const val OP_PROPERTY = MobileAbi.OP_PROPERTY
internal const val OP_CLEAR_PROPERTY = MobileAbi.OP_CLEAR_PROPERTY
internal const val OP_EVENT_MASK = MobileAbi.OP_EVENT_MASK
internal const val OP_HIT_TEST = MobileAbi.OP_HIT_TEST
internal const val OP_COMMAND = MobileAbi.OP_COMMAND
internal const val OP_BACKGROUND_LAYERS = MobileAbi.OP_BACKGROUND_LAYERS
internal const val OP_BOX_SHADOWS = MobileAbi.OP_BOX_SHADOWS
internal const val OP_CLIP_PATH = MobileAbi.OP_CLIP_PATH
internal const val OP_BACKDROP_BLUR = MobileAbi.OP_BACKDROP_BLUR
internal const val OP_IMAGE_RENDERING = MobileAbi.OP_IMAGE_RENDERING
internal const val OP_CURSOR = MobileAbi.OP_CURSOR
internal const val OP_TEXT_STYLE = MobileAbi.OP_TEXT_STYLE
internal const val OP_ACCESSIBILITY = MobileAbi.OP_ACCESSIBILITY
internal const val BACKGROUND_GEOMETRY_PACKED_SIZE = 15
internal const val BOX_SHADOW_PACKED_SIZE = 10
internal const val CLIP_PATH_INSET_PACKED_SIZE = 26
internal const val CLIP_PATH_CIRCLE_PACKED_SIZE = 8
internal const val CLIP_PATH_ELLIPSE_PACKED_SIZE = 10
internal const val CLIP_PATH_HEADER_SIZE = 4
internal const val PATH_COMMAND_PACKED_SIZE = 13
internal const val MAX_PATH_COMMANDS = 4096
internal const val CLIP_SHAPE_INSET = MobileAbi.CLIP_SHAPE_INSET
internal const val CLIP_SHAPE_CIRCLE = MobileAbi.CLIP_SHAPE_CIRCLE
internal const val CLIP_SHAPE_ELLIPSE = MobileAbi.CLIP_SHAPE_ELLIPSE
internal const val CLIP_SHAPE_PATH = MobileAbi.CLIP_SHAPE_PATH
internal const val FILL_RULE_NON_ZERO = MobileAbi.FILL_RULE_NON_ZERO
internal const val FILL_RULE_EVEN_ODD = MobileAbi.FILL_RULE_EVEN_ODD
internal const val PATH_MOVE_TO = MobileAbi.PATH_MOVE_TO
internal const val PATH_CLOSE = MobileAbi.PATH_CLOSE
internal const val BACKGROUND_GRADIENT_STOP_PACKED_SIZE = 7
internal const val BACKGROUND_PACKED_LAYER_HEADER_SIZE = 3
internal const val BACKGROUND_PACKED_LAYERS = 256
internal const val MAX_BACKGROUND_LAYERS = 256
internal const val BACKGROUND_RESOURCE_ID_WORDS = 4
internal const val RESOURCE_ID_WORD_BITS = 16
internal const val RESOURCE_ID_WORD_MAX = 0xffff
internal const val BACKGROUND_LINEAR = MobileAbi.BACKGROUND_LINEAR
internal const val BACKGROUND_RADIAL = MobileAbi.BACKGROUND_RADIAL
internal const val BACKGROUND_CONIC = MobileAbi.BACKGROUND_CONIC
internal const val BACKGROUND_RESOURCE = MobileAbi.BACKGROUND_RESOURCE
internal const val BACKGROUND_SIZE_AUTO = MobileAbi.BACKGROUND_SIZE_AUTO
internal const val BACKGROUND_SIZE_EXPLICIT = MobileAbi.BACKGROUND_SIZE_EXPLICIT
internal const val BACKGROUND_SIZE_COVER = MobileAbi.BACKGROUND_SIZE_COVER
internal const val BACKGROUND_SIZE_CONTAIN = MobileAbi.BACKGROUND_SIZE_CONTAIN
internal const val BACKGROUND_SIZE_WIDTH = MobileAbi.BACKGROUND_SIZE_WIDTH
internal const val BACKGROUND_SIZE_HEIGHT = MobileAbi.BACKGROUND_SIZE_HEIGHT
internal const val BACKGROUND_REPEAT = MobileAbi.BACKGROUND_REPEAT_REPEAT
internal const val BACKGROUND_NO_REPEAT = MobileAbi.BACKGROUND_REPEAT_NO_REPEAT
internal const val BACKGROUND_SPACE = MobileAbi.BACKGROUND_REPEAT_SPACE
internal const val BACKGROUND_ROUND = MobileAbi.BACKGROUND_REPEAT_ROUND
internal const val BACKGROUND_BOX_BORDER = MobileAbi.BACKGROUND_BOX_BORDER
internal const val BACKGROUND_BOX_PADDING = MobileAbi.BACKGROUND_BOX_PADDING
internal const val BACKGROUND_BOX_CONTENT = MobileAbi.BACKGROUND_BOX_CONTENT
internal const val BACKGROUND_BOX_BORDER_AREA = MobileAbi.BACKGROUND_BOX_BORDER_AREA
internal const val BACKGROUND_ATTACHMENT_SCROLL = MobileAbi.BACKGROUND_ATTACHMENT_SCROLL
internal const val BACKGROUND_BLEND_NORMAL = MobileAbi.BACKGROUND_BLEND_NORMAL
