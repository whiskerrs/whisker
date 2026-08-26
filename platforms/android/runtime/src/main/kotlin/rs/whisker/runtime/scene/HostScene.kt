package rs.whisker.runtime.scene

import android.content.Context
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
import rs.whisker.runtime.paint.HostBoxPaint
import rs.whisker.runtime.paint.HostBackgroundLayers
import rs.whisker.runtime.paint.HostBackgroundRepeat
import rs.whisker.runtime.paint.HostConicGradient
import rs.whisker.runtime.paint.HostGradientStop
import rs.whisker.runtime.paint.HostLinearGradient
import rs.whisker.runtime.paint.HostPaintCoordinate
import rs.whisker.runtime.paint.HostRadialGradient
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
        )
        node.setOverflowClipGeometry(geometry)
    }

    private fun applyBackgroundLayers(node: HostNode, operation: HostSceneOperation) {
        val numbers = operation.numbers ?: FloatArray(0)
        if (numbers.isEmpty()) {
            node.backgroundLayers = null
        } else {
            val density = root.resources.displayMetrics.density
            val names = requireNotNull(operation.names)
            val imageOffset = BACKGROUND_GEOMETRY_PACKED_SIZE
            val stopOffset = imageOffset + when (operation.flags) {
                BACKGROUND_RADIAL -> 8
                BACKGROUND_CONIC -> 4
                else -> 0
            }
            val stops = decodeGradientStops(numbers, stopOffset, names, density)
            fun coordinate(offset: Int) = HostPaintCoordinate(
                length = numbers[offset] * density,
                fraction = numbers[offset + 1],
            )
            val geometry = HostBackgroundGeometry(
                positionX = coordinate(0),
                positionY = coordinate(2),
                sizeWidth = if (numbers[8] == BACKGROUND_SIZE_EXPLICIT.toFloat()) {
                    coordinate(4)
                } else {
                    null
                },
                sizeHeight = if (numbers[8] == BACKGROUND_SIZE_EXPLICIT.toFloat()) {
                    coordinate(6)
                } else {
                    null
                },
                repeatX = backgroundRepeat(numbers[9]),
                repeatY = backgroundRepeat(numbers[10]),
                origin = backgroundBox(numbers[11]),
                clip = backgroundBox(numbers[12]),
            )
            node.backgroundLayers = if (operation.flags == BACKGROUND_RADIAL) {
                HostBackgroundLayers(
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
                HostBackgroundLayers(
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
                HostBackgroundLayers(
                    linearGradient = HostLinearGradient(operation.scalar, stops),
                    geometry = geometry,
                )
            }
        }
        node.paint?.let { applyPaint(node, it) }
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
            else -> HostBackgroundRepeat.Space
        }

    private fun backgroundBox(value: Float): HostBackgroundBox =
        if (value == BACKGROUND_BOX_BORDER.toFloat()) {
            HostBackgroundBox.Border
        } else {
            HostBackgroundBox.Padding
        }

    private fun validBackgroundLayers(
        operation: HostSceneOperation,
        existing: Set<Long>,
    ): Boolean {
        if (
            operation.node !in existing || operation.flags !in BACKGROUND_LINEAR..BACKGROUND_CONIC ||
            !operation.scalar.isFinite()
        ) {
            return false
        }
        val numbers = operation.numbers ?: FloatArray(0)
        val names = operation.names ?: emptyArray()
        if (numbers.isEmpty()) return operation.flags == BACKGROUND_LINEAR && names.isEmpty()
        if (numbers.size < BACKGROUND_GEOMETRY_PACKED_SIZE || !validBackgroundGeometry(numbers)) {
            return false
        }
        val stopOffset = BACKGROUND_GEOMETRY_PACKED_SIZE + when (operation.flags) {
            BACKGROUND_RADIAL -> 8
            BACKGROUND_CONIC -> 4
            else -> 0
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
        val supportedGeometry =
            (
                sizeKind == BACKGROUND_SIZE_AUTO.toFloat() &&
                    repeatX == BACKGROUND_REPEAT.toFloat() &&
                    repeatY == BACKGROUND_REPEAT.toFloat()
            ) ||
                (
                    sizeKind == BACKGROUND_SIZE_EXPLICIT.toFloat() &&
                        validBackgroundRepeat(repeatX) &&
                        validBackgroundRepeat(repeatY)
                )
        return supportedGeometry &&
            (sizeKind == BACKGROUND_SIZE_EXPLICIT.toFloat() || (0..3).all { numbers[it] == 0f }) &&
            if (sizeKind == BACKGROUND_SIZE_EXPLICIT.toFloat()) {
                validBackgroundBox(numbers[11]) && validBackgroundBox(numbers[12])
            } else {
                numbers[11] == BACKGROUND_BOX_PADDING.toFloat() &&
                    numbers[12] == BACKGROUND_BOX_BORDER.toFloat()
            } &&
            numbers[13] == BACKGROUND_ATTACHMENT_SCROLL.toFloat() &&
            numbers[14] == BACKGROUND_BLEND_NORMAL.toFloat()
    }

    private fun validBackgroundRepeat(value: Float): Boolean =
        value == BACKGROUND_REPEAT.toFloat() ||
            value == BACKGROUND_NO_REPEAT.toFloat() ||
            value == BACKGROUND_SPACE.toFloat()

    private fun validBackgroundBox(value: Float): Boolean =
        value == BACKGROUND_BOX_BORDER.toFloat() || value == BACKGROUND_BOX_PADDING.toFloat()

    private companion object {
        const val BACKGROUND_GEOMETRY_PACKED_SIZE = 15
        const val BACKGROUND_LINEAR = 0
        const val BACKGROUND_RADIAL = 1
        const val BACKGROUND_CONIC = 2
        const val BACKGROUND_SIZE_AUTO = 0
        const val BACKGROUND_SIZE_EXPLICIT = 1
        const val BACKGROUND_REPEAT = 0
        const val BACKGROUND_NO_REPEAT = 1
        const val BACKGROUND_SPACE = 2
        const val BACKGROUND_BOX_BORDER = 0
        const val BACKGROUND_BOX_PADDING = 1
        const val BACKGROUND_ATTACHMENT_SCROLL = 0
        const val BACKGROUND_BLEND_NORMAL = 0
    }
}
