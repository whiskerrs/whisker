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

/** See `WhiskerView.configureEventDispatcher`'s doc comment for why this can't just be `LynxViewBuilder`'s tapSlop. */
private const val TAP_SLOP_DP = 18f

/** `[identifier, clientX, clientY, pageX, pageY, x, y]` — the per-finger record `uiTouchMap` carries. */
private const val FINGER_FIELDS = 7

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
        //
        // Kept even though it's confirmed dead for this app (see
        // `configureEventDispatcher`'s doc comment below) — cheap, harmless, and
        // becomes real again if a future Lynx version or code path
        // starts honoring it.
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
            if (event is LynxTouchEvent && event.getIsMultiTouch()) {
                return dispatchMultiTouch(name, event)
            }
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
                // `getClientPoint`/`getPagePoint` do.
                // Splice touches/changedTouches/detail on here,
                // mirroring the shape `whisker_bridge_ios.mm`'s
                // reporter block builds from the same Lynx class on
                // iOS, so `whisker_runtime::event::TouchEvent`
                // deserializes identically on both platforms instead
                // of every field silently defaulting to zero here.
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
            }
            return nativeOnLynxEvent(engine, event.tag, name, body)
        }
        override fun onInternalEvent(event: LynxInternalEvent) {}
    }

    /**
     * Fingers currently on the screen, keyed by touch identifier, in
     * the order they landed. Lynx reports only the fingers that changed
     * in a given event, so `touches` — every finger currently down,
     * which is what the DOM contract and
     * `whisker_runtime::event::TouchEvent` both promise — has to be
     * accumulated here.
     */
    private val currentTouches = LinkedHashMap<Int, Map<String, Any?>>()

    /**
     * Deliver one multi-touch event.
     *
     * Turning multi-touch on (see `configureEventDispatcher`) moves
     * every `touchstart`/`touchmove`/`touchend`/`touchcancel` onto
     * `EventEmitter.sendMultiTouchEvent`, which reports the fingers in
     * `uiTouchMap` and leaves `event.tag` at -1 — no single element
     * owns a gesture that can span several. `tap` / `longpress` /
     * `click` are unaffected and still arrive with a real tag.
     *
     * `uiTouchMap` is `{ elementTag: [[identifier, clientX, clientY,
     * pageX, pageY, x, y], …] }` — the same shape
     * `TouchEventHandler::GetTouchEventParam` parses in the engine.
     * Coordinates are raw device px, like every other point Lynx hands
     * the platform (see `pxToDip`).
     *
     * Fans back out to one dispatch per element, because a native
     * callback is registered against exactly one tag: two fingers on
     * two elements is two events, each carrying its own fingers as
     * `changedTouches` and every finger down as `touches`.
     */
    private fun dispatchMultiTouch(name: String, event: LynxTouchEvent): Boolean {
        val uiTouchMap = event.getUITouchMap() ?: return false

        val changedByTag = LinkedHashMap<Int, MutableList<Map<String, Any?>>>()
        for ((tagKey, fingers) in uiTouchMap) {
            val tag = tagKey.toIntOrNull() ?: continue
            if (fingers !is List<*>) continue
            for (finger in fingers) {
                val values = finger as? List<*> ?: continue
                if (values.size < FINGER_FIELDS) continue
                val numbers = values.map { (it as? Number)?.toFloat() ?: return@map 0f }
                val identifier = numbers[0].toInt()
                val touch =
                    mapOf<String, Any?>(
                        "identifier" to identifier,
                        "clientX" to pxToDip(numbers[1]),
                        "clientY" to pxToDip(numbers[2]),
                        "pageX" to pxToDip(numbers[3]),
                        "pageY" to pxToDip(numbers[4]),
                        "x" to pxToDip(numbers[5]),
                        "y" to pxToDip(numbers[6]),
                    )
                changedByTag.getOrPut(tag) { mutableListOf() }.add(touch)
                when (name) {
                    LynxTouchEvent.EVENT_TOUCH_END -> currentTouches.remove(identifier)
                    LynxTouchEvent.EVENT_TOUCH_CANCEL -> currentTouches.clear()
                    else -> currentTouches[identifier] = touch
                }
            }
        }
        if (changedByTag.isEmpty()) return false

        val touches = currentTouches.values.toList()
        var handled = false
        for ((tag, changed) in changedByTag) {
            // The engine reports the primary finger's position as the
            // event's own `detail`; anything watching a single point
            // (a tap fraction, a drag delta) reads that and stays
            // oblivious to the extra fingers.
            val primary = changed.firstOrNull { it["identifier"] == 0 } ?: changed.first()
            val body: MutableMap<String, Any?> =
                mutableMapOf(
                    "type" to name,
                    "target" to tag,
                    "currentTarget" to tag,
                    "detail" to mapOf("x" to primary["pageX"], "y" to primary["pageY"]),
                    "touches" to touches,
                    "changedTouches" to changed,
                )
            handled = nativeOnLynxEvent(engine, tag, name, body) || handled
        }
        return handled
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
    }

    // `LynxViewBuilder().setTapSlop(...)` above is a no-op for this
    // app — confirmed on-device via reflection (`adb logcat -s
    // WhiskerTapSlop`: `TouchEventDispatcher.mTapSlop` stayed at
    // Lynx's built-in 50dip default no matter what the builder was
    // given). Root cause, traced in the Lynx fork source
    // (`LynxUIRenderer.java`): the builder's tapSlop string only ever
    // reaches the live `TouchEventDispatcher` via
    // `onPageConfigDecoded()` → `updateEventDispatcherConfig()`, both
    // driven by Lynx's own template-loading pipeline — the one this
    // class's own doc comment says whisker bypasses entirely (engine
    // handed to Rust via JNI instead). `onPageConfigDecoded` never
    // fires, `mIsUpdatedConfig` never flips true, so
    // `EnsureEventDispatcher()` creates the dispatcher (lazily, on the
    // first real touch — `LynxUIRenderer.onTouchEvent`/
    // `onInterceptTouchEvent`) but never calls
    // `updateEventDispatcherConfig()`, and the dispatcher is left on
    // its own constructor default.
    //
    // Since nothing ever calls `LynxContext.setTouchEventDispatcher`
    // either, `lynxContext.touchEventDispatcher` stays null forever
    // too — there's no public path to the live dispatcher at all.
    // Reflects through `LynxView.mLynxTemplateRender` (protected,
    // direct field access) → `LynxTemplateRender.mLynxUIRender`
    // (private, declared as the `ILynxUIRenderer` interface but always
    // a `LynxUIRenderer` at runtime) → `LynxUIRenderer.mEventDispatcher`
    // (private) to reach the actual instance and set its tapSlop
    // directly, bypassing the page-config pipeline this app never
    // drives. Tried on every touch (idempotent past the first success)
    // since the dispatcher doesn't exist until the first one.
    private var dispatcherConfigured = false

    override fun dispatchTouchEvent(ev: android.view.MotionEvent): Boolean {
        val result = super.dispatchTouchEvent(ev)
        if (!dispatcherConfigured) configureEventDispatcher()
        return result
    }

    private fun configureEventDispatcher() {
        try {
            val templateRenderField = LynxView::class.java.getDeclaredField("mLynxTemplateRender")
            templateRenderField.isAccessible = true
            val templateRender = templateRenderField.get(this) ?: return

            val uiRenderField = templateRender.javaClass.getDeclaredField("mLynxUIRender")
            uiRenderField.isAccessible = true
            val uiRenderer = uiRenderField.get(templateRender) ?: return

            val dispatcherField = uiRenderer.javaClass.getDeclaredField("mEventDispatcher")
            dispatcherField.isAccessible = true
            val dispatcher = dispatcherField.get(uiRenderer) as? TouchEventDispatcher ?: return

            dispatcher.setTapSlop(TAP_SLOP_DP * resources.displayMetrics.density)
            // Off by default, and reachable only from the same
            // page-config pipeline as tapSlop. While off, a second
            // finger is dropped before any handler sees it, so
            // `TouchEvent.touches` can never hold more than one point
            // and a pinch is indistinguishable from a drag.
            dispatcher.setEnableMultiTouch(true)
            dispatcherConfigured = true
            Log.d(
                "WhiskerTouch",
                "forced tapSlop to ${TAP_SLOP_DP}dp and enabled multi-touch on live dispatcher",
            )
        } catch (e: Exception) {
            Log.e(
                "WhiskerTouch",
                "configureEventDispatcher failed — tapSlop stays at Lynx's 50dip default " +
                    "and touches stay single-finger",
                e,
            )
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
