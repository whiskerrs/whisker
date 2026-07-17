package rs.whisker.runtime

import android.content.Context
import android.os.Looper
import android.util.AttributeSet
import android.util.Log
import android.view.Choreographer
import androidx.annotation.Keep
import com.lynx.tasm.EventEmitter
import com.lynx.tasm.LynxView
import com.lynx.tasm.LynxViewBuilder
import com.lynx.tasm.behavior.TouchEventDispatcher
import com.lynx.tasm.event.LynxCustomEvent
import com.lynx.tasm.event.LynxEvent
import com.lynx.tasm.event.LynxInternalEvent
import com.lynx.tasm.event.LynxTouchEvent
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Hosts the Lynx engine and bridges it to the Rust runtime.
 *
 * Inherits from [LynxView] to reuse Lynx's Android SDK polish (Surface
 * management, vsync, lifecycle, touch dispatch, accessibility, IME).
 *
 * Instead of using LynxView's template-loading path, we obtain the engine
 * shell pointer and hand it to the Rust runtime via JNI. The Rust side
 * then drives the element tree directly via the bridge (Element PAPI).
 *
 * Render loop:
 *   - A [Choreographer.FrameCallback] is the heartbeat; we only schedule
 *     a frame when the Rust runtime asks for one (`requestFrameFromNative`,
 *     called from JNI on every signal update).
 *   - Each fire calls `nativeTick`; if the runtime reports it's still
 *     not idle we schedule another frame.
 */
