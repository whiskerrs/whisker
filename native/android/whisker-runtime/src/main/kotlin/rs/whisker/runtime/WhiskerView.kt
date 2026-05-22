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
import org.json.JSONArray
import org.json.JSONObject

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
) : LynxView(context, LynxViewBuilder()) {

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
            // Phase 7-Φ.C: serialise the event's params (if any) into
            // a JSON string so the bridge can hand it through to Rust
            // `on_<event>: String` callbacks declared via
            // `#[whisker::native_element]`. Mirrors what the iOS
            // event reporter does with `LynxEvent.generateEventBody`
            // + `NSJSONSerialization`.
            //
            // `LynxCustomEvent` is the only public LynxEvent subclass
            // that carries a params dict; touch / mouse / wheel /
            // keyboard events don't currently surface a payload
            // through here. Pass empty payload for those — the
            // bridge normalises NULL → "" anyway, but going through
            // `""` keeps the JNI call's String non-null.
            val payload = if (event is LynxCustomEvent) {
                paramsToJson(event.eventParams())
            } else {
                ""
            }
            return nativeOnLynxEvent(engine, event.tag, name, payload)
        }
        override fun onInternalEvent(event: LynxInternalEvent) {}
    }

    /**
     * Best-effort serialise a Lynx event-params dict to a UTF-8 JSON
     * string. `org.json.JSONObject` accepts heterogeneous value types
     * (String / Number / Boolean / nested Map / List) at runtime via
     * casting in `JSONObject.put(name, value)`, but `Map<String, Any?>`
     * specifically needs each value individually wrapped or it ends up
     * as `{"key":"java.util.HashMap@…"}` (toString fallback).
     *
     * Returns the empty string when [params] is null or empty — same
     * as the iOS path's `""` default.
     */
    private fun paramsToJson(params: Map<String, Any?>?): String {
        if (params.isNullOrEmpty()) return ""
        return try {
            mapToJsonObject(params).toString()
        } catch (e: Throwable) {
            // Swallow — degrade to empty payload rather than crash
            // the event-reporter chain.
            ""
        }
    }

    private fun mapToJsonObject(map: Map<String, Any?>): JSONObject {
        val out = JSONObject()
        for ((k, v) in map) {
            out.put(k, jsonifyValue(v))
        }
        return out
    }

    @Suppress("UNCHECKED_CAST")
    private fun jsonifyValue(v: Any?): Any {
        return when (v) {
            null -> JSONObject.NULL
            is Map<*, *> -> mapToJsonObject(v as Map<String, Any?>)
            is List<*> -> {
                val arr = JSONArray()
                for (item in v) arr.put(jsonifyValue(item))
                arr
            }
            is Array<*> -> {
                val arr = JSONArray()
                for (item in v) arr.put(jsonifyValue(item))
                arr
            }
            // JSONObject.put accepts these directly.
            is String, is Number, is Boolean -> v
            // Fallback — let JSONObject use toString().
            else -> v.toString()
        }
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
        super.destroy()
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
        payloadJson: String,
    ): Boolean
}
