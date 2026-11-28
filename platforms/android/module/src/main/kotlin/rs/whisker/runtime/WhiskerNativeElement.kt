package rs.whisker.runtime

import android.content.Context
import android.view.Choreographer
import android.view.View
import android.view.ViewGroup

/** Context supplied when a module element is mounted by `WhiskerView`. */
public typealias WhiskerContext = Context

/** Event source shared by class-backed module Views and built-in controls. */
public interface WhiskerEventSource {
    public fun installWhiskerEventSink(sink: ((String, WhiskerValue) -> Unit)?)
}

/** Implemented by the View that owns a Whisker runtime; runs requested View frames before its tick. */
public interface WhiskerFrameHost {
    public fun requestViewFrame(view: WhiskerUI<*>)
}

/** Lightweight native-element base class with no renderer SDK dependency. */
public abstract class WhiskerUI<V : View>(context: Context) :
    WhiskerContainerView(context), WhiskerEventSource {
    private val nativeView: V = createView(context)
    private var eventSink: ((String, WhiskerValue) -> Unit)? = null
    private var frameRequested = false
    private val fallbackFrame = Choreographer.FrameCallback { frameTimeNanos ->
        frameRequested = false
        onFrame(frameTimeNanos)
    }

    init {
        addView(
            nativeView,
            LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
    }

    protected abstract fun createView(context: Context): V

    public fun view(): V = nativeView

    final override fun installWhiskerEventSink(sink: ((String, WhiskerValue) -> Unit)?) {
        eventSink = sink
    }

    public fun emitWhiskerEvent(name: String, detail: WhiskerValue = WhiskerValue.Null) {
        eventSink?.invoke(name, detail)
    }

    /** Calls [onFrame] once on the next display frame, before Whisker applies that frame's updates. */
    public fun requestFrame() {
        if (frameRequested) return
        frameRequested = true
        if (isAttachedToWindow) forwardFrameRequest()
    }

    /** Runs outside Whisker's commit, so it may call into Rust; what it publishes lands in this frame. */
    public open fun onFrame(frameTimeNanos: Long) {}

    /** Called by [WhiskerFrameHost] when the requested frame runs. */
    public fun runWhiskerFrame(frameTimeNanos: Long) {
        if (!frameRequested || !isAttachedToWindow) return
        frameRequested = false
        onFrame(frameTimeNanos)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (frameRequested) forwardFrameRequest()
    }

    override fun onDetachedFromWindow() {
        Choreographer.getInstance().removeFrameCallback(fallbackFrame)
        super.onDetachedFromWindow()
    }

    private fun forwardFrameRequest() {
        var ancestor = parent
        while (ancestor != null) {
            if (ancestor is WhiskerFrameHost) {
                ancestor.requestViewFrame(this)
                return
            }
            ancestor = ancestor.parent
        }
        Choreographer.getInstance().postFrameCallback(fallbackFrame)
    }
}

/** Event helper used by module-owned native controls. */
public object WhiskerCustomEvent {
    @JvmStatic
    public fun dispatch(
        ui: WhiskerUI<*>,
        name: String,
        params: Map<String, Any?> = emptyMap(),
    ) {
        ui.emitWhiskerEvent(name, whiskerValueOf(params))
    }
}
