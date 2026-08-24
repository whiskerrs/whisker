package rs.whisker.runtime.scene

import android.content.Context
import android.graphics.RectF
import android.util.Log
import android.view.View
import android.view.ViewGroup
import rs.whisker.runtime.WhiskerChildPolicy
import rs.whisker.runtime.WhiskerContainerView
import rs.whisker.runtime.WhiskerElementRegistry
import rs.whisker.runtime.WhiskerTextContent
import rs.whisker.runtime.WhiskerValue
import rs.whisker.runtime.paint.HostBackgroundGeometry
import rs.whisker.runtime.paint.HostBackgroundBox
import rs.whisker.runtime.paint.HostBackgroundLayer
import rs.whisker.runtime.paint.HostBoxPaint
import rs.whisker.runtime.paint.HostBackgroundLayers
import rs.whisker.runtime.paint.HostBackgroundRepeat
import rs.whisker.runtime.paint.HostBackgroundSize
import rs.whisker.runtime.paint.HostConicGradient
import rs.whisker.runtime.paint.HostGradientStop
import rs.whisker.runtime.paint.HostLinearGradient
import rs.whisker.runtime.paint.HostPaintCoordinate
import rs.whisker.runtime.paint.HostRadialGradient
import rs.whisker.runtime.paint.HostRasterResourceStore
import rs.whisker.runtime.paint.applyBoxPaint
import rs.whisker.runtime.paint.parseNamedColor
import rs.whisker.runtime.paint.rgba
import kotlin.math.min

internal data class HostSceneOperation(
    val tag: Int,
    val flags: Int,
    val node: Long,
    val parent: Long,
    val child: Long,
    val index: Int,
    val member: Int,
    val integer: Int,
    val scalar: Float,
    val wide: Long,
    val numbers: FloatArray?,
    val text: String?,
    val names: Array<String>?,
    val value: WhiskerValue?,
)

