package rs.whisker.runtime

import android.content.Context
import android.view.View
import android.view.ViewGroup

/** Context supplied when a module element is mounted by `WhiskerView`. */
public typealias WhiskerContext = Context

/** Lightweight native-element base class with no renderer SDK dependency. */
public abstract class WhiskerUI<V : View>(context: Context) : WhiskerContainerView(context) {
    private val nativeView: V = createView(context)
    private var eventSink: ((String, WhiskerValue) -> Unit)? = null

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

    internal fun installWhiskerEventSink(sink: ((String, WhiskerValue) -> Unit)?) {
        eventSink = sink
    }

    public fun emitWhiskerEvent(name: String, detail: WhiskerValue = WhiskerValue.Null) {
        eventSink?.invoke(name, detail)
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
