// Module-side host context (Android).
//
// Modelled on Expo's `appContext.currentActivity`. A `Module` body
// reaches the host Activity via:
//
//   @WhiskerModule
//   class PredictiveBackModule : Module() {
//       override fun definition() = ModuleDefinition {
//           Name("PredictiveBack")
//           OnStartObserving("backInvoked") {
//               val act = appContext.currentActivity ?: return@OnStartObserving
//               act.onBackInvokedDispatcher.registerOnBackInvokedCallback(...)
//           }
//       }
//   }
//
// Whisker isn't Activity-centric — a host app can embed `WhiskerView`
// inside any Activity / Fragment / Compose AndroidView / Dialog /
// PopupWindow. So the host that publishes the Activity reference is
// the **WhiskerView itself**: it implements [WhiskerRuntimeOwner] and
// pushes itself onto [WhiskerAppContext]'s host stack on
// `onAttachedToWindow`, popping on `onDetachedFromWindow`.
//
// `appContext.currentActivity` then resolves the live host's
// `context`, unwrapping any `ContextWrapper` chain (Dialog, Compose,
// PopupWindow all hide the Activity behind one or more wrappers).
//
// Caveats — the getter returns `null` in these cases (Module body
// must null-check):
//
//   * View is not (yet) attached to any window.
//   * View's context unwraps to an `Application` / `Service`
//     context, not an `Activity` (e.g. headless integration test).
//   * Host Activity has been destroyed but the WeakReference hasn't
//     been cleared yet (avoid by always clearing in
//     `onDetachedFromWindow` AND `destroy()`).
//
// All other cases (multiple WhiskerViews, config-change Activity
// rotation, Compose AndroidView nesting) are handled by the LIFO
// stack + ContextWrapper unwrap.

package rs.whisker.runtime

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import java.lang.ref.WeakReference
import java.util.concurrent.CopyOnWriteArrayList

/**
 * Listener registered via
 * [WhiskerAppContext.addOnRuntimeAttachedListener]. Fires every time a
 * `WhiskerRuntimeOwner` attaches (and once at registration time if a
 * host is already attached). Used by modules whose
 * `OnStartObserving` hook runs before any host is available — they
 * defer their Activity-touching wiring through this hook.
 *
 * Modeled as a SAM interface rather than `() -> Unit` so a Java
 * caller can implement it without a Kotlin function-type adapter.
 */
public fun interface RuntimeAttachedListener {
    public fun onRuntimeAttached()
}

/** Fires with the full URL when a deep link arrives via `onNewIntent`. */
public fun interface DeepLinkListener {
    public fun onDeepLink(url: String)
}

/**
 * Marker interface a Whisker module host implements to publish
 * itself as a candidate for `appContext.currentActivity`. The
 * default implementer is `WhiskerView`, but a custom host (e.g. a
 * unit-test harness or a non-View embedding) can implement this too.
 */
public interface WhiskerRuntimeOwner {
    /**
     * The host's Android `Context`. [WhiskerAppContext.currentActivity]
     * walks the `ContextWrapper` chain to find the underlying
     * Activity, so any Activity-derived context works.
     *
     * Named `runtimeContext` (not `context`) so a `View` implementer
     * doesn't collide with `View.getContext()` on the JVM signature.
     */
    public val runtimeContext: Context
}

/**
 * Process-wide host registry + `currentActivity` accessor.
 *
 * Singleton accessed via [shared]. `Module.appContext` returns this
 * singleton, mirroring Expo's `Module.appContext` property.
 */
public class WhiskerAppContext internal constructor() {

    /**
     * The host Activity for the currently-foremost
     * [WhiskerRuntimeOwner], or `null` if none is attached / the
     * host's context doesn't unwrap to an Activity.
     *
     * Request-time lookup — the Activity reference is never stored
     * on the Module itself, so a config-change rotation transparently
     * resolves to the new Activity once the new WhiskerView attaches.
     */
    public val currentActivity: Activity?
        get() = currentRuntimeOwner()?.runtimeContext?.let { unwrapActivity(it) }

