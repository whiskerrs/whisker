package dev.tuft.runtime

import android.content.Context
import android.os.Looper
import android.util.AttributeSet
import android.view.Choreographer
import androidx.annotation.Keep
import com.lynx.tasm.EventEmitter
import com.lynx.tasm.LynxView
import com.lynx.tasm.LynxViewBuilder
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
class TuftView @JvmOverloads constructor(
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
            return nativeOnLynxEvent(engine, event.tag, name)
        }
        override fun onInternalEvent(event: LynxInternalEvent) {}
    }

    init {
        engine = nativeEngineAttach(this)
        if (engine != 0L) {
            nativeBindTuftView(engine)
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
    // wraps it in a `TuftEngine`, and returns the engine handle.
    private external fun nativeEngineAttach(view: LynxView): Long
    private external fun nativeBindTuftView(engine: Long)
    private external fun nativeAppMain(engine: Long)
    private external fun nativeTick(engine: Long): Boolean
    private external fun nativeEngineRelease(engine: Long)
    private external fun nativeOnLynxEvent(engine: Long, tag: Int, name: String): Boolean
}
