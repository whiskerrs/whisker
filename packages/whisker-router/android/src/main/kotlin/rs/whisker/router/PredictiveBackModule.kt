// Phase L-3 — Android predictive back gesture, Kotlin half.
//
// Subscribes to the host Activity's `OnBackPressedDispatcher`
// while at least one Rust subscriber is registered against the
// `backInvoked` event. Rust-side, `AndroidPredictiveBack` in
// `packages/whisker-router/src/gestures/android_predictive_back.rs`
// is the consumer — it reads `StackLayoutHandle` from context and
// calls `handle.commit_preview_and_back()` when the event fires.
//
// We use `androidx.activity.OnBackPressedDispatcher` rather than the
// raw `Activity.onBackInvokedDispatcher` (API 33+) so the same code
// path works on minSdk=21 — androidx routes through the platform
// dispatcher on 33+ automatically.
//
// ## Events
//
// * `backInvoked` — fired once when the back gesture commits. No
//   payload (`.null`). Mirrors the iOS swipe-back commit point.
//
// Predictive-back progress events (`backStarted` / `backProgressed`
// / `backCancelled`) for interactive preview animation aren't wired
// in this first pass — the simpler commit-only event is enough to
// drive `StackLayoutHandle::commit_preview_and_back`. Add the
// progress hooks once we want a Rust-side preview pose during the
// drag.
//
// ## Lifecycle
//
// `OnStartObserving("backInvoked")` registers the callback against
// the current host Activity (resolved via `appContext.currentActivity`).
// `OnStopObserving("backInvoked")` removes it. If no subscriber is
// active, no callback is registered — the host's normal back
// behaviour applies (e.g. finishing the Activity).

package rs.whisker.router

import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import rs.whisker.runtime.HostAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition

/**
 * `whisker-router:PredictiveBack` module. View-less — registers
 * itself via the dispatch table; the only public surface is the
 * `backInvoked` event.
 */
public class PredictiveBackModule : Module() {

    /**
     * Live `OnBackPressedCallback`. `null` between the
     * `OnStopObserving` tearing it down and the next
     * `OnStartObserving` re-registering. Reset to `null` after
     * `remove()` so a second StartObserving creates a fresh
     * callback bound to the (possibly rotated) Activity's
     * dispatcher.
     */
    private var callback: OnBackPressedCallback? = null
    /**
     * Pending registration listener. Set when `OnStartObserving`
     * fires before the WhiskerView attaches to its window — the
     * listener fires once the host becomes available and finishes
     * the deferred `addCallback`. Cleared on `OnStopObserving` to
     * keep `OnBackPressedCallback` lifetime tied to the Rust-side
     * subscription.
     */
    private var pendingAttachListener: HostAttachedListener? = null

    override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("PredictiveBack")
        Events("backInvoked")

        OnStartObserving("backInvoked") {
            if (callback != null) return@OnStartObserving
            if (tryRegisterCallback()) return@OnStartObserving

            // The Rust runtime renders the first tree from inside
            // WhiskerView's constructor (before `onAttachedToWindow`
            // fires), so on cold start the host Activity isn't
            // resolvable yet. Defer the dispatcher registration
            // until WhiskerAppContext signals attach.
            val listener = HostAttachedListener {
                if (callback != null) return@HostAttachedListener
                tryRegisterCallback()
            }
            pendingAttachListener = listener
            appContext.addOnHostAttachedListener(listener)
        }

        OnStopObserving("backInvoked") {
            pendingAttachListener?.let {
                appContext.removeOnHostAttachedListener(it)
            }
            pendingAttachListener = null
            callback?.remove()
            callback = null
        }
    }

    /**
     * Attempt to register `OnBackPressedCallback` against the host
     * Activity's dispatcher. Returns true if registration succeeded
     * (or the callback was already installed); false if the host
     * isn't currently attached.
     */
    private fun tryRegisterCallback(): Boolean {
        if (callback != null) return true
        val activity = appContext.currentActivity as? ComponentActivity ?: return false
        val cb = object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                sendEvent("backInvoked")
            }
        }
        activity.onBackPressedDispatcher.addCallback(cb)
        callback = cb
        return true
    }
}