class WhiskerView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : LynxView(
        context,
        // Lynx's own tap-cancel threshold (`TouchEventDispatcher.mTapSlop`)
        // defaults to 50px (~50dip-equivalent), far more generous than the
        // ~8dp `ViewConfiguration.getScaledTouchSlop()` Android's own
        // scroll containers (and Lynx's `NestedScrollContainerView`) use to
        // start scrolling. That gap let a finger drift far enough to
        // visibly start a scroll while still firing a `tap` on release.
        // 18dp (not the scroll threshold's own 8dp) matches Flutter's
        // `kTouchSlop` — Flutter shipped 8dp first and raised it to 18dp
        // after complaints that deliberate taps were too easily cancelled
        // by ordinary hand tremor; still far below the original 50dp gap.
        LynxViewBuilder().setTapSlop("18px"),
    ),
    WhiskerModuleHost {

    private var engine: Long = 0
    private val scheduled = AtomicBoolean(false)
    private val frameCallback = Choreographer.FrameCallback { onFrame() }

    // LynxEventEmitter stores the reporter as a WeakReference (see
    // `LynxEventEmitter.registerEventReporter` upstream), so if we only
    // hand it an anonymous object the GC will reclaim it before any tap
    // ever fires. Keep a strong reference here.
    private val eventReporter = object : EventEmitter.LynxEventReporter {
        override fun onLynxEvent(event: LynxEvent): Boolean {
            val name = event.name ?: return false
            // Normalize the body to the same shape iOS's
            // `LynxEvent.generateEventBody` produces —
            // `{type, target, currentTarget, detail}` — so the typed
            // event structs in `whisker_runtime::event` deserialize
            // identically on both platforms. Android's reporter
            // otherwise hands us only the raw params dict
            // (`LynxCustomEvent.eventParams()`, where component events
            // like `scroll` put their fields directly via `addDetail`),
            // which has no `detail` wrapper or target keys — leaving the
            // typed `detail` (and `target`) blank. `target`/`currentTarget`
            // are the integer sign (the Rust `Target` deserializes an
            // int → `uid`). `detail` is the params dict for
            // `LynxCustomEvent` (scroll / layout / …). The marshaller
            // turns this Java map into a `WhiskerValueRaw` tree (no JSON).
            val body: MutableMap<String, Any?> =
                mutableMapOf(
                    "type" to name,
                    "target" to event.tag,
                    "currentTarget" to event.tag,
                    // Overwritten below for custom/touch events; stays
                    // explicitly null (not just absent) for anything
                    // else, matching this reporter's previous behavior.
                    "detail" to null,
                )
            if (event is LynxCustomEvent) {
                body["detail"] = event.eventParams()
            } else if (event is LynxTouchEvent) {
                // `LynxTouchEvent` doesn't carry coordinates through
                // the generic params path above — only
                // `getClientPoint`/`getPagePoint`/`getTouchMap` do.
                // Splice touches/changedTouches/detail on here,
                // mirroring the shape `whisker_bridge_ios.mm`'s
                // reporter block builds from the same Lynx class on
                // iOS, so `whisker_runtime::event::TouchEvent`
                // deserializes identically on both platforms instead
                // of every field silently defaulting to zero here.
                if (!event.getIsMultiTouch()) {
                    val page = event.getPagePoint()
                    val client = event.getClientPoint()
                    if (page != null && client != null) {
                        val x = pxToDip(page.x)
                        val y = pxToDip(page.y)
                        val clientX = pxToDip(client.x)
                        val clientY = pxToDip(client.y)
                        val touch =
                            mapOf(
                                "identifier" to 0,
                                "x" to x,
                                "y" to y,
                                "pageX" to x,
                                "pageY" to y,
                                "clientX" to clientX,
                                "clientY" to clientY,
                            )
                        body["touches"] = listOf(touch)
                        body["changedTouches"] = listOf(touch)
                        body["detail"] = mapOf("x" to x, "y" to y)
                    }
                } else {
                    val touchMap = event.getTouchMap()
                    if (touchMap != null) {
                        val touches =
                            touchMap.entries.map { (identifier, point) ->
                                val x = pxToDip(point.x)
                                val y = pxToDip(point.y)
                                mapOf(
                                    "identifier" to identifier,
                                    "x" to x,
                                    "y" to y,
                                    "pageX" to x,
                                    "pageY" to y,
                                    "clientX" to x,
                                    "clientY" to y,
                                )
                            }
                        body["touches"] = touches
                        body["changedTouches"] = touches
                        touches.firstOrNull()?.let { first ->
                            body["detail"] = mapOf("x" to first["pageX"], "y" to first["pageY"])
                        }
                    }
                }
            }
            return nativeOnLynxEvent(engine, event.tag, name, body)
        }
        override fun onInternalEvent(event: LynxInternalEvent) {}
    }

    // `LynxTouchEvent.getPagePoint()`/`getClientPoint()`/`getTouchMap()`
    // hand back raw `MotionEvent` coordinates (device px, no density
    // conversion) — confirmed against the Lynx fork's own
    // `TouchEventDispatcher.dispatchEvent`. Every other geometry value
    // reaching Rust (`boundingClientRect()`, layout `left`/`top`/…) is
    // in dip, via `LynxBaseUI.boundingClientRectInner`'s explicit
    // `/ density`. Forwarding touch points unconverted made drag
    // gestures (e.g. the reader's progress-bar seek, which divides a
    // touch delta by a dip-based measured width) scale with the
    // device's density instead of the physical drag distance — up to
    // ~3x too sensitive on a high-density phone.
    private fun pxToDip(px: Float): Float = px / resources.displayMetrics.density

    init {
        engine = nativeEngineAttach(this)
        if (engine != 0L) {
            nativeBindWhiskerView(engine)
            installEventReporter()
            nativeAppMain(engine)
        }
        // Temporary diagnostic — reports what tapSlop value actually
        // ends up armed on `TouchEventDispatcher` (the value the
        // engine really compares touch drift against), vs. what we
        // asked for via the `LynxViewBuilder` constructor above.
        // `TouchEventDispatcher.onPageConfigDecoded`'s own tapSlop
        // resolution runs once the page/template config decodes,
        // which can happen after this constructor returns, so read
        // it on a short delay rather than inline here. Remove once
        // the tapSlop regression is root-caused. Filter with
        // `adb logcat -s WhiskerTapSlop`.
        postDelayed({ logTapSlopDiagnostic() }, 1000)
    }

    private fun logTapSlopDiagnostic() {
        try {
            val ctx = lynxContext
            val builderValue = ctx?.tapSlop
            val dispatcher = ctx?.touchEventDispatcher
            val field = TouchEventDispatcher::class.java.getDeclaredField("mTapSlop")
            field.isAccessible = true
            val liveValue = dispatcher?.let { field.get(it) }
            Log.d(
                "WhiskerTapSlop",
                "LynxContext.tapSlop=$builderValue " +
                    "TouchEventDispatcher.mTapSlop(px)=$liveValue " +
                    "density=${resources.displayMetrics.density}",
            )
        } catch (e: Exception) {
            Log.e("WhiskerTapSlop", "diagnostic failed", e)
        }
    }

    /**
     * Route every LynxEvent the engine fires through the bridge so Rust
     * `on_tap:` (and friends) declared on Fiber elements get called.
     *
     * Mirrors the iOS path that installs an `eventReporterBlock` on
     * `LynxEventEmitter`. The reporter is a single Java object that
     * forwards `(tag, name)` into JNI; the bridge does the registry
     * lookup against the native callbacks Rust registered.
     */
    private fun installEventReporter() {
        lynxContext?.eventEmitter?.registerEventReporter(eventReporter)
    }

    override fun destroy() {
        if (engine != 0L) {
            Choreographer.getInstance().removeFrameCallback(frameCallback)
            nativeEngineRelease(engine)
            engine = 0L
        }
        // Defensive — onDetachedFromWindow normally runs first, but
        // some host flows call destroy() directly. Either path leaves
        // the host stack clean.
        WhiskerAppContext.popHost(this)
        super.destroy()
    }

    // ---- WhiskerModuleHost wiring -----------------------------------------
    //
    // Publish this view as the current `appContext.currentActivity`
    // candidate while it's attached to a window. Modules use the
    // resolved Activity to register window-scoped callbacks
    // (`OnBackInvokedCallback`, sensor listeners, ...). The Activity
    // accessor unwraps the view's `context` ContextWrapper chain at
    // call time, so a config-change rotation transparently picks up
    // the new Activity once the new view attaches.

    /**
     * Implements [WhiskerModuleHost.hostContext] by forwarding to
     * the view's own [getContext]. Named `hostContext` (not
     * `context`) so it doesn't collide with `View.getContext()` on
     * the JVM `getContext()` signature.
     */
    override val hostContext: Context get() = getContext()

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        WhiskerAppContext.pushHost(this)
    }

    override fun onDetachedFromWindow() {
        WhiskerAppContext.popHost(this)
        super.onDetachedFromWindow()
    }

    /** Called from native (any thread) when a signal update marks the
     *  tree dirty and the render loop needs to run. */
    @Keep
    fun requestFrameFromNative() {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            scheduleFrame()
        } else {
            post { scheduleFrame() }
        }
    }

    private fun scheduleFrame() {
        if (engine == 0L) return
        if (scheduled.compareAndSet(false, true)) {
            Choreographer.getInstance().postFrameCallback(frameCallback)
        }
    }

    private fun onFrame() {
        scheduled.set(false)
        if (engine == 0L) return
        val idle = nativeTick(engine)
        if (!idle) scheduleFrame()
    }

    // `nativeEngineAttach(this)` reads our LynxView superclass's
    // private `mLynxTemplateRender.mNativePtr` to get a `LynxShell*`,
    // wraps it in a `WhiskerEngine`, and returns the engine handle.
    private external fun nativeEngineAttach(view: LynxView): Long
    private external fun nativeBindWhiskerView(engine: Long)
    private external fun nativeAppMain(engine: Long)
    private external fun nativeTick(engine: Long): Boolean
    private external fun nativeEngineRelease(engine: Long)
    private external fun nativeOnLynxEvent(
        engine: Long,
        tag: Int,
        name: String,
        params: Any?,
    ): Boolean
}
