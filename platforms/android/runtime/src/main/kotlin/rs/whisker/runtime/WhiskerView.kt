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
import rs.whisker.runtime.WhiskerValue
import rs.whisker.runtime.input.HostPointerInput
import rs.whisker.runtime.input.normalizePointerInput
import rs.whisker.runtime.measure.HostMeasurementProvider
import rs.whisker.runtime.module.HostModuleDispatcher
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

private const val HOST_SCENE_OPERATION_STRIDE = 10
private const val HOST_APPLY_ACCEPTED = 0
private const val HOST_APPLY_REJECTED = 2

/** The single Android View that owns a Whisker runtime and its native scene. */
class WhiskerView(context: Context) :
    WhiskerContainerView(context),
    Choreographer.FrameCallback,
    WhiskerRuntimeOwner {
    override val runtimeContext: Context
        get() = context

    private val choreographer = Choreographer.getInstance()
    private var nativeHandle = 0L
    private var frameScheduled = false
    private var windowVisible = true
    private val mainHandler = Handler(Looper.getMainLooper())
    private val measurements = HostMeasurementProvider(context)
    private val bootstrap = HostElementBootstrap()
    private val rasterResources = HostRasterResourceStore()
    private val scene = HostScene(this, context, ::dispatchElementEvent, rasterResources)
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
    private var pointerInputObserver: ((HostPointerInput) -> Unit)? = null

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
            if (nativeHandle != 0L) {
                requestFrameFromNative()
            } else {
                Log.e("WhiskerView", "Unable to create the Rust runtime; see bootstrap diagnostics above")
            }
        }
    }

    override fun onDetachedFromWindow() {
        if (nativeHandle != 0L) nativeDestroy(nativeHandle)
        nativeHandle = 0L
        frameScheduled = false
        choreographer.removeFrameCallback(this)
        WhiskerModuleEventCenter.installEventSink(null)
        scene.clear()
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
            pointerInputObserver?.invoke(pointer)
            val handle = nativeHandle
            if (handle != 0L) {
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
                )
            }
        }
        // Android requires the View that accepted ACTION_DOWN to retain the
        // complete stream. Listener consumption is a Rust routing result, not
        // an Android ownership decision, so receiving a normalized sample is
        // enough to claim it while the runtime is mounted.
        return nativeHandle != 0L && pointers.isNotEmpty()
    }

    /** Test-only observer at the production MotionEvent-to-runtime dispatch seam. */
    fun observePointerInputForTesting(observer: ((LongArray, DoubleArray) -> Unit)?) {
        pointerInputObserver = observer?.let { callback ->
            { pointer ->
                callback(
                    longArrayOf(
                        pointer.event.toLong(),
                        pointer.pointerId,
                        pointer.kind.toLong(),
                        pointer.buttons.toLong(),
                        pointer.changedButton.toLong(),
                    ),
                    doubleArrayOf(
                        pointer.timestampMs,
                        pointer.x.toDouble(),
                        pointer.y.toDouble(),
                    ),
                )
            }
        }
    }

    /** Test-only observer at the production text shaping configuration seam. */
    fun observeTextMeasurementForTesting(observer: ((IntArray, FloatArray) -> Unit)?) {
        measurements.textInspectionObserver = observer
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

    fun beginFrameFromNative(
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
        val operationCount = metadata.size / HOST_SCENE_OPERATION_STRIDE
        if (
            metadata.size % HOST_SCENE_OPERATION_STRIDE != 0 ||
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
            val offset = index * HOST_SCENE_OPERATION_STRIDE
            scene.stage(
                HostSceneOperation(
                    tag = metadata[offset].toInt(),
                    flags = metadata[offset + 1].toInt(),
                    node = metadata[offset + 2],
                    parent = metadata[offset + 3],
                    child = metadata[offset + 4],
                    index = metadata[offset + 5].toInt(),
                    member = metadata[offset + 6].toInt(),
                    integer = metadata[offset + 7].toInt(),
                    scalar = Float.fromBits(metadata[offset + 8].toInt()),
                    wide = metadata[offset + 9],
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
    fun registerRasterResourceFromNative(resourceId: Long, bitmap: Bitmap): Boolean =
        rasterResources.register(resourceId, bitmap)

    fun loadRasterResourceBytesFromNative(
        resourceId: Long,
        generation: Long,
        mediaType: String,
        data: ByteArray,
    ): Boolean = resourceService.load(
        resourceId,
        generation,
        HostRasterSource.Bytes(mediaType, data.copyOf()),
    )

    fun loadRasterResourceUrlFromNative(
        resourceId: Long,
        generation: Long,
        url: String,
    ): Boolean = resourceService.load(resourceId, generation, HostRasterSource.Url(url))

    fun releaseRasterResourceFromNative(resourceId: Long, generation: Long): Boolean =
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

    fun awaitRasterResourceFromNative(
        resourceId: Long,
        generation: Long,
        timeoutMillis: Long,
    ): HostResourceSnapshot? =
        resourceService.awaitTerminal(resourceId, generation, timeoutMillis)

    /** Observes owned Android lifecycle messages after asynchronous completion. */
    fun observeRasterResourceEvents(observer: ((HostResourceSnapshot) -> Unit)?) {
        resourceEventObserver = observer
    }

    @Suppress("LongParameterList")
    fun stageOperationFromNative(
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

    fun commitFrameFromNative(): Boolean = scene.commit()

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
            val handle = nativeHandle
            if (handle != 0L) {
                nativeDispatchEvent(
                    handle,
                    node,
                    name,
                    detail,
                    android.os.SystemClock.uptimeMillis().toDouble(),
                )
            }
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
        modules.observe(module, event, observing)
    }

    private fun dispatchModuleEvent(module: String, event: String, payload: WhiskerValue) {
        scene.dispatchOrDefer {
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
            } else {
                nativeDispatchModuleEvent(handle, module, event, payload)
            }
        }
    }

    @Suppress("LongParameterList")
    fun measureFromNative(
        elementType: Int, kind: Int,
        knownWidth: Float, knownHeight: Float, knownMask: Int,
        availableWidth: Float, availableHeight: Float, availableWidthKind: Int, availableHeightKind: Int,
        text: String, fontFamilies: Array<String>, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, wordBreak: Int, overflow: Int, letterSpacing: Float,
        lineHeight: Float, indentLogicalPixels: Float, indentPercentage: Float,
        maxLines: Int, fontSettings: Array<String>, fontFeatureCount: Int,
        fontOpticalSizing: Int, payloadVersion: Int, payload: ByteArray,
        intrinsicWidth: Float, intrinsicHeight: Float, intrinsicMask: Int,
        direction: Int, alignment: Int,
    ): FloatArray = measurements.measure(
        elementType, kind,
        knownWidth, knownHeight, knownMask,
        availableWidth, availableHeight, availableWidthKind, availableHeightKind,
        text, fontFamilies, fontSize, fontWeight,
        fontStyle, wrap, wordBreak, overflow, letterSpacing,
        lineHeight, indentLogicalPixels, indentPercentage,
        maxLines, fontSettings, fontFeatureCount, fontOpticalSizing, payloadVersion, payload,
        intrinsicWidth, intrinsicHeight, intrinsicMask, direction, alignment,
    )

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