    /**
     * Register `listener` to fire whenever a [WhiskerRuntimeOwner]
     * attaches (i.e. `currentActivity` becomes non-null). If a host
     * is already attached at registration time, fires synchronously
     * once.
     *
     * Required for modules whose `OnStartObserving` hook runs
     * BEFORE the WhiskerView attaches to its window — the Rust
     * runtime's first render fires during `WhiskerView`'s
     * constructor (inside `nativeAppMain`), but
     * `onAttachedToWindow` only fires after `setContentView`
     * inflates the view hierarchy. `rs.whisker.router`'s
     * `PredictiveBackModule` defers its `addCallback` against the
     * host Activity through this hook.
     *
     * Caller is responsible for [removeOnRuntimeAttachedListener] —
     * stale listeners pin nothing (the listener list holds Kotlin
     * lambdas, not module references), but firing them after
     * `OnStopObserving` would re-register a torn-down handler.
     */
    public fun addOnRuntimeAttachedListener(listener: RuntimeAttachedListener) {
        attachListeners.add(listener)
        if (currentActivity != null) listener.onRuntimeAttached()
    }

    public fun removeOnRuntimeAttachedListener(listener: RuntimeAttachedListener) {
        attachListeners.remove(listener)
    }

    /** Listeners registered via [addOnRuntimeAttachedListener]. */
    private val attachListeners: CopyOnWriteArrayList<RuntimeAttachedListener> =
        CopyOnWriteArrayList()

    public companion object {
        private val deepLinkListeners: CopyOnWriteArrayList<DeepLinkListener> =
            CopyOnWriteArrayList()

        /** Called by [rs.whisker.runtime.WhiskerActivity.onNewIntent]. */
        @JvmStatic
        public fun dispatchDeepLink(url: String) {
            for (l in deepLinkListeners) l.onDeepLink(url)
        }

        @JvmStatic
        public fun addDeepLinkListener(listener: DeepLinkListener) {
            deepLinkListeners.add(listener)
        }

        @JvmStatic
        public fun removeDeepLinkListener(listener: DeepLinkListener) {
            deepLinkListeners.remove(listener)
        }

        /** The single instance every [Module] reaches via `appContext`. */
        @JvmStatic
        public val shared: WhiskerAppContext = WhiskerAppContext()

        // LIFO stack of WeakReferences. Most recently attached host
        // wins (handles nested WhiskerView embeds + the common case
        // of a single-host app). Weak refs so a misbehaving host
        // that forgets to pop doesn't pin its Activity.
        private val hostsLock = Any()
        private val hosts: MutableList<WeakReference<WhiskerRuntimeOwner>> =
            mutableListOf()

        /**
         * Register `owner` as the current top-of-stack runtime owner.
         * Idempotent — an owner that's already in the stack moves to
         * the top (LIFO refresh).
         */
        @JvmStatic
        public fun pushRuntimeOwner(owner: WhiskerRuntimeOwner) {
            synchronized(hostsLock) {
                // Drop any stale weak ref to this same owner before pushing.
                hosts.removeAll { it.get() === owner || it.get() == null }
                hosts.add(WeakReference(owner))
            }
            // Fire host-attached listeners OUTSIDE the lock (their
            // bodies may call back into the AppContext).
            for (l in shared.attachListeners) l.onRuntimeAttached()
        }

        /**
         * Unregister `owner`. Safe to call multiple times. Also
         * sweeps stale (already-collected) weak refs.
         */
        @JvmStatic
        public fun popRuntimeOwner(owner: WhiskerRuntimeOwner) {
            synchronized(hostsLock) {
                hosts.removeAll { it.get() === owner || it.get() == null }
            }
        }

        private fun currentRuntimeOwner(): WhiskerRuntimeOwner? {
            synchronized(hostsLock) {
                // Walk from the top until we find a live ref. Sweep
                // dead refs as we go so the list doesn't grow.
                while (hosts.isNotEmpty()) {
                    val top = hosts.last().get()
                    if (top != null) return top
                    hosts.removeAt(hosts.lastIndex)
                }
                return null
            }
        }

        private fun unwrapActivity(ctx: Context): Activity? {
            // Dialog, Compose AndroidView, PopupWindow, and several
            // theme overlays all wrap the host Activity in one or
            // more ContextWrappers. Walk until we hit an Activity or
            // a non-wrapper terminator.
            var c: Context? = ctx
            while (c is ContextWrapper) {
                if (c is Activity) return c
                c = c.baseContext
            }
            return null
        }
    }
}
