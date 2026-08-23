package rs.whisker.runtime

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Choreographer
import android.view.View
import android.view.ViewGroup
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import rs.whisker.runtime.WhiskerElementRegistry
import rs.whisker.runtime.WhiskerMountedElement
import rs.whisker.runtime.WhiskerValue
import kotlin.math.min

/** The single Android View that owns a Whisker runtime and its native scene. */
class WhiskerView(context: Context) :
    WhiskerContainerView(context),
    Choreographer.FrameCallback,
    WhiskerRuntimeOwner {
    override val runtimeContext: Context
        get() = context

    private data class Geometry(
        var x: Float = 0f, var y: Float = 0f,
        var width: Float = 0f, var height: Float = 0f,
        var contentX: Float = 0f, var contentY: Float = 0f,
        var contentWidth: Float = 0f, var contentHeight: Float = 0f,
    )

    private data class Paint(val values: FloatArray, val names: Array<String>)

    private data class NativeOperation(
        val tag: Int, val flags: Int, val node: Long, val parent: Long, val child: Long,
        val index: Int, val member: Int, val integer: Int, val scalar: Float, val wide: Long,
        val numbers: FloatArray?, val text: String?, val names: Array<String>?, val value: WhiskerValue?,
    )

    private class Node(context: Context, val element: String) : WhiskerContainerView(context) {
        val geometry = Geometry()
        var paint: Paint? = null
        var mountedElement: WhiskerMountedElement? = null
    }

    private val nodes = LinkedHashMap<Long, Node>()
    private val parents = HashMap<Long, Long>()
    private val choreographer = Choreographer.getInstance()
    private var nativeHandle = 0L
    private var frameScheduled = false
    private var windowVisible = true
    private var sceneEpoch = 0
    private var revision = 0L
    private var stagedSceneEpoch = 0
    private var stagedTargetRevision = 0L
    private var stagedSnapshot = false
    private val stagedOperations = ArrayList<NativeOperation>()
    private val bootstrapRegistrations = ArrayList<WhiskerElementRegistration>()
    private var applyingFrame = false
    private val deferredEvents = ArrayList<() -> Unit>()

    init {
        WhiskerApplication.initialize(context)
        setBackgroundColor(Color.TRANSPARENT)
        clipChildren = false
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        WhiskerAppContext.pushRuntimeOwner(this)
        mountWhenSized()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        mountWhenSized()
        requestFrameFromNative()
    }

    private fun mountWhenSized() {
        if (nativeHandle == 0L && isAttachedToWindow && width > 0 && height > 0) {
            WhiskerModuleEventCenter.installEventSink { module, event, payload ->
                dispatchModuleEvent(module, event, payload)
            }
            val density = resources.displayMetrics.density
            nativeHandle = nativeCreate(width / density, height / density, density)
            if (nativeHandle != 0L) requestFrameFromNative()
        }
    }

    override fun onDetachedFromWindow() {
        if (nativeHandle != 0L) nativeDestroy(nativeHandle)
        nativeHandle = 0L
        frameScheduled = false
        choreographer.removeFrameCallback(this)
        WhiskerModuleEventCenter.installEventSink(null)
        clearScene()
        WhiskerAppContext.popRuntimeOwner(this)
        super.onDetachedFromWindow()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        windowVisible = visibility == View.VISIBLE
        if (windowVisible) {
            mountWhenSized()
            requestFrameFromNative()
        } else {
            frameScheduled = false
            choreographer.removeFrameCallback(this)
        }
    }

    /** Called from Rust through JNI. Safe even when the wake originates off-main. */
    fun requestFrameFromNative() {
        if (!isAttachedToWindow || !windowVisible || nativeHandle == 0L) return
        if (!isLaidOut || android.os.Looper.myLooper() != android.os.Looper.getMainLooper()) {
            post { requestFrameFromNative() }
            return
        }
        if (!frameScheduled) {
            frameScheduled = true
            choreographer.postFrameCallback(this)
        }
    }

    override fun doFrame(frameTimeNanos: Long) {
        frameScheduled = false
        val handle = nativeHandle
        if (handle == 0L) {
            mountWhenSized()
            return
        }
        val density = resources.displayMetrics.density
        val idle = nativeTick(
            handle,
            frameTimeNanos / 1_000_000.0,
            width / density,
            height / density,
            density,
        )
        if (!idle) requestFrameFromNative()
    }

    fun beginBootstrapFromNative() { bootstrapRegistrations.clear() }

    @Suppress("LongParameterList")
    fun registerElementFromNative(
        elementType: Int, name: String, childPolicy: Int, measurement: Int,
        propertyIds: IntArray, propertyKinds: IntArray, propertyNames: Array<String>,
        eventIds: IntArray, eventKinds: IntArray, eventNames: Array<String>,
        commandIds: IntArray, commandKinds: IntArray, commandNames: Array<String>,
    ) {
        val kinds = WhiskerValueKind.entries
        bootstrapRegistrations += WhiskerElementRegistration(
            elementType, name, WhiskerChildPolicy.entries[childPolicy], WhiskerMeasurement.entries[measurement],
            propertyIds.indices.map { WhiskerPropertyBinding(propertyIds[it], propertyNames[it], kinds[propertyKinds[it]]) },
            eventIds.indices.map { WhiskerEventBinding(eventIds[it], eventNames[it], if (eventKinds[it] < 0) null else kinds[eventKinds[it]]) },
            commandIds.indices.map { WhiskerCommandBinding(commandIds[it], commandNames[it], kinds[commandKinds[it]]) },
        )
    }

    fun finishBootstrapFromNative(): Boolean = WhiskerElementRegistry.bind(bootstrapRegistrations)

    /** 0 stages a transaction, 1 asks Rust for a snapshot, 2 rejects. */
    fun beginFrameFromNative(mode: Int, epoch: Int, baseRevision: Long, targetRevision: Long): Int {
        if (mode == 1 && (epoch != sceneEpoch || baseRevision != revision)) return 1
        if (mode == 0 && baseRevision != 0L) return 2
        stagedSnapshot = mode == 0
        stagedSceneEpoch = epoch
        stagedTargetRevision = targetRevision
        stagedOperations.clear()
        return 0
    }

    fun currentRevisionFromNative(): Long = revision

    @Suppress("LongParameterList")
    fun stageOperationFromNative(
        tag: Int, flags: Int, node: Long, parent: Long, child: Long,
        index: Int, member: Int, integer: Int, scalar: Float, wide: Long,
        numbers: FloatArray?, text: String?, names: Array<String>?, value: WhiskerValue?,
    ): Boolean {
        stagedOperations += NativeOperation(tag, flags, node, parent, child, index, member, integer, scalar, wide, numbers, text, names, value)
        return true
    }

    fun commitFrameFromNative(): Boolean {
        if (!validateStagedFrame()) return false
        return try {
            applyingFrame = true
            if (stagedSnapshot) clearScene()
            stagedOperations.forEach(::applyOperation)
            attachRoots()
            sceneEpoch = stagedSceneEpoch
            revision = stagedTargetRevision
            true
        } catch (error: Throwable) {
            android.util.Log.e("WhiskerView", "Frame commit failed", error)
            false
        } finally {
            applyingFrame = false
            val events = deferredEvents.toList()
            deferredEvents.clear()
            events.forEach { it() }
        }
    }

    private fun validateStagedFrame(): Boolean {
        val existing = if (stagedSnapshot) mutableSetOf() else nodes.keys.toMutableSet()
        val stagedParents = if (stagedSnapshot) HashMap() else HashMap(parents)
        val elementTypes = if (stagedSnapshot) HashMap() else HashMap(nodes.mapValues { it.value.mountedElement!!.registration.elementType })
        for (operation in stagedOperations) when (operation.tag) {
            1 -> {
                if (operation.node == 0L || !existing.add(operation.node) || WhiskerElementRegistry.registration(operation.member) == null) return false
                elementTypes[operation.node] = operation.member
            }
            2 -> {
                if (!existing.remove(operation.node)) return false
                elementTypes.remove(operation.node)
                stagedParents.entries.removeAll { it.key == operation.node || it.value == operation.node }
            }
            3 -> {
                val policy = elementTypes[operation.parent]?.let(WhiskerElementRegistry::registration)?.childPolicy
                if (operation.parent !in existing || operation.child !in existing || stagedParents.containsKey(operation.child) || policy != WhiskerChildPolicy.Elements) return false
                stagedParents[operation.child] = operation.parent
            }
            4 -> if (stagedParents.remove(operation.child) != operation.parent) return false
            5 -> if (stagedParents[operation.child] != operation.parent) return false
            6 -> if (operation.node !in existing || operation.numbers?.size ?: 0 < 8) return false
            7 -> if (operation.node !in existing || operation.numbers?.size ?: 0 < 41 || operation.names?.size ?: 0 < 5) return false
            8, 10, 11, 12, 15, 16 -> if (operation.node !in existing) return false
            13 -> if (operation.node !in existing || operation.text == null || operation.numbers?.size ?: 0 < 8 || operation.names?.isEmpty() != false) return false
            14 -> if (operation.node !in existing || operation.value == null) return false
            // Transform, hit-test, capture and commands are rejected until
            // their native behavior is implemented; never acknowledge a no-op.
            else -> return false
        }
        return true
    }

    private fun applyOperation(operation: NativeOperation) {
        val id = operation.node
        when (operation.tag) {
            1 -> {
                val registration = requireNotNull(WhiskerElementRegistry.registration(operation.member))
                val mounted = requireNotNull(WhiskerElementRegistry.mount(operation.member, context) { event, detail ->
                    dispatchElementEvent(id, event.name, detail)
                })
                val node = Node(context, registration.name)
                node.mountedElement = mounted
                node.addView(mounted.view, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
                nodes[id] = node
            }
            2 -> deleteNode(id)
            3, 5 -> insertChild(operation.parent, operation.child, operation.index)
            4 -> detachChild(operation.parent, operation.child)
            6 -> applyLayout(id, nodes[id] ?: return, requireNotNull(operation.numbers))
            7 -> applyPaint(nodes[id] ?: return, Paint(requireNotNull(operation.numbers), requireNotNull(operation.names)))
            8 -> (nodes[id] ?: return).clipChildren = operation.flags and 3 != 0
            10 -> (nodes[id] ?: return).alpha = operation.scalar
            11 -> (nodes[id] ?: return).visibility = if (operation.integer != 0) View.VISIBLE else View.INVISIBLE
            12 -> (nodes[id] ?: return).translationZ = operation.integer.toFloat()
            13 -> applyText(nodes[id] ?: return, requireNotNull(operation.text), requireNotNull(operation.numbers), requireNotNull(operation.names))
            14 -> (nodes[id] ?: return).mountedElement?.setProperty(operation.member, requireNotNull(operation.value))
            15 -> (nodes[id] ?: return).mountedElement?.clearProperty(operation.member)
            16 -> (nodes[id] ?: return).mountedElement?.setEventMask(operation.wide)
        }
    }

    private fun dispatchElementEvent(node: Long, name: String, detail: WhiskerValue) {
        if (applyingFrame) {
            deferredEvents += { dispatchElementEvent(node, name, detail) }
            return
        }
        val handle = nativeHandle
        if (handle == 0L) return
        nativeDispatchEvent(
            handle, node, name, detail, android.os.SystemClock.uptimeMillis().toDouble(),
        )
    }

    /** Called by the retained Rust runtime through its small JNI callback. */
    fun invokeModuleFromNative(
        module: String,
        method: String,
        args: Array<WhiskerValue>,
        isAsync: Boolean,
        callbackPtr: Long,
        userDataPtr: Long,
    ): Boolean = try {
        val settle: (WhiskerValue) -> Unit = { value ->
            nativeResolveModule(callbackPtr, userDataPtr, value)
        }
        if (isAsync && WhiskerModuleRegistry.invokeDispatchAsync(module, method, args, settle)) {
            true
        } else {
            settle(WhiskerModuleRegistry.invokeDispatch(module, method, args))
            true
        }
    } catch (error: Throwable) {
        nativeResolveModule(
            callbackPtr, userDataPtr,
            WhiskerValue.Err("module $module.$method failed: ${error.message ?: error.javaClass.simpleName}"),
        )
        true
    }

    /** Called by Rust on the first and last listener transition. */
    fun observeModuleFromNative(module: String, event: String, observing: Boolean) {
        if (observing) {
            WhiskerModuleEventCenter.fireStart(module, event)
        } else {
            WhiskerModuleEventCenter.fireStop(module, event)
        }
    }

    private fun dispatchModuleEvent(module: String, event: String, payload: WhiskerValue) {
        if (applyingFrame) {
            deferredEvents += { dispatchModuleEvent(module, event, payload) }
            return
        }
        val handle = nativeHandle
        if (handle == 0L) {
            // OnStartObserving can synchronously emit while nativeCreate is
            // still returning its handle. Retry once on the next main-loop
            // turn instead of dropping that initial value.
            post {
                val mountedHandle = nativeHandle
                if (mountedHandle != 0L) {
                    nativeDispatchModuleEvent(mountedHandle, module, event, payload)
                }
            }
            return
        }
        nativeDispatchModuleEvent(handle, module, event, payload)
    }

    private fun clearScene() {
        nodes.values.forEach { it.mountedElement?.dispose() }
        nodes.clear()
        parents.clear()
        removeAllViews()
    }

    private fun attachRoots() {
        nodes.forEach { (id, node) ->
            if (!parents.containsKey(id) && node.parent !== this) {
                (node.parent as? ViewGroup)?.removeView(node)
                addView(node)
            }
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
        val parent = nodes[parentId] ?: return
        val child = nodes[childId] ?: return
        (child.parent as? ViewGroup)?.removeView(child)
        parents.remove(childId)
    }

    private fun deleteNode(id: Long) {
        val node = nodes.remove(id) ?: return
        val descendants = nodes.keys.filter { candidate -> isDescendant(candidate, id) }
        descendants.forEach { child -> nodes.remove(child)?.mountedElement?.dispose(); parents.remove(child) }
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

    private fun applyLayout(id: Long, node: Node, values: FloatArray) {
        require(values.size >= 8)
        val density = resources.displayMetrics.density
        node.geometry.apply {
            x = values[0]; y = values[1]; width = values[2]; height = values[3]
            contentX = values[4]; contentY = values[5]; contentWidth = values[6]; contentHeight = values[7]
        }
        val parentNode = parents[id]?.let(nodes::get)
        val customHost = parentNode?.mountedElement?.childrenHost() != null
        node.x = (node.geometry.x - if (customHost) parentNode!!.geometry.contentX else 0f) * density
        node.y = (node.geometry.y - if (customHost) parentNode!!.geometry.contentY else 0f) * density
        node.layoutParams = (node.layoutParams ?: LayoutParams(0, 0)).apply {
            width = (node.geometry.width * density).toInt().coerceAtLeast(0)
            height = (node.geometry.height * density).toInt().coerceAtLeast(0)
        }
        node.mountedElement?.view?.let { content ->
            content.x = node.geometry.contentX * density
            content.y = node.geometry.contentY * density
            content.layoutParams = (content.layoutParams ?: LayoutParams(0, 0)).apply {
                width = (node.geometry.contentWidth * density).toInt().coerceAtLeast(0)
                height = (node.geometry.contentHeight * density).toInt().coerceAtLeast(0)
            }
        }
        node.paint?.let { applyPaint(node, it) }
    }

    private fun applyText(node: Node, text: String, values: FloatArray, names: Array<String>) {
        require(values.size >= 8)
        val mounted = requireNotNull(node.mountedElement)
        require(
            mounted.setText(
                WhiskerTextContent(
                    value = text, fontSize = values[0], fontWeight = values[1].toInt(),
                    color = if (values[7] == 0f) parseNamedColor(names[0]) else rgba(values[3], values[4], values[5], values[6]),
                ),
            ),
        ) {
            "text operation sent to element ${mounted.registration.name} without a text implementation"
        }
    }

    private fun applyPaint(node: Node, paint: Paint) {
        node.paint = paint
        val values = paint.values
        require(values.size >= 41)
        val density = resources.displayMetrics.density
        node.background = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            setColor(if (values[0] == 0f) parseNamedColor(paint.names[0]) else rgba(values[1], values[2], values[3], values[4]))
            val borderWidth = resolveLength(values[5], values[6], node.geometry.height) * density
            val borderColor = if (values[13] == 0f) parseNamedColor(paint.names[1]) else rgba(values[14], values[15], values[16], values[17])
            if (borderWidth > 0f) setStroke(borderWidth.toInt().coerceAtLeast(1), borderColor)
            cornerRadii = floatArrayOf(
                resolveLength(values[33], values[34], node.geometry.width) * density,
                resolveLength(values[33], values[34], node.geometry.height) * density,
                resolveLength(values[35], values[36], node.geometry.width) * density,
                resolveLength(values[35], values[36], node.geometry.height) * density,
                resolveLength(values[37], values[38], node.geometry.width) * density,
                resolveLength(values[37], values[38], node.geometry.height) * density,
                resolveLength(values[39], values[40], node.geometry.width) * density,
                resolveLength(values[39], values[40], node.geometry.height) * density,
            )
        }
    }

    private fun resolveLength(length: Float, fraction: Float, axis: Float): Float = length + fraction * axis
    private fun rgba(red: Float, green: Float, blue: Float, alpha: Float): Int = Color.argb(
        (alpha * 255f).toInt().coerceIn(0, 255), red.toInt().coerceIn(0, 255),
        green.toInt().coerceIn(0, 255), blue.toInt().coerceIn(0, 255),
    )
    private fun parseNamedColor(name: String): Int = runCatching { Color.parseColor(name) }.getOrDefault(Color.TRANSPARENT)

    @Suppress("LongParameterList")
    fun measureFromNative(
        elementType: Int, kind: Int,
        knownWidth: Float, knownHeight: Float, knownMask: Int,
        availableWidth: Float, availableHeight: Float, availableWidthKind: Int, availableHeightKind: Int,
        text: String, fontFamily: String, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, letterSpacing: Float,
        lineHeight: Float, maxLines: Int, payloadVersion: Int, payload: ByteArray,
        intrinsicWidth: Float, intrinsicHeight: Float, intrinsicMask: Int,
    ): FloatArray {
        if (kind == 1) {
            val density = resources.displayMetrics.density
            val paint = TextPaint().apply {
                textSize = fontSize * density
                val typefaceStyle = (if (fontWeight >= 600) android.graphics.Typeface.BOLD else 0) or
                    (if (fontStyle != 0) android.graphics.Typeface.ITALIC else 0)
                val baseTypeface = if (fontFamily.isEmpty()) android.graphics.Typeface.DEFAULT else
                    android.graphics.Typeface.create(fontFamily, android.graphics.Typeface.NORMAL)
                typeface = android.graphics.Typeface.create(baseTypeface, typefaceStyle)
                this.letterSpacing = if (fontSize > 0f) letterSpacing / fontSize else 0f
            }
            val maxWidthPx = if (availableWidthKind == 0 && wrap != 0) {
                (availableWidth * density).toInt().coerceAtLeast(1)
            } else {
                paint.measureText(text).toInt().coerceAtLeast(1)
            }
            val builder = StaticLayout.Builder.obtain(text, 0, text.length, paint, maxWidthPx)
                .setAlignment(Layout.Alignment.ALIGN_NORMAL)
                .setIncludePad(false)
                .setMaxLines(if (maxLines == 0) Int.MAX_VALUE else maxLines)
            if (lineHeight > 0f) {
                val fontHeight = paint.fontMetrics.run { descent - ascent }
                builder.setLineSpacing((lineHeight * density - fontHeight).coerceAtLeast(0f), 1f)
            }
            val layout = builder.build()
            val width = if (knownMask and 1 != 0) knownWidth else layout.width / density
            val height = if (knownMask and 2 != 0) knownHeight else layout.height / density
            val first = if (layout.lineCount > 0) layout.getLineBaseline(0) / density else 0f
            val last = if (layout.lineCount > 0) layout.getLineBaseline(layout.lineCount - 1) / density else first
            return floatArrayOf(1f, 0f, width, height, first, last, 3f)
        }
        if ((kind == 2 || kind == 4) && intrinsicMask == 3) {
            return floatArrayOf(
                1f, 0f,
                if (knownMask and 1 != 0) knownWidth else intrinsicWidth,
                if (knownMask and 2 != 0) knownHeight else intrinsicHeight,
                0f, 0f, 0f,
            )
        }
        val custom = WhiskerElementRegistry.measure(elementType, WhiskerMeasureRequest(
            if (availableWidthKind == 0) availableWidth else null,
            if (availableHeightKind == 0) availableHeight else null,
            if (knownMask and 1 != 0) knownWidth else null,
            if (knownMask and 2 != 0) knownHeight else null,
            payloadVersion, payload,
        )) ?: return floatArrayOf(3f, 1f, 0f, 0f, 0f, 0f, 0f)
        return floatArrayOf(
            1f, 0f,
            if (knownMask and 1 != 0) knownWidth else custom.width,
            if (knownMask and 2 != 0) knownHeight else custom.height,
            0f, 0f, 0f,
        )
    }

    private external fun nativeCreate(width: Float, height: Float, scale: Float): Long
    private external fun nativeTick(
        handle: Long,
        timestampMs: Double,
        width: Float,
        height: Float,
        scale: Float,
    ): Boolean
    private external fun nativeDestroy(handle: Long)
    private external fun nativeDispatchEvent(
        handle: Long,
        node: Long,
        name: String,
        detail: WhiskerValue,
        timestampMs: Double,
    ): Boolean
    private external fun nativeResolveModule(
        callbackPtr: Long,
        userDataPtr: Long,
        payload: WhiskerValue,
    )
    private external fun nativeDispatchModuleEvent(
        handle: Long,
        module: String,
        event: String,
        payload: WhiskerValue,
    ): Boolean
}