/** Owns the transactional Android projection of one Whisker surface. */
internal class HostScene(
    private val root: WhiskerContainerView,
    private val context: Context,
    private val emitElementEvent: (Long, String, WhiskerValue) -> Unit,
    private val rasterResources: HostRasterResourceStore,
) {
    private val nodes = LinkedHashMap<Long, HostNode>()
    private val parents = HashMap<Long, Long>()
    private var sceneEpoch = 0
    private var revision = 0L
    private var stagedSceneEpoch = 0
    private var stagedTargetRevision = 0L
    private var stagedSnapshot = false
    private val stagedOperations = ArrayList<HostSceneOperation>()
    private var applyingFrame = false
    private val deferredEvents = ArrayList<() -> Unit>()

    /** 0 stages a transaction, 1 asks Rust for a snapshot, 2 rejects. */
    fun beginFrame(mode: Int, epoch: Int, baseRevision: Long, targetRevision: Long): Int {
        if (mode == 1 && (epoch != sceneEpoch || baseRevision != revision)) return 1
        if (mode == 0 && baseRevision != 0L) return 2
        stagedSnapshot = mode == 0
        stagedSceneEpoch = epoch
        stagedTargetRevision = targetRevision
        stagedOperations.clear()
        return 0
    }

    fun currentRevision(): Long = revision

    fun stage(operation: HostSceneOperation): Boolean {
        stagedOperations += operation
        return true
    }

    fun commit(): Boolean {
        if (!validateStagedFrame()) return false
        return try {
            applyingFrame = true
            if (stagedSnapshot) clear()
            stagedOperations.forEach(::applyOperation)
            attachRoots()
            if (stagedOperations.any { it.tag in 1..5 || it.tag == 12 }) {
                refreshZOrderProjection()
            }
            sceneEpoch = stagedSceneEpoch
            revision = stagedTargetRevision
            true
        } catch (error: Throwable) {
            Log.e("WhiskerView", "Frame commit failed", error)
            false
        } finally {
            applyingFrame = false
            val events = deferredEvents.toList()
            deferredEvents.clear()
            events.forEach { it() }
        }
    }

    fun dispatchOrDefer(event: () -> Unit) {
        if (applyingFrame) deferredEvents += event else event()
    }

    fun clear() {
        nodes.values.forEach { it.mountedElement?.dispose() }
        nodes.clear()
        parents.clear()
        root.removeAllViews()
    }

    private fun validateStagedFrame(): Boolean {
        val existing = if (stagedSnapshot) mutableSetOf() else nodes.keys.toMutableSet()
        val stagedParents = if (stagedSnapshot) HashMap() else HashMap(parents)
        val elementTypes = if (stagedSnapshot) {
            HashMap()
        } else {
            HashMap(nodes.mapValues { it.value.mountedElement!!.registration.elementType })
        }
        for (operation in stagedOperations) when (operation.tag) {
            1 -> {
                if (
                    operation.node == 0L || !existing.add(operation.node) ||
                    WhiskerElementRegistry.registration(operation.member) == null
                ) return false
                elementTypes[operation.node] = operation.member
            }
            2 -> {
                if (!existing.remove(operation.node)) return false
                elementTypes.remove(operation.node)
                stagedParents.entries.removeAll {
                    it.key == operation.node || it.value == operation.node
                }
            }
            3 -> {
                val policy = elementTypes[operation.parent]
                    ?.let(WhiskerElementRegistry::registration)?.childPolicy
                if (
                    operation.parent !in existing || operation.child !in existing ||
                    stagedParents.containsKey(operation.child) ||
                    policy != WhiskerChildPolicy.Elements
                ) return false
                stagedParents[operation.child] = operation.parent
            }
            4 -> if (stagedParents.remove(operation.child) != operation.parent) return false
            5 -> if (stagedParents[operation.child] != operation.parent) return false
            6 -> if (operation.node !in existing || operation.numbers?.size ?: 0 < 8) return false
            7 -> if (
                operation.node !in existing || operation.numbers?.size ?: 0 < 53 ||
                operation.names?.size ?: 0 < 5
            ) return false
            8, 12, 15, 16 -> if (operation.node !in existing) return false
            9 -> if (
                operation.node !in existing ||
                !isSupported2dTransform(operation.numbers ?: return false)
            ) return false
            10 -> if (
                operation.node !in existing || !operation.scalar.isFinite() ||
                operation.scalar !in 0f..1f
            ) return false
            11 -> if (operation.node !in existing || operation.integer !in 0..1) return false
            13 -> if (
                operation.node !in existing || operation.text == null ||
                operation.numbers?.size ?: 0 < 8 || operation.names?.isEmpty() != false
            ) return false
            14 -> if (operation.node !in existing || operation.value == null) return false
            21 -> if (!validBackgroundLayers(operation, existing)) return false
            else -> return false
        }
        return true
    }

    private fun applyOperation(operation: HostSceneOperation) {
        val id = operation.node
        when (operation.tag) {
            1 -> {
                val registration = requireNotNull(
                    WhiskerElementRegistry.registration(operation.member),
                )
                val mounted = requireNotNull(
                    WhiskerElementRegistry.mount(operation.member, context) { event, detail ->
                        emitElementEvent(id, event.name, detail)
                    },
                )
                val node = HostNode(context, registration.name)
                node.mountedElement = mounted
                node.addView(
                    mounted.view,
                    ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.MATCH_PARENT,
                    ),
                )
                nodes[id] = node
            }
            2 -> deleteNode(id)
            3, 5 -> insertChild(operation.parent, operation.child, operation.index)
            4 -> detachChild(operation.parent, operation.child)
            6 -> applyLayout(id, nodes[id] ?: return, requireNotNull(operation.numbers))
            7 -> applyPaint(
                nodes[id] ?: return,
                HostBoxPaint(requireNotNull(operation.numbers), requireNotNull(operation.names)),
            )
            8 -> (nodes[id] ?: return).setDescendantClip(
                horizontal = operation.flags and 1 != 0,
                vertical = operation.flags and 2 != 0,
            )
            9 -> (nodes[id] ?: return).setLocalTransform(
                requireNotNull(operation.numbers),
                root.resources.displayMetrics.density,
            )
            10 -> (nodes[id] ?: return).alpha = operation.scalar
            11 -> (nodes[id] ?: return).visibility =
                if (operation.integer != 0) View.VISIBLE else View.INVISIBLE
            12 -> (nodes[id] ?: return).zOrder = operation.integer
            13 -> applyText(
                nodes[id] ?: return,
                requireNotNull(operation.text),
                requireNotNull(operation.numbers),
                requireNotNull(operation.names),
            )
            14 -> (nodes[id] ?: return).mountedElement
                ?.setProperty(operation.member, requireNotNull(operation.value))
            15 -> (nodes[id] ?: return).mountedElement?.clearProperty(operation.member)
            16 -> (nodes[id] ?: return).mountedElement?.setEventMask(operation.wide)
            21 -> applyBackgroundLayers(nodes[id] ?: return, operation)
        }
    }

    private fun attachRoots() {
        nodes.forEach { (id, node) ->
            if (!parents.containsKey(id) && node.parent !== root) {
                (node.parent as? ViewGroup)?.removeView(node)
                root.addView(node)
            }
        }
    }

    /** Preserves exact signed i32 ordering while projecting onto Android's Float Z axis. */
    private fun refreshZOrderProjection() {
        nodes.entries.groupBy { parents[it.key] }.values.forEach { siblings ->
            val ranks = siblings.map { it.value.zOrder }.distinct().sorted()
                .withIndex().associate { (rank, value) -> value to rank.toFloat() }
            siblings.forEach { (_, node) -> node.translationZ = requireNotNull(ranks[node.zOrder]) }
        }
    }

    private fun insertChild(parentId: Long, childId: Long, requestedIndex: Int) {
        val parent = nodes[parentId] ?: return
        val child = nodes[childId] ?: return
        val mounted = requireNotNull(parent.mountedElement)
        require(mounted.registration.childPolicy == WhiskerChildPolicy.Elements) {
            "${mounted.registration.name} does not accept element children"
        }
        (child.parent as? ViewGroup)?.removeView(child)
        parents[childId] = parentId
        val childHost = mounted.childrenHost()
        if (childHost != null) {
            childHost.addView(child, min(requestedIndex, childHost.childCount))
        } else {
            parent.addView(child, min(requestedIndex + 1, parent.childCount))
        }
    }

    private fun detachChild(parentId: Long, childId: Long) {
        val child = nodes[childId] ?: return
        (child.parent as? ViewGroup)?.removeView(child)
        parents.remove(childId)
        check(nodes.containsKey(parentId))
    }

    private fun deleteNode(id: Long) {
        val node = nodes.remove(id) ?: return
        val descendants = nodes.keys.filter { candidate -> isDescendant(candidate, id) }
        descendants.forEach { child ->
            nodes.remove(child)?.mountedElement?.dispose()
            parents.remove(child)
        }
        parents.remove(id)
        (node.parent as? ViewGroup)?.removeView(node)
        node.mountedElement?.dispose()
    }

    private fun isDescendant(candidate: Long, ancestor: Long): Boolean {
        var current = parents[candidate]
        while (current != null) {
            if (current == ancestor) return true
            current = parents[current]
        }
        return false
    }

    private fun applyLayout(id: Long, node: HostNode, values: FloatArray) {
        require(values.size >= 8)
        val density = root.resources.displayMetrics.density
        node.geometry.apply {
            x = values[0]
            y = values[1]
            width = values[2]
            height = values[3]
            contentX = values[4]
            contentY = values[5]
            contentWidth = values[6]
            contentHeight = values[7]
        }
        val parentNode = parents[id]?.let(nodes::get)
        val customHost = parentNode?.mountedElement?.childrenHost() != null
        node.x = (node.geometry.x - if (customHost) parentNode!!.geometry.contentX else 0f) * density
        node.y = (node.geometry.y - if (customHost) parentNode!!.geometry.contentY else 0f) * density
        node.layoutParams = (node.layoutParams ?: ViewGroup.LayoutParams(0, 0)).apply {
            width = (node.geometry.width * density).toInt().coerceAtLeast(0)
            height = (node.geometry.height * density).toInt().coerceAtLeast(0)
        }
        node.mountedElement?.view?.let { content ->
            content.x = node.geometry.contentX * density
            content.y = node.geometry.contentY * density
            content.layoutParams = (content.layoutParams ?: ViewGroup.LayoutParams(0, 0)).apply {
                width = (node.geometry.contentWidth * density).toInt().coerceAtLeast(0)
                height = (node.geometry.contentHeight * density).toInt().coerceAtLeast(0)
            }
        }
        node.paint?.let { applyPaint(node, it) }
    }

    private fun applyText(node: HostNode, text: String, values: FloatArray, names: Array<String>) {
        require(values.size >= 8)
        val mounted = requireNotNull(node.mountedElement)
        require(
            mounted.setText(
                WhiskerTextContent(
                    value = text,
                    fontSize = values[0],
                    fontWeight = values[1].toInt(),
                    color = if (values[7] == 0f) {
                        parseNamedColor(names[0])
                    } else {
                        rgba(values[3], values[4], values[5], values[6])
                    },
                ),
            ),
        ) {
            "text operation sent to element ${mounted.registration.name} without a text implementation"
        }
    }

    private fun applyPaint(node: HostNode, paint: HostBoxPaint) {
        node.paint = paint
        val geometry = applyBoxPaint(
            node,
            paint,
            node.geometry.width,
            node.geometry.height,
            root.resources.displayMetrics.density,
            node.backgroundLayers,
            RectF(
                node.geometry.contentX,
                node.geometry.contentY,
                node.geometry.contentX + node.geometry.contentWidth,
                node.geometry.contentY + node.geometry.contentHeight,
            ),
        )
        node.setOverflowClipGeometry(geometry)
    }

    private fun applyBackgroundLayers(node: HostNode, operation: HostSceneOperation) {
        val numbers = operation.numbers ?: FloatArray(0)
        if (numbers.isEmpty()) {
            node.backgroundLayers = null
        } else {
            node.backgroundLayers = HostBackgroundLayers(
                requireNotNull(projectedBackgroundLayerOperations(operation)).map {
                    decodeBackgroundLayer(it)
                },
            )
        }
        node.paint?.let { applyPaint(node, it) }
    }

    private fun decodeBackgroundLayer(operation: HostSceneOperation): HostBackgroundLayer {
        val density = root.resources.displayMetrics.density
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

    private fun projectedBackgroundLayerOperations(
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

    private fun packedCount(value: Float, maximum: Int): Int? {
        if (!value.isFinite() || value < 0f || value > maximum.toFloat()) return null
        val integer = value.toInt()
        return integer.takeIf { it.toFloat() == value }
    }

    private fun decodeResourceId(numbers: FloatArray, offset: Int): Long? {
        if (offset + BACKGROUND_RESOURCE_ID_WORDS > numbers.size) return null
        var resourceId = 0L
        repeat(BACKGROUND_RESOURCE_ID_WORDS) { wordIndex ->
            val word = packedCount(numbers[offset + wordIndex], RESOURCE_ID_WORD_MAX) ?: return null
            resourceId = resourceId or (word.toLong() shl (wordIndex * RESOURCE_ID_WORD_BITS))
        }
        return resourceId.takeIf { it != 0L }
    }

    private fun decodeGradientStops(
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

    private fun backgroundRepeat(value: Float): HostBackgroundRepeat =
        when (value) {
            BACKGROUND_REPEAT.toFloat() -> HostBackgroundRepeat.Repeat
            BACKGROUND_NO_REPEAT.toFloat() -> HostBackgroundRepeat.NoRepeat
            BACKGROUND_SPACE.toFloat() -> HostBackgroundRepeat.Space
            else -> HostBackgroundRepeat.Round
        }

    private fun backgroundSize(value: Float): HostBackgroundSize =
        when (value) {
            BACKGROUND_SIZE_EXPLICIT.toFloat() -> HostBackgroundSize.Explicit
            BACKGROUND_SIZE_COVER.toFloat() -> HostBackgroundSize.Cover
            BACKGROUND_SIZE_CONTAIN.toFloat() -> HostBackgroundSize.Contain
            BACKGROUND_SIZE_WIDTH.toFloat() -> HostBackgroundSize.Width
            BACKGROUND_SIZE_HEIGHT.toFloat() -> HostBackgroundSize.Height
            else -> HostBackgroundSize.Auto
        }

    private fun backgroundBox(value: Float): HostBackgroundBox =
        when (value) {
            BACKGROUND_BOX_BORDER.toFloat() -> HostBackgroundBox.Border
            BACKGROUND_BOX_PADDING.toFloat() -> HostBackgroundBox.Padding
            BACKGROUND_BOX_BORDER_AREA.toFloat() -> HostBackgroundBox.BorderArea
            else -> HostBackgroundBox.Content
        }

    private fun validBackgroundLayers(
        operation: HostSceneOperation,
        existing: Set<Long>,
    ): Boolean {
        if (operation.node !in existing) return false
        val numbers = operation.numbers ?: FloatArray(0)
        val names = operation.names ?: emptyArray()
        if (numbers.isEmpty()) {
            return operation.flags == BACKGROUND_LINEAR &&
                operation.scalar.isFinite() && names.isEmpty()
        }
        return projectedBackgroundLayerOperations(operation)
            ?.all(::validBackgroundLayer) == true
    }

    private fun validBackgroundLayer(operation: HostSceneOperation): Boolean {
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

    private fun validBackgroundGeometry(numbers: FloatArray): Boolean {
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

    private fun validBackgroundSize(value: Float): Boolean =
        value == BACKGROUND_SIZE_AUTO.toFloat() ||
            value == BACKGROUND_SIZE_EXPLICIT.toFloat() ||
            value == BACKGROUND_SIZE_COVER.toFloat() ||
            value == BACKGROUND_SIZE_CONTAIN.toFloat() ||
            value == BACKGROUND_SIZE_WIDTH.toFloat() ||
            value == BACKGROUND_SIZE_HEIGHT.toFloat()

    private fun validBackgroundRepeat(value: Float): Boolean =
        value == BACKGROUND_REPEAT.toFloat() ||
            value == BACKGROUND_NO_REPEAT.toFloat() ||
            value == BACKGROUND_SPACE.toFloat() ||
            value == BACKGROUND_ROUND.toFloat()

    private fun validBackgroundOrigin(value: Float): Boolean =
        value == BACKGROUND_BOX_BORDER.toFloat() ||
            value == BACKGROUND_BOX_PADDING.toFloat() ||
            value == BACKGROUND_BOX_CONTENT.toFloat()

    private fun validBackgroundClip(value: Float): Boolean =
        validBackgroundOrigin(value) || value == BACKGROUND_BOX_BORDER_AREA.toFloat()

    private companion object {
        const val BACKGROUND_GEOMETRY_PACKED_SIZE = 15
        const val BACKGROUND_GRADIENT_STOP_PACKED_SIZE = 7
        const val BACKGROUND_PACKED_LAYER_HEADER_SIZE = 3
        const val BACKGROUND_PACKED_LAYERS = 256
        const val MAX_BACKGROUND_LAYERS = 256
        const val BACKGROUND_RESOURCE_ID_WORDS = 4
        const val RESOURCE_ID_WORD_BITS = 16
        const val RESOURCE_ID_WORD_MAX = 0xffff
        const val BACKGROUND_LINEAR = 0
        const val BACKGROUND_RADIAL = 1
        const val BACKGROUND_CONIC = 2
        const val BACKGROUND_RESOURCE = 3
        const val BACKGROUND_SIZE_AUTO = 0
        const val BACKGROUND_SIZE_EXPLICIT = 1
        const val BACKGROUND_SIZE_COVER = 2
        const val BACKGROUND_SIZE_CONTAIN = 3
        const val BACKGROUND_SIZE_WIDTH = 4
        const val BACKGROUND_SIZE_HEIGHT = 5
        const val BACKGROUND_REPEAT = 0
        const val BACKGROUND_NO_REPEAT = 1
        const val BACKGROUND_SPACE = 2
        const val BACKGROUND_ROUND = 3
        const val BACKGROUND_BOX_BORDER = 0
        const val BACKGROUND_BOX_PADDING = 1
        const val BACKGROUND_BOX_CONTENT = 2
        const val BACKGROUND_BOX_BORDER_AREA = 3
        const val BACKGROUND_ATTACHMENT_SCROLL = 0
        const val BACKGROUND_BLEND_NORMAL = 0
    }
}
