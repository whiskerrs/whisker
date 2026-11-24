package rs.whisker.runtime

import android.os.Handler
import android.os.Looper

/**
 * Holds the [WhiskerView] whose runtime outlived its destroyed Activity.
 *
 * The next [WhiskerView.obtain] takes it back. When the last [WhiskerRuntimeRetention] lease is
 * released while a view is still held, the view's runtime is destroyed on the main thread. The
 * destroy is posted so it never re-enters Rust from inside a module call that released the lease.
 * All members are main-thread only.
 */
internal object WhiskerViewRetainer {
    private val mainHandler = Handler(Looper.getMainLooper())
    private var held: WhiskerView? = null
    private var listening = false

    private val releasedListener = RuntimeRetentionReleasedListener {
        mainHandler.post(::destroyIfUnrequested)
    }

    fun hold(view: WhiskerView) {
        val previous = held
        held = view
        if (!listening) {
            listening = true
            WhiskerRuntimeRetention.addOnReleasedListener(releasedListener)
        }
        if (previous != null && previous !== view) previous.destroy()
        // The lease may have been released between the decision to retain and this call.
        destroyIfUnrequested()
    }

    fun take(): WhiskerView? {
        val view = held ?: return null
        held = null
        return view
    }

    fun forget(view: WhiskerView) {
        if (held === view) held = null
    }

    private fun destroyIfUnrequested() {
        if (WhiskerRuntimeRetention.isRequested) return
        val view = held ?: return
        held = null
        view.destroy()
    }
}
