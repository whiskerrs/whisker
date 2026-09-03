package rs.whisker.runtime

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Choreographer
import android.view.MotionEvent
import android.view.View
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.findViewTreeLifecycleOwner
import rs.whisker.runtime.WhiskerValue
import rs.whisker.runtime.bridge.AndroidFrameBatch
import rs.whisker.runtime.bridge.AndroidHostCapabilities
import rs.whisker.runtime.bridge.MobileAbi
import rs.whisker.runtime.input.normalizePointerInput
import rs.whisker.runtime.measure.HostMeasurementProvider
import rs.whisker.runtime.measure.HostMeasureBatchAbi
import rs.whisker.runtime.measure.HostMeasureBatchResponse
import rs.whisker.runtime.module.HostModuleDispatcher
import rs.whisker.runtime.module.PendingModuleEvent
import rs.whisker.runtime.module.PendingModuleEvents
import rs.whisker.runtime.paint.HostRasterResourceStore
import rs.whisker.runtime.resource.HostResourceAbiEvent
import rs.whisker.runtime.resource.HostResourceChannel
import rs.whisker.runtime.resource.HostRasterSource
import rs.whisker.runtime.resource.HostResourceService
import rs.whisker.runtime.resource.HostResourceSnapshot
import rs.whisker.runtime.scene.HostElementBootstrap
import rs.whisker.runtime.scene.HostNode
import rs.whisker.runtime.scene.HostScene
import rs.whisker.runtime.scene.HostSceneOperation

private const val HOST_APPLY_ACCEPTED = MobileAbi.APPLY_ACCEPTED
private const val HOST_APPLY_REJECTED = MobileAbi.APPLY_REJECTED

private class ScrollOffsetBatch(val nodes: LongArray, val offsets: FloatArray)

