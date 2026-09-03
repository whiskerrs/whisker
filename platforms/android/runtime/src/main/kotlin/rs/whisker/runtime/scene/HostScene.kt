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
    private val presentationPool = HashMap<Int, ArrayDeque<rs.whisker.runtime.WhiskerMountedElement>>()
    private var sceneEpoch = 0
    private var revision = 0L
    private var stagedSceneEpoch = 0
    private var stagedTargetRevision = 0L
    private var stagedSnapshot = false
    private val stagedOperations = ArrayList<HostSceneOperation>()
    private var applyingFrame = false
    private val deferredEvents = ArrayList<() -> Unit>()
    private val pointerCaptures = HashMap<Long, Long>()

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
        if (!validateStagedFrame()) {
            Log.e(
                "WhiskerView",
                "Frame validation failed: snapshot=$stagedSnapshot, revision=$stagedTargetRevision, operations=${stagedOperations.size}",
            )
            return false
        }
        return try {
            applyingFrame = true
            val zOrderParents = if (stagedSnapshot) {
                null
            } else {
                affectedZOrderParents()
            }
            if (stagedSnapshot) clear()
            stagedOperations.forEach(::applyOperation)
            attachRoots()
            if (zOrderParents == null) {
                refreshAllZOrderProjections()
            } else if (zOrderParents.isNotEmpty()) {
                zOrderParents.forEach(::refreshZOrderProjection)
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
        nodes.values.toList().forEach(::releasePresentation)
        nodes.clear()
        parents.clear()
        pointerCaptures.clear()
        root.parent?.requestDisallowInterceptTouchEvent(false)
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
            OP_CREATE -> {
                if (
                    operation.node == 0L || !existing.add(operation.node) ||
                    WhiskerElementRegistry.registration(operation.member) == null
                ) return false
                elementTypes[operation.node] = operation.member
            }
            OP_DELETE -> {
                if (!existing.remove(operation.node)) return false
                elementTypes.remove(operation.node)
                stagedParents.entries.removeAll {
                    it.key == operation.node || it.value == operation.node
                }
            }
            OP_INSERT -> {
                val policy = elementTypes[operation.parent]
                    ?.let(WhiskerElementRegistry::registration)?.childPolicy
                if (
                    operation.parent !in existing || operation.child !in existing ||
                    stagedParents.containsKey(operation.child) ||
                    policy != WhiskerChildPolicy.Elements
                ) return false
                stagedParents[operation.child] = operation.parent
            }
            OP_REMOVE -> if (stagedParents.remove(operation.child) != operation.parent) return false
            OP_MOVE -> if (stagedParents[operation.child] != operation.parent) return false
            OP_LAYOUT -> if (operation.node !in existing || operation.numbers?.size ?: 0 < 8) return false
            OP_PAINT -> if (
                operation.node !in existing || operation.numbers?.size ?: 0 < 53 ||
                operation.names?.size ?: 0 < 5
            ) return false
            OP_CLIP, OP_Z_ORDER, OP_CLEAR_PROPERTY, OP_EVENT_MASK ->
                if (operation.node !in existing) return false
            OP_HIT_TEST -> if (operation.node !in existing || operation.integer !in 0..3) return false
            OP_CURSOR -> if (operation.node !in existing || operation.integer !in 0..34) return false
            OP_CAPTURE, OP_RELEASE_CAPTURE -> if (
                operation.node !in existing || operation.wide == 0L
            ) return false
            OP_BOX_SHADOWS -> if (!validBoxShadows(operation, existing)) return false
            OP_CLIP_PATH -> if (!validClipPath(operation, existing)) return false
            OP_BACKDROP_BLUR -> if (
                operation.node !in existing || !operation.scalar.isFinite() ||
                operation.scalar < 0f || (operation.scalar > 0f && Build.VERSION.SDK_INT < 31)
            ) return false
            OP_IMAGE_RENDERING -> if (operation.node !in existing || operation.integer !in 0..2) return false
            OP_TRANSFORM -> if (
                operation.node !in existing ||
                !isProjectableFlatPlaneTransform(operation.numbers ?: return false)
            ) return false
            OP_OPACITY -> if (
                operation.node !in existing || !operation.scalar.isFinite() ||
                operation.scalar !in 0f..1f
            ) return false
            OP_VISIBILITY -> if (operation.node !in existing || operation.integer !in 0..1) return false
            OP_TEXT, OP_TEXT_STYLE -> {
                val values = operation.numbers ?: return false
                val registration = elementTypes[operation.node]
                    ?.let(WhiskerElementRegistry::registration) ?: return false
                if (
                    operation.node !in existing || operation.text == null ||
                    values.size < 37 || operation.names?.size ?: 0 < 3 ||
                    !values.all { it.isFinite() } || values[17].toInt() !in 0..2 ||
                    values[17] != values[17].toInt().toFloat() ||
                    values[18].toInt() !in 0..4 ||
                    values[18] != values[18].toInt().toFloat() ||
                    values[24].toInt() !in 0..4 ||
                    values[24] != values[24].toInt().toFloat() ||
                    values[36].toInt() !in 0..2 ||
                    values[36] != values[36].toInt().toFloat()
                ) return false
                if (operation.tag == OP_TEXT && registration.childPolicy != WhiskerChildPolicy.PlainText) return false
                if (operation.tag == OP_TEXT_STYLE && !registration.textStyle) return false
            }
            OP_PROPERTY, OP_COMMAND, OP_ACCESSIBILITY ->
                if (operation.node !in existing || operation.value == null) return false
            OP_BACKGROUND_LAYERS -> if (!validBackgroundLayers(operation, existing, rasterResources)) return false
            else -> return false
        }
        return true
    }

    private fun applyOperation(operation: HostSceneOperation) {
        val id = operation.node
        when (operation.tag) {
            OP_CREATE -> {
                val registration = requireNotNull(
                    WhiskerElementRegistry.registration(operation.member),
                )
                val eventSink: rs.whisker.runtime.WhiskerElementEventSink = { event, detail ->
                    emitElementEvent(id, event.name, detail)
                }
                val mounted = presentationPool[operation.member]?.pollFirst()?.also {
                    it.prepareForReuse(eventSink)
                } ?: requireNotNull(
                    WhiskerElementRegistry.mount(operation.member, context, eventSink),
                )
                val node = HostNode(context, registration.name, root as? WhiskerView)
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
            OP_DELETE -> deleteNode(id)
            OP_INSERT, OP_MOVE -> insertChild(operation.parent, operation.child, operation.index)
            OP_REMOVE -> detachChild(operation.parent, operation.child)
            OP_LAYOUT -> applyLayout(id, nodes[id] ?: return, requireNotNull(operation.numbers))
            OP_PAINT -> applyPaint(
                nodes[id] ?: return,
                HostBoxPaint(requireNotNull(operation.numbers), requireNotNull(operation.names)),
            )
            OP_CLIP -> (nodes[id] ?: return).setDescendantClip(
                horizontal = operation.flags and 1 != 0,
                vertical = operation.flags and 2 != 0,
            )
            OP_TRANSFORM -> (nodes[id] ?: return).setLocalTransform(
                requireNotNull(operation.numbers),
                root.resources.displayMetrics.density,
            )
            OP_OPACITY -> (nodes[id] ?: return).alpha = operation.scalar
            OP_VISIBILITY -> (nodes[id] ?: return).setWhiskerVisibility(operation.integer != 0)
            OP_Z_ORDER -> (nodes[id] ?: return).zOrder = operation.integer
            OP_TEXT -> applyText(
                nodes[id] ?: return,
                requireNotNull(operation.text),
                requireNotNull(operation.numbers),
                requireNotNull(operation.names),
            )
            OP_TEXT_STYLE -> applyText(
                nodes[id] ?: return,
                requireNotNull(operation.text),
                requireNotNull(operation.numbers),
                requireNotNull(operation.names),
                styleOnly = true,
            )
            OP_ACCESSIBILITY -> applyAccessibility(nodes[id] ?: return, requireNotNull(operation.value))
            OP_PROPERTY -> (nodes[id] ?: return).mountedElement
                ?.setProperty(operation.member, requireNotNull(operation.value))
            OP_CLEAR_PROPERTY -> (nodes[id] ?: return).mountedElement?.clearProperty(operation.member)
            OP_EVENT_MASK -> (nodes[id] ?: return).mountedElement?.setEventMask(operation.wide)
            OP_HIT_TEST -> (nodes[id] ?: return).setHitTestBehavior(operation.integer)
            OP_COMMAND -> (nodes[id] ?: return).mountedElement
                ?.invokeCommand(operation.member, requireNotNull(operation.value))
            OP_BACKGROUND_LAYERS -> applyBackgroundLayers(nodes[id] ?: return, operation)
            OP_BOX_SHADOWS -> applyBoxShadows(nodes[id] ?: return, operation)
            OP_CLIP_PATH -> applyClipPath(nodes[id] ?: return, operation)
            OP_BACKDROP_BLUR -> (nodes[id] ?: return).backdropBlur =
                operation.scalar * root.resources.displayMetrics.density
            OP_IMAGE_RENDERING -> {
                val node = nodes[id] ?: return
                node.imageRendering = HostImageRendering.fromWire(operation.integer) ?: return
                node.paint?.let { applyPaint(node, it) }
            }
            OP_CURSOR -> (nodes[id] ?: return).setCursorKeyword(operation.integer)
            OP_CAPTURE -> {
                pointerCaptures[operation.wide] = id
                root.parent?.requestDisallowInterceptTouchEvent(true)
            }
            OP_RELEASE_CAPTURE -> {
                if (pointerCaptures[operation.wide] == id) {
                    pointerCaptures.remove(operation.wide)
                    if (pointerCaptures.isEmpty()) {
                        root.parent?.requestDisallowInterceptTouchEvent(false)
                    }
                }
            }
        }
    }

    private fun applyAccessibility(node: HostNode, raw: WhiskerValue) {
        val value = (raw as? WhiskerValue.Map)?.value ?: return
        val state = (value["state"] as? WhiskerValue.Map)?.value.orEmpty()
        node.setAccessibility(
            HostAccessibility(
                label = (value["label"] as? WhiskerValue.Str)?.value,
                hint = (value["hint"] as? WhiskerValue.Str)?.value,
                role = (value["role"] as? WhiskerValue.Str)?.value,
                identifier = (value["identifier"] as? WhiskerValue.Str)?.value,
                hidden = (value["hidden"] as? WhiskerValue.Bool)?.value ?: false,
                modal = (value["modal"] as? WhiskerValue.Bool)?.value ?: false,
                disabled = (state["disabled"] as? WhiskerValue.Bool)?.value,
                selected = (state["selected"] as? WhiskerValue.Bool)?.value,
                checked = (state["checked"] as? WhiskerValue.Str)?.value,
                expanded = (state["expanded"] as? WhiskerValue.Bool)?.value,
            ),
        )
    }

    private fun attachRoots() {
        nodes.forEach { (id, node) ->
            if (!parents.containsKey(id) && node.parent !== root) {
                (node.parent as? ViewGroup)?.removeView(node)
                root.addView(node)
            }
        }
    }

    private fun affectedZOrderParents(): Set<Long?> {
        val affected = HashSet<Long?>()
        stagedOperations.forEach { operation ->
            when (operation.tag) {
                OP_CREATE -> affected += null
                OP_DELETE -> affected += parents[operation.node]
                OP_INSERT -> {
                    affected += null
                    affected += operation.parent
                }
                OP_REMOVE -> {
                    affected += operation.parent
                    affected += null
                }
                OP_MOVE -> affected += operation.parent
                OP_Z_ORDER -> affected += parents[operation.node]
            }
        }
        return affected
    }

    private fun refreshAllZOrderProjections() {
        val parentIds = HashSet<Long?>()
        parentIds += null
        parents.values.forEach(parentIds::add)
        parentIds.forEach(::refreshZOrderProjection)
    }

    /**
     * Projects CSS z-order into sibling order without using Android elevation.
     *
     * Kotlin's stable sort preserves the structural order established by insert/move for equal
     * z-order values. Reordering the actual children avoids the shadows Android draws for Views
     * with positive translationZ.
     */
    private fun refreshZOrderProjection(parentId: Long?) {
        val host = if (parentId == null) {
            root
        } else {
            val parent = nodes[parentId] ?: return
            parent.mountedElement?.childrenHost() ?: parent
        }
        val structuralOrder = buildList {
            repeat(host.childCount) { index ->
                (host.getChildAt(index) as? HostNode)?.let(::add)
            }
        }
        if (structuralOrder.isEmpty()) return

        // Clear state written by older Hosts and keep z-order independent from Android elevation.
        structuralOrder.forEach { it.translationZ = 0f }

        val desiredOrder = structuralOrder.sortedBy { it.zOrder }
        if (structuralOrder == desiredOrder) return

        desiredOrder.forEach(host::bringChildToFront)
        host.invalidate()
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
        val removedNodes = descendants.toSet() + id
        pointerCaptures.entries.removeAll { it.value in removedNodes }
        if (pointerCaptures.isEmpty()) root.parent?.requestDisallowInterceptTouchEvent(false)
        descendants.forEach { child ->
            nodes.remove(child)?.let(::releasePresentation)
            parents.remove(child)
        }
        parents.remove(id)
        (node.parent as? ViewGroup)?.removeView(node)
        releasePresentation(node)
    }

    private fun releasePresentation(node: HostNode) {
        (node.parent as? ViewGroup)?.removeView(node)
        val mounted = node.mountedElement ?: return
        (mounted.view.parent as? ViewGroup)?.removeView(mounted.view)
        node.mountedElement = null
        mounted.dispose()
        if (
            mounted.registration.name != "whisker.ui/View" &&
            mounted.registration.name != "whisker.ui/Text"
        ) return
        val pool = presentationPool.getOrPut(mounted.registration.elementType, ::ArrayDeque)
        if (pool.size < PRESENTATION_POOL_LIMIT_PER_TYPE) pool.addLast(mounted)
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
        refreshClipPath(node)
    }

    private fun applyText(
        node: HostNode,
        text: String,
        values: FloatArray,
        names: Array<String>,
        styleOnly: Boolean = false,
    ) {
        require(values.size >= 37)
        require(values[0].isFinite() && values[0] > 0f)
        require(values[1] in 1f..1000f && values[1] == values[1].toInt().toFloat())
        require(values[2] in 0f..2f && values[2] == values[2].toInt().toFloat())
        require(values[27] == 0f || values[27] == 1f)
        require(values[28] in 0f..2f && values[28] == values[28].toInt().toFloat())
        require(values[29] >= 0f && values[29] == values[29].toInt().toFloat())
        require(values[30] == 0f || values[30] == 1f)
        require(values[31] == 0f || values[31] == 1f)
        val featureCount = values[32].toInt()
        require(featureCount >= 0 && values[32] == featureCount.toFloat())
        val familyCount = values[33].toInt()
        require(familyCount > 0 && values[33] == familyCount.toFloat())
        require(values[34].isFinite() && values[34] >= 0f)
        require(values[35].isFinite())
        require(names.size >= 3 + familyCount + featureCount)
        val families = names.slice(3 until 3 + familyCount)
        require(families.all(String::isNotEmpty))
        val settings = names.drop(3 + familyCount).map(::parseFontSetting)
        val features = settings.take(featureCount).map {
            WhiskerFontFeature(it.first, it.second.toLong())
        }
        val variations = settings.drop(featureCount).map {
            WhiskerFontVariation(it.first, it.second.toFloat())
        }
        val mounted = requireNotNull(node.mountedElement)
        val content = WhiskerTextContent(
                    value = text,
                    fontFamilies = families,
                    fontSize = values[0],
                    fontWeight = values[1].toInt(),
                    fontStyle = WhiskerFontStyle.entries[values[2].toInt()],
                    lineHeight = values[34].takeIf { it > 0f },
                    letterSpacing = values[35],
                    fontFeatures = features,
                    fontVariations = variations,
                    fontOpticalSizing = if (values[31] == 0f) {
                        WhiskerFontOpticalSizing.AUTO
                    } else {
                        WhiskerFontOpticalSizing.NONE
                    },
                    color = if (values[7] == 0f) {
                        parseNamedColor(names[0])
                    } else {
                        rgba(values[3], values[4], values[5], values[6])
                    },
                    direction = WhiskerTextDirection.entries[values[36].toInt()],
                    alignment = WhiskerTextAlignment.entries[values[24].toInt()],
                    indent = WhiskerTextIndent(
                        logicalPixels = values[25],
                        percentage = values[26],
                    ),
                    wrap = values[27] != 0f,
                    wordBreak = WhiskerTextWordBreak.entries[values[28].toInt()],
                    maxLines = values[29].toInt(),
                    overflow = WhiskerTextOverflow.entries[values[30].toInt()],
                    decoration = if (values[17] == 0f) null else WhiskerTextDecoration(
                        line = if (values[17].toInt() and 1 != 0) {
                            WhiskerTextDecorationLine.UNDERLINE
                        } else {
                            WhiskerTextDecorationLine.LINE_THROUGH
                        },
                        style = WhiskerTextDecorationStyle.entries[values[18].toInt()],
                        color = if (values[23] == 0f) {
                            parseNamedColor(names[2])
                        } else {
                            rgba(values[19], values[20], values[21], values[22])
                        },
                    ),
                    shadow = if (values[8] == 0f) null else WhiskerTextShadow(
                        offsetX = values[9],
                        offsetY = values[10],
                        blurRadius = values[11],
                        color = if (values[16] == 0f) {
                            parseNamedColor(names[1])
                        } else {
                            rgba(values[12], values[13], values[14], values[15])
                        },
                    ),
                )
        require(if (styleOnly) mounted.setTextStyle(content.styleSnapshot()) else mounted.setText(content)) {
            "text operation sent to element ${mounted.registration.name} without the declared text implementation"
        }
    }

    private fun parseFontSetting(value: String): Pair<String, Double> {
        val separator = value.indexOf('=')
        require(separator == 4)
        return value.substring(0, separator) to value.substring(separator + 1).toDouble()
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
            node.imageRendering,
            RectF(
                node.geometry.contentX,
                node.geometry.contentY,
                node.geometry.contentX + node.geometry.contentWidth,
                node.geometry.contentY + node.geometry.contentHeight,
            ),
        )
        node.setOverflowClipGeometry(geometry)
        refreshClipPath(node)
    }

    private fun applyBackgroundLayers(node: HostNode, operation: HostSceneOperation) {
        val numbers = operation.numbers ?: FloatArray(0)
        if (numbers.isEmpty()) {
            node.backgroundLayers = null
        } else {
            node.backgroundLayers = HostBackgroundLayers(
                requireNotNull(projectedBackgroundLayerOperations(operation)).map {
                    decodeBackgroundLayer(
                        it,
                        root.resources.displayMetrics.density,
                        rasterResources,
                    )
                },
            )
        }
        node.paint?.let { applyPaint(node, it) }
    }

    private fun applyBoxShadows(node: HostNode, operation: HostSceneOperation) {
        val values = requireNotNull(operation.numbers)
        val names = requireNotNull(operation.names)
        node.boxShadows = values.indices.step(BOX_SHADOW_PACKED_SIZE).map { offset ->
            HostBoxShadow(
                offsetX = values[offset] * root.resources.displayMetrics.density,
                offsetY = values[offset + 1] * root.resources.displayMetrics.density,
                blurRadius = values[offset + 2] * root.resources.displayMetrics.density,
                spreadRadius = values[offset + 3] * root.resources.displayMetrics.density,
                inset = values[offset + 4] != 0f,
                color = if (values[offset + 5] == 0f) {
                    parseNamedColor(names[offset / BOX_SHADOW_PACKED_SIZE])
                } else {
                    rgba(
                        values[offset + 6],
                        values[offset + 7],
                        values[offset + 8],
                        values[offset + 9],
                    )
                },
            )
        }
        node.invalidate()
        (node.parent as? View)?.invalidate()
    }

    private fun applyClipPath(node: HostNode, operation: HostSceneOperation) {
        val values = operation.numbers ?: FloatArray(0)
        node.clipPath = if (values.isEmpty()) {
            null
        } else {
            val density = root.resources.displayMetrics.density
            fun coordinates(offset: Int) = (0 until 4).map { index ->
                val cursor = offset + index * 2
                HostPaintCoordinate(values[cursor] * density, values[cursor + 1])
            }
            val referenceBox = when (values[0].toInt()) {
                BACKGROUND_BOX_PADDING -> HostClipReferenceBox.Padding
                BACKGROUND_BOX_CONTENT -> HostClipReferenceBox.Content
                else -> HostClipReferenceBox.Border
            }
            when (values[1].toInt()) {
                CLIP_SHAPE_CIRCLE -> HostCircleClipPath(
                    referenceBox, coordinate(values, 2, density),
                    coordinate(values, 4, density), coordinate(values, 6, density),
                )
                CLIP_SHAPE_ELLIPSE -> HostEllipseClipPath(
                    referenceBox, coordinate(values, 2, density), coordinate(values, 4, density),
                    coordinate(values, 6, density), coordinate(values, 8, density),
                )
                CLIP_SHAPE_PATH -> {
                    val commandCount = values[3].toInt()
                    HostPathClipPath(
                        referenceBox = referenceBox,
                        evenOdd = values[2].toInt() == FILL_RULE_EVEN_ODD,
                        commands = (0 until commandCount).map { commandIndex ->
                            val offset = CLIP_PATH_HEADER_SIZE + commandIndex * PATH_COMMAND_PACKED_SIZE
                            HostPathCommand(
                                kind = values[offset].toInt(),
                                points = (0 until 6).map { pointIndex ->
                                    coordinate(values, offset + 1 + pointIndex * 2, density)
                                },
                            )
                        },
                    )
                }
                else -> HostInsetClipPath(
                    referenceBox = referenceBox,
                    edges = coordinates(2),
                    radiiHorizontal = coordinates(10),
                    radiiVertical = coordinates(18),
                )
            }
        }
        refreshClipPath(node)
    }

    private fun coordinate(values: FloatArray, offset: Int, density: Float) =
        HostPaintCoordinate(values[offset] * density, values[offset + 1])

    private fun refreshClipPath(node: HostNode) {
        val clip = node.clipPath
        if (clip == null) {
            node.setPaintClipPath(null)
            return
        }
        val density = root.resources.displayMetrics.density
        node.setPaintClipPath(
            resolveClipPath(
                clip,
                node.geometry.width * density,
                node.geometry.height * density,
                node.resolvedBorderWidths(),
                RectF(
                    node.geometry.contentX * density,
                    node.geometry.contentY * density,
                    (node.geometry.contentX + node.geometry.contentWidth) * density,
                    (node.geometry.contentY + node.geometry.contentHeight) * density,
                ),
            ),
        )
    }

}

private const val PRESENTATION_POOL_LIMIT_PER_TYPE = 128
