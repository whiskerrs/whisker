// `WhiskerInsetsDispatcher` — the single owner of the host Activity's
// `decorView.setOnApplyWindowInsetsListener` slot, fanned out to any
// number of module subscribers.
//
// ## Why this exists
//
// Android stores exactly ONE `OnApplyWindowInsetsListener` per view
// (`View.ListenerInfo.mOnApplyWindowInsetsListener`) — the setter
// overwrites, it does not append. Two modules both calling
// `ViewCompat.setOnApplyWindowInsetsListener` on the same `decorView`
// means whichever installs last silently clobbers the other, freezing
// its inset signal for good (IME show/hide doesn't recreate the
// Activity, so nothing re-installs the lost listener).
//
// This dispatcher owns the single slot and multiplexes it: every module
// that wants insets calls [addListener] and receives the same
// `WindowInsetsCompat` the platform delivers. No module may touch the
// decor slot directly.
//
// The public surface is `(WindowInsetsCompat) -> Unit` — the dispatcher
// knows nothing about IME vs system-bars vs cutout. Each subscriber
// reads whatever `Type` it cares about from the raw insets.
//
// ## Lifecycle + configuration-change handling
//
// The decor view is replaced on every config-change Activity recreation
// (rotation, multi-window resize). We rewire transparently by owning a
// single [RuntimeAttachedListener]: registered lazily on the 0→1
// subscriber transition, it (re)installs the inset listener on whichever
// decor view is current each time a host attaches, and is dropped on the
// 1→0 transition. One host listener and one decor slot regardless of how
// many modules subscribe.

package rs.whisker.runtime

import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import java.util.concurrent.CopyOnWriteArrayList

public object WhiskerInsetsDispatcher {

    /**
     * Opaque handle returned by [addListener]. Pass it back to
     * [removeListener] to unsubscribe. Holds the callback so the
     * dispatcher can fan out to it.
     */
    public class Registration internal constructor(
        internal val callback: (WindowInsetsCompat) -> Unit,
    )

    /**
     * Live subscribers. `CopyOnWriteArrayList` so the decor-view
     * listener (UI thread) can iterate while `add`/`removeListener`
     * (possibly the runtime thread, from `OnStartObserving`) mutate.
     */
    private val listeners: CopyOnWriteArrayList<Registration> = CopyOnWriteArrayList()

    /** Guards the 0↔1 host-listener register/unregister transitions. */
    private val lock = Any()

    /**
     * The permanent host-attached listener — non-null exactly while at
     * least one subscriber is registered. Its body re-installs the inset
     * listener on the current decor view, so a config-change recreation
     * transparently rewires.
     */
    private var hostListener: RuntimeAttachedListener? = null

    private val appContext: WhiskerAppContext get() = WhiskerAppContext.shared

    /**
     * Subscribe to decor-view window insets. The callback fires on the
     * UI thread with the raw `WindowInsetsCompat` every time the platform
     * dispatches insets, plus once synchronously with the current insets
     * if a host is already attached (so a late subscriber doesn't sit at
     * its default until the next genuine change).
     */
    public fun addListener(callback: (WindowInsetsCompat) -> Unit): Registration {
        val registration = Registration(callback)
        var listenerToRegister: RuntimeAttachedListener? = null
        synchronized(lock) {
            val wasEmpty = listeners.isEmpty()
            listeners.add(registration)
            if (wasEmpty) {
                val listener = RuntimeAttachedListener { installOnCurrentHost() }
                hostListener = listener
                listenerToRegister = listener
            }
        }
        val toRegister = listenerToRegister
        if (toRegister != null) {
            // 0→1: register the host listener OUTSIDE the lock (its body
            // installs the decor slot and seeds every subscriber, incl.
            // this one, if a host is already attached).
            appContext.addOnRuntimeAttachedListener(toRegister)
        } else {
            // 1→N: the decor slot is already installed and seeds only
            // fire on install / genuine change, so seed just the new
            // subscriber with the current insets.
            seedInto(registration)
        }
        return registration
    }

    /**
     * Unsubscribe. On the 1→0 transition the host listener is dropped.
     * We deliberately leave the decor view's inset listener in place —
     * with no subscribers its fan-out loop is a harmless no-op, and the
     * next 0→1 transition re-installs it on the current host anyway.
     */
    public fun removeListener(registration: Registration) {
        var listenerToRemove: RuntimeAttachedListener? = null
        synchronized(lock) {
            listeners.remove(registration)
            if (listeners.isEmpty()) {
                listenerToRemove = hostListener
                hostListener = null
            }
        }
        listenerToRemove?.let { appContext.removeOnRuntimeAttachedListener(it) }
    }

    /**
     * Install (or re-install) the inset listener on the current host
     * Activity's decor view and seed every subscriber with the current
     * insets. Runs on each host attach.
     */
    private fun installOnCurrentHost() {
        val decor = appContext.currentActivity?.window?.decorView ?: return

        ViewCompat.setOnApplyWindowInsetsListener(decor) { _, insets ->
            for (l in listeners) l.callback(insets)
            // Pass through unmodified so consumers further down the view
            // tree (e.g. application listeners) still see the same insets.
            insets
        }

        // Seed all current subscribers so a freshly-attached (or rotated)
        // host doesn't sit at defaults until the next genuine dispatch.
        // `getRootWindowInsets` returns null until first measure — fine,
        // the listener above picks up the first real fire.
        ViewCompat.getRootWindowInsets(decor)?.let { insets ->
            for (l in listeners) l.callback(insets)
        }
    }

    /** Seed a single freshly-added subscriber with the current insets. */
    private fun seedInto(registration: Registration) {
        val decor = appContext.currentActivity?.window?.decorView ?: return
        ViewCompat.getRootWindowInsets(decor)?.let { registration.callback(it) }
    }
}