/** The single Android View that owns a Whisker runtime and its native scene. */
class WhiskerView(context: Context) :
    WhiskerContainerView(context),
    Choreographer.FrameCallback,
    WhiskerRuntimeOwner {
    override val runtimeContext: Context
        get() = context

    private val choreographer = Choreographer.getInstance()
    private var nativeHandle = 0L
    private var runtimePaused = false
    private var permanentlyDestroyed = false
    private var lifecycleOwner: LifecycleOwner? = null
    private val lifecycleObserver = LifecycleEventObserver { owner, event ->
        if (event == Lifecycle.Event.ON_DESTROY && owner === lifecycleOwner) {
            destroyRuntime(permanent = true)
        }
    }
    private var frameScheduled = false
    private var windowVisible = true
    private val mainHandler = Handler(Looper.getMainLooper())
    private val elements = WhiskerElementRegistry.newBindings()
    private val measurements = HostMeasurementProvider(context, elements)
    private val bootstrap = HostElementBootstrap(elements)
    private val rasterResources = HostRasterResourceStore()
    private val dirtyScrollOffsets = LinkedHashMap<Long, FloatArray>()
    private val emptyScrollNodes = LongArray(0)
    private val emptyScrollOffsets = FloatArray(0)
    private val scene = HostScene(
        this,
        context,
        ::dispatchElementEvent,
        ::recordScrollOffset,
        { node -> dirtyScrollOffsets.remove(node) },
        rasterResources,
        elements,
    )
    private val resourceService = HostResourceService(
        rasterResources,
        ::handleResourceEvent,
        { path -> context.assets.open(path) },
    )
    private val resourceChannel = HostResourceChannel(resourceService)
    private var resourceEventObserver: ((HostResourceSnapshot) -> Unit)? = null
    private val modules = HostModuleDispatcher(::nativeResolveModule)
    private var backdropCaptureTarget: HostNode? = null
    private var backdropCaptureReached = false
    private val pendingContinuousEvents = PendingContinuousEvents()
    private var continuousEventFlushPending = false
    private val pendingModuleEvents = PendingModuleEvents()
    private val moduleEventFlush = Runnable {
        val pending = pendingModuleEvents.drain(moduleEventSinkEpoch)
        if (pending.isEmpty()) return@Runnable
        scene.dispatchOrDefer {
            val handle = nativeHandle
            if (handle == 0L) return@dispatchOrDefer
            var dispatched = false
            pending.forEach { moduleEvent ->
                dispatched = nativeDispatchModuleEvent(
                    handle,
                    moduleEvent.module,
                    moduleEvent.event,
                    moduleEvent.payload,
                ) || dispatched
            }
            if (dispatched) requestFrameFromNative()
        }
    }
    private var moduleEventSinkEpoch = 0L
    private val continuousEventFlush = Runnable {
        val pending = pendingContinuousEvents.drain()
        if (nativeHandle == 0L || pending.isEmpty()) return@Runnable
        pending.forEach(::dispatchElementEventNow)
        requestFrameFromNative()
    }

    init {
        WhiskerApplication.initialize(context)
        setBackgroundColor(Color.TRANSPARENT)
        clipChildren = false
    }

    internal val isRecordingBackdrop: Boolean
        get() = backdropCaptureTarget != null

    /** Records only content painted before [target] in Host draw order. */
    internal fun recordBackdrop(canvas: Canvas, target: HostNode) {
        check(backdropCaptureTarget == null)
        backdropCaptureTarget = target
        backdropCaptureReached = false
        try {
            super.draw(canvas)
        } finally {
            backdropCaptureTarget = null
            backdropCaptureReached = false
        }
    }

    /** Returns true once capture reaches the target, excluding it and later siblings. */
    internal fun shouldSkipBackdropCapture(node: HostNode): Boolean {
        val target = backdropCaptureTarget ?: return false
        if (node === target) backdropCaptureReached = true
        return backdropCaptureReached
    }

    override fun dispatchDraw(canvas: Canvas) {
        super.dispatchDraw(canvas)
        if (backdropCaptureTarget == null && continuousEventFlushPending) {
            continuousEventFlushPending = false
            mainHandler.post(continuousEventFlush)
        }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        bindLifecycleOwner()
        WhiskerAppContext.pushRuntimeOwner(this)
        mountWhenSized()
        resumeRuntime()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        mountWhenSized()
        requestFrameFromNative()
    }

    private fun mountWhenSized() {
        if (
            !permanentlyDestroyed && nativeHandle == 0L &&
            isAttachedToWindow && width > 0 && height > 0
        ) {
            moduleEventSinkEpoch += 1
            val sinkEpoch = moduleEventSinkEpoch
            WhiskerModuleEventCenter.installEventSink(this) { module, event, payload ->
                val shouldPost = pendingModuleEvents.offer(
                    PendingModuleEvent(sinkEpoch, module, event, payload),
                )
                if (shouldPost) {
                    mainHandler.post(moduleEventFlush)
                }
            }
            val density = resources.displayMetrics.density
            nativeHandle = nativeCreate(
                width / density,
                height / density,
                density,
                AndroidHostCapabilities.current().wireValues(),
            )
            if (nativeHandle != 0L) {
                runtimePaused = false
                requestFrameFromNative()
            } else {
                moduleEventSinkEpoch += 1
                WhiskerModuleEventCenter.installEventSink(this, null)
                Log.e("WhiskerView", "Unable to create the Rust runtime; see bootstrap diagnostics above")
            }
        }
    }

    override fun onDetachedFromWindow() {
        pauseRuntime()
        frameScheduled = false
        continuousEventFlushPending = false
        choreographer.removeFrameCallback(this)
        mainHandler.removeCallbacks(continuousEventFlush)
        mainHandler.removeCallbacks(moduleEventFlush)
        pendingContinuousEvents.clear()
        WhiskerAppContext.popRuntimeOwner(this)
        super.onDetachedFromWindow()
    }

    /**
     * Permanently releases this View's Rust runtime and retained Host scene.
     *
     * A View without a ViewTreeLifecycleOwner is only paused when detached,
     * because Android does not distinguish temporary reparenting from final
     * removal at that callback. Its owner must call this method when the View
     * will not be attached again.
     */
    fun destroy() {
        check(Looper.myLooper() == Looper.getMainLooper()) {
            "WhiskerView.destroy() must run on the main thread"
        }
        destroyRuntime(permanent = true)
    }

    private fun bindLifecycleOwner() {
        val next = findViewTreeLifecycleOwner()
        if (next === lifecycleOwner) return
        lifecycleOwner?.lifecycle?.removeObserver(lifecycleObserver)
        lifecycleOwner = next
        next?.lifecycle?.addObserver(lifecycleObserver)
    }

    private fun pauseRuntime() {
        val handle = nativeHandle
        if (handle != 0L && !runtimePaused && nativePause(handle)) {
            runtimePaused = true
        }
    }

    private fun resumeRuntime() {
        val handle = nativeHandle
        if (handle != 0L && runtimePaused && nativeResume(handle)) {
            runtimePaused = false
            requestFrameFromNative()
        }
    }

    private fun destroyRuntime(permanent: Boolean = false) {
        permanentlyDestroyed = permanentlyDestroyed || permanent
        lifecycleOwner?.lifecycle?.removeObserver(lifecycleObserver)
        lifecycleOwner = null
        moduleEventSinkEpoch += 1
        WhiskerModuleEventCenter.installEventSink(this, null)
        val handle = nativeHandle
        nativeHandle = 0L
        runtimePaused = false
        if (handle != 0L) nativeDestroy(handle)
        frameScheduled = false
        continuousEventFlushPending = false
        choreographer.removeFrameCallback(this)
        mainHandler.removeCallbacks(continuousEventFlush)
        mainHandler.removeCallbacks(moduleEventFlush)
        scene.clear()
        pendingContinuousEvents.clear()
        pendingModuleEvents.clear()
        WhiskerAppContext.popRuntimeOwner(this)
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        windowVisible = visibility == View.VISIBLE
        if (windowVisible) {
            mountWhenSized()
            requestFrameFromNative()
        } else {
            frameScheduled = false
            continuousEventFlushPending = false
            choreographer.removeFrameCallback(this)
            mainHandler.removeCallbacks(continuousEventFlush)
            pendingContinuousEvents.clear()
        }
    }

    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        val childConsumed = super.dispatchTouchEvent(event)
        val runtimeReceived = dispatchPointerInput(event)
        return childConsumed || runtimeReceived
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        val childConsumed = super.dispatchGenericMotionEvent(event)
        val runtimeConsumed = dispatchPointerInput(event)
        return childConsumed || runtimeConsumed
    }

    private fun dispatchPointerInput(event: MotionEvent): Boolean {
        val density = resources.displayMetrics.density
        val pointers = normalizePointerInput(event, density)
        pointers.forEach { pointer ->
            val handle = nativeHandle
            if (handle != 0L) {
                val scrollBatch = takeScrollOffsets()
                nativeDispatchPointer(
                    handle,
                    pointer.timestampMs,
                    pointer.event,
                    pointer.pointerId,
                    pointer.kind,
                    pointer.x,
                    pointer.y,
                    pointer.buttons,
                    pointer.changedButton,
                    scrollBatch?.nodes ?: emptyScrollNodes,
                    scrollBatch?.offsets ?: emptyScrollOffsets,
                )
            }
        }
        // Android requires the View that accepted ACTION_DOWN to retain the
        // complete stream. Listener consumption is a Rust routing result, not
        // an Android ownership decision, so receiving a normalized sample is
        // enough to claim it while the runtime is mounted.
        return nativeHandle != 0L && pointers.isNotEmpty()
    }

    private fun recordScrollOffset(node: Long, x: Float, y: Float) {
        val offset = dirtyScrollOffsets[node]
        if (offset == null) dirtyScrollOffsets[node] = floatArrayOf(x, y)
        else {
            offset[0] = x
            offset[1] = y
        }
    }

    private fun takeScrollOffsets(): ScrollOffsetBatch? {
        if (dirtyScrollOffsets.isEmpty()) return null
        val nodes = LongArray(dirtyScrollOffsets.size)
        val offsets = FloatArray(dirtyScrollOffsets.size * 2)
        dirtyScrollOffsets.entries.forEachIndexed { index, entry ->
            nodes[index] = entry.key
            offsets[index * 2] = entry.value[0]
            offsets[index * 2 + 1] = entry.value[1]
        }
        dirtyScrollOffsets.clear()
        return ScrollOffsetBatch(nodes, offsets)
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

    fun beginBootstrapFromNative() = bootstrap.begin()

    @Suppress("LongParameterList")
    fun registerElementFromNative(
        elementType: Int,
        name: String,
        childPolicy: Int,
        measurement: Int,
        textStyle: Int,
        propertyIds: IntArray,
        propertyKinds: IntArray,
        propertyNames: Array<String>,
        eventIds: IntArray,
        eventKinds: IntArray,
        eventNames: Array<String>,
        commandIds: IntArray,
        commandKinds: IntArray,
        commandNames: Array<String>,
    ) = bootstrap.register(
        elementType,
        name,
        childPolicy,
        measurement,
        textStyle,
        propertyIds,
        propertyKinds,
        propertyNames,
        eventIds,
        eventKinds,
        eventNames,
        commandIds,
        commandKinds,
        commandNames,
    )

    fun finishBootstrapFromNative(): Boolean = bootstrap.finish()

    internal fun beginFrameForTesting(
        mode: Int,
        epoch: Int,
        baseRevision: Long,
        targetRevision: Long,
    ): Int = scene.beginFrame(mode, epoch, baseRevision, targetRevision)

    fun currentRevisionFromNative(): Long = scene.currentRevision()

    /**
     * Accepts one complete scene transaction through a single JNI method invocation.
     *
     * Each operation occupies ten longs in [metadata]: tag, flags, node, parent, child,
     * index, member, integer, scalar bits, and wide. Variable-sized typed payloads remain
     * in parallel arrays so JNI owns them for the duration of this call.
     */
    @Suppress("LongParameterList")
    fun presentFrameFromNative(
        mode: Int,
        epoch: Int,
        baseRevision: Long,
        targetRevision: Long,
        metadata: LongArray,
        numbers: Array<FloatArray?>,
        texts: Array<String?>,
        names: Array<Array<String>?>,
        values: Array<WhiskerValue?>,
        response: LongArray,
    ): Boolean {
        if (response.size < 2) return false
        val operationCount = metadata.size / AndroidFrameBatch.STRIDE
        if (
            metadata.size % AndroidFrameBatch.STRIDE != 0 ||
            numbers.size != operationCount ||
            texts.size != operationCount ||
            names.size != operationCount ||
            values.size != operationCount
        ) {
            response[0] = HOST_APPLY_REJECTED.toLong()
            response[1] = scene.currentRevision()
            return true
        }

        val beginStatus = scene.beginFrame(mode, epoch, baseRevision, targetRevision)
        if (beginStatus != HOST_APPLY_ACCEPTED) {
            response[0] = beginStatus.toLong()
            response[1] = scene.currentRevision()
            return true
        }

        repeat(operationCount) { index ->
            val offset = index * AndroidFrameBatch.STRIDE
            scene.stage(
                HostSceneOperation(
                    tag = metadata[offset + AndroidFrameBatch.TAG].toInt(),
                    flags = metadata[offset + AndroidFrameBatch.FLAGS].toInt(),
                    node = metadata[offset + AndroidFrameBatch.NODE],
                    parent = metadata[offset + AndroidFrameBatch.PARENT],
                    child = metadata[offset + AndroidFrameBatch.CHILD],
                    index = metadata[offset + AndroidFrameBatch.INDEX].toInt(),
                    member = metadata[offset + AndroidFrameBatch.MEMBER].toInt(),
                    integer = metadata[offset + AndroidFrameBatch.INTEGER].toInt(),
                    scalar = Float.fromBits(metadata[offset + AndroidFrameBatch.SCALAR].toInt()),
                    wide = metadata[offset + AndroidFrameBatch.WIDE],
                    numbers = numbers[index],
                    text = texts[index],
                    names = names[index],
                    value = values[index],
                ),
            )
        }

        val accepted = scene.commit()
        response[0] = (if (accepted) HOST_APPLY_ACCEPTED else HOST_APPLY_REJECTED).toLong()
        response[1] = if (accepted) targetRevision else scene.currentRevision()
        return true
    }

    /** Registers an already decoded raster. Acquisition and eviction are separate Host concerns. */
    internal fun registerRasterResourceForTesting(resourceId: Long, bitmap: Bitmap): Boolean =
        rasterResources.register(resourceId, bitmap)

    internal fun loadRasterResourceBytesForTesting(
        resourceId: Long,
        generation: Long,
        mediaType: String,
        data: ByteArray,
    ): Boolean = resourceService.load(
        resourceId,
        generation,
        HostRasterSource.Bytes(mediaType, data.copyOf()),
    )

    internal fun loadRasterResourceUrlForTesting(
        resourceId: Long,
        generation: Long,
        url: String,
    ): Boolean = resourceService.load(resourceId, generation, HostRasterSource.Url(url))

    internal fun releaseRasterResourceForTesting(resourceId: Long, generation: Long): Boolean =
        resourceService.release(resourceId, generation)

    /** Receives one typed command whose JNI-owned arguments outlive the C callback. */
    @Suppress("LongParameterList")
    fun resourceCommandFromNative(
        command: Int,
        kind: Int,
        source: Int,
        resourceId: Long,
        generation: Long,
        identifier: String,
        data: ByteArray,
    ): Boolean = resourceChannel.accept(
        command,
        kind,
        source,
        resourceId,
        generation,
        identifier,
        data,
    )

    internal fun awaitRasterResourceForTesting(
        resourceId: Long,
        generation: Long,
        timeoutMillis: Long,
    ): HostResourceSnapshot? =
        resourceService.awaitTerminal(resourceId, generation, timeoutMillis)

    /** Observes owned Android lifecycle messages after asynchronous completion. */
    internal fun observeRasterResourceEventsForTesting(observer: ((HostResourceSnapshot) -> Unit)?) {
        resourceEventObserver = observer
    }

    @Suppress("LongParameterList")
    internal fun stageOperationForTesting(
        tag: Int,
        flags: Int,
        node: Long,
        parent: Long,
        child: Long,
        index: Int,
        member: Int,
        integer: Int,
        scalar: Float,
        wide: Long,
        numbers: FloatArray?,
        text: String?,
        names: Array<String>?,
        value: WhiskerValue?,
    ): Boolean = scene.stage(
        HostSceneOperation(
            tag,
            flags,
            node,
            parent,
            child,
            index,
            member,
            integer,
            scalar,
            wide,
            numbers,
            text,
            names,
            value,
        ),
    )

    internal fun commitFrameForTesting(): Boolean = scene.commit()

    private fun handleResourceEvent(event: HostResourceSnapshot) {
        val abiEvent = HostResourceChannel.encodeEvent(event)
        mainHandler.post {
            resourceEventObserver?.invoke(event)
            val handle = nativeHandle
            if (handle != 0L && abiEvent != null) {
                dispatchResourceEvent(handle, abiEvent)
            }
            invalidate()
            requestFrameFromNative()
        }
    }

    private fun dispatchResourceEvent(handle: Long, event: HostResourceAbiEvent) {
        nativeDispatchResourceEvent(
            handle,
            event.status,
            event.failureCode,
            event.resourceId,
            event.generation,
            event.width,
            event.height,
            event.scale,
            event.dimensionsMask,
            event.diagnostic,
        )
    }

    private fun dispatchElementEvent(node: Long, name: String, detail: WhiskerValue) {
        scene.dispatchOrDefer {
            val event = PendingElementEvent(
                node,
                name,
                detail,
                android.os.SystemClock.uptimeMillis().toDouble(),
            )
            if (name == "scroll") {
                pendingContinuousEvents.offer(event)
                scheduleContinuousEventFlush()
                return@dispatchOrDefer
            }
            dispatchElementEventNow(event)
        }
    }

    private fun scheduleContinuousEventFlush() {
        if (nativeHandle == 0L) return
        continuousEventFlushPending = true
        postInvalidateOnAnimation()
    }

    private fun dispatchElementEventNow(event: PendingElementEvent) {
        val handle = nativeHandle
        if (handle != 0L) {
            nativeDispatchEvent(
                handle,
                event.node,
                event.name,
                event.detail,
                event.timestampMs,
            )
        }
    }

    /** Called by the retained Rust runtime through its small JNI callback. */
    fun invokeModuleFromNative(
        module: String,
        method: String,
        args: Array<WhiskerValue>,
        isAsync: Boolean,
        callbackPtr: Long,
        userDataPtr: Long,
    ): Boolean = modules.invoke(module, method, args, isAsync, callbackPtr, userDataPtr)

    /** Called by Rust on the first and last listener transition. */
    fun observeModuleFromNative(module: String, event: String, observing: Boolean) {
        modules.observe(this, module, event, observing)
    }

    /** Processes one native intrinsic-measurement batch in a single Host call. */
    fun measureBatchFromNative(
        requestLongs: LongArray,
        requestInts: IntArray,
        requestFloats: FloatArray,
        requestStrings: Array<String>,
        fontFamilies: Array<Array<String>>,
        fontSettings: Array<Array<String>>,
        payloads: Array<ByteArray>,
    ): HostMeasureBatchResponse = HostMeasureBatchAbi.measure(
        measurements,
        requestLongs,
        requestInts,
        requestFloats,
        requestStrings,
        fontFamilies,
        fontSettings,
        payloads,
    )

    private external fun nativeCreate(
        width: Float,
        height: Float,
        scale: Float,
        capabilities: LongArray,
    ): Long
    private external fun nativeTick(
        handle: Long,
        timestampMs: Double,
        width: Float,
        height: Float,
        scale: Float,
    ): Boolean
    private external fun nativePause(handle: Long): Boolean
    private external fun nativeResume(handle: Long): Boolean
    private external fun nativeDestroy(handle: Long)
    private external fun nativeDispatchEvent(
        handle: Long,
        node: Long,
        name: String,
        detail: WhiskerValue,
        timestampMs: Double,
    ): Boolean
    private external fun nativeDispatchPointer(
        handle: Long,
        timestampMs: Double,
        event: Int,
        pointerId: Long,
        pointerKind: Int,
        x: Float,
        y: Float,
        buttons: Int,
        changedButton: Int,
        scrollNodes: LongArray,
        scrollOffsets: FloatArray,
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
    private external fun nativeDispatchResourceEvent(
        handle: Long,
        status: Int,
        failureCode: Int,
        resourceId: Long,
        generation: Long,
        width: Float,
        height: Float,
        scale: Float,
        dimensionsMask: Int,
        diagnostic: String,
    ): Boolean
}
