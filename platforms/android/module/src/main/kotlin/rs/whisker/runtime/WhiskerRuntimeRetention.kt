// Process-wide requests to keep a Whisker runtime alive after its host Activity is destroyed (Android).
//
// Removing the task from Recents (or finishing the root Activity) destroys the Activity while the
// process keeps running for a foreground service, e.g. media playback. Without a request the
// `WhiskerView` destroys its Rust runtime with the Activity, so module events such as media
// notification buttons have nobody to reach. A native module that keeps background work running
// holds a [WhiskerRuntimeRetention.Lease] for as long as that work needs the runtime:
//
//   class PlaybackService : Service() {
//       private var lease: WhiskerRuntimeRetention.Lease? = null
//       override fun onCreate() { lease = WhiskerRuntimeRetention.acquire("playback") }
//       override fun onDestroy() { lease?.release(); lease = null }
//   }
//
// Retention only applies to apps that opt into `rs.whisker.runsInBackground` and to views created
// through `WhiskerView.obtain`. While retained, the view is detached from any window and keeps
// driving module events, tasks and reactive work without layout or presentation. The next Activity
// that calls `WhiskerView.obtain` adopts the same view and runtime. When the last lease is released,
// a runtime retained without a window is destroyed.

package rs.whisker.runtime

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean

/** Fires on the releasing thread when the last retention lease is released. */
public fun interface RuntimeRetentionReleasedListener {
    public fun onRetentionReleased()
}

/** Reference-counted requests to keep the Whisker runtime alive without a host Activity. */
public object WhiskerRuntimeRetention {
    /** One retention request. Releasing it more than once has no further effect. */
    public class Lease internal constructor(public val reason: String) {
        private val released = AtomicBoolean(false)

        public fun release() {
            if (released.compareAndSet(false, true)) WhiskerRuntimeRetention.releaseOne()
        }
    }

    private val lock = Any()
    private var count = 0
    private val releasedListeners = CopyOnWriteArrayList<RuntimeRetentionReleasedListener>()

    /** Requests retention until the returned lease is released. */
    @JvmStatic
    public fun acquire(reason: String): Lease {
        synchronized(lock) { count += 1 }
        return Lease(reason)
    }

    /** Whether any lease is currently held. */
    @JvmStatic
    public val isRequested: Boolean
        get() = synchronized(lock) { count > 0 }

    /** Registers a listener for the transition to no held leases. */
    @JvmStatic
    public fun addOnReleasedListener(listener: RuntimeRetentionReleasedListener) {
        releasedListeners.addIfAbsent(listener)
    }

    @JvmStatic
    public fun removeOnReleasedListener(listener: RuntimeRetentionReleasedListener) {
        releasedListeners.remove(listener)
    }

    private fun releaseOne() {
        val reachedZero = synchronized(lock) {
            count = (count - 1).coerceAtLeast(0)
            count == 0
        }
        if (reachedZero) releasedListeners.forEach { it.onRetentionReleased() }
    }

    /** Clears every lease and listener. Tests only. */
    internal fun resetForTesting() {
        synchronized(lock) { count = 0 }
        releasedListeners.clear()
    }
}
