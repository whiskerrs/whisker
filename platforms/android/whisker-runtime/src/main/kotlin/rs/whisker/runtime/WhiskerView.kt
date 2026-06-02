package rs.whisker.runtime

import android.content.Context
import android.os.Looper
import android.util.AttributeSet
import android.view.Choreographer
import androidx.annotation.Keep
import com.lynx.tasm.EventEmitter
import com.lynx.tasm.LynxView
import com.lynx.tasm.LynxViewBuilder
import com.lynx.tasm.event.LynxCustomEvent
import com.lynx.tasm.event.LynxEvent
import com.lynx.tasm.event.LynxInternalEvent
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
) : LynxView(context, LynxViewBuilder()), WhiskerModuleHost {

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
            // `LynxCustomEvent` (scroll / layout / …), null otherwise
            // (touch events carry no body on Android). The marshaller
            // turns this Java map into a `WhiskerValueRaw` tree (no JSON).
            val detail: Map<String, Any?>? =
                if (event is LynxCustomEvent) event.eventParams() else null
            val body: Map<String, Any?> =
                mapOf(
                    "type" to name,
                    "target" to event.tag,
                    "currentTarget" to event.tag,
                    "detail" to detail,
                )
            return nativeOnLynxEvent(engine, event.tag, name, body)
        }
        override fun onInternalEvent(event: LynxInternalEvent) {}
    }

    init {
        engine = nativeEngineAttach(this)
        if (engine != 0L) {
            nativeBindWhiskerView(engine)
            installEventReporter()
            nativeAppMain(engine)
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
