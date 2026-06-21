// Android predictive-back gesture, Kotlin half.
//
// Subscribes to the host Activity's `OnBackPressedDispatcher` while at
// least one Rust subscriber is registered. Rust-side, the new
// `AndroidPredictiveBack` component (packages/whisker-router/src/render/
// gesture.rs) drives the SAME coordinated two-screen scrub the iOS
// `SwipeBack` uses: it reads the active stack's transition controller
// from the router and animates the outgoing/revealed screens by the
// gesture progress.
//
// We use `androidx.activity.OnBackPressedDispatcher` (not the raw
// `Activity.onBackInvokedDispatcher`, API 33+) so one code path works on
// minSdk=21 — androidx routes through the platform dispatcher on 33+
// automatically, and exposes the predictive-back progress via
// `BackEventCompat`.
//
// ## Events
//
// * `backStarted`   — the predictive-back gesture began (API 34+).
// * `backProgressed` — fired each gesture frame; payload `{ progress:
//   0..1, swipeEdge: Int }` from `BackEventCompat`.
// * `backCancelled` — the gesture was released below the commit
//   threshold (API 34+).
// * `backInvoked`   — the back committed. Mirrors the iOS swipe-back
//   commit point. **This is the only event on API < 34** (the
//   `BackEventCompat` hooks are not called there), so commit-only back
//   still works — just without the interactive preview.
//
// ## Lifecycle
//
// `OnStartObserving` registers the callback against the current host
// Activity (resolved via `appContext.currentActivity`).
// `OnStopObserving` removes it. If no subscriber is active, no callback
// is registered — the host's normal back behaviour applies.

package rs.whisker.router

import android.util.Log
import androidx.activity.BackEventCompat
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import rs.whisker.runtime.HostAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

// DIAG (temporary): logcat tag for tracing predictive-back wiring.
// Filter with `adb logcat -s WhiskerPB`. Remove once verified on device.
private const val TAG = "WhiskerPB"

/**
 * `whisker-router:PredictiveBack` module. View-less — registers itself
 * via the dispatch table; the public surface is the four back events.
 */
public class PredictiveBackModule : Module() {

    /**
     * Live `OnBackPressedCallback`. `null` between the `OnStopObserving`
     * tearing it down and the next `OnStartObserving` re-registering.
     */
    private var callback: OnBackPressedCallback? = null
    /**
     * Pending registration listener. Set when `OnStartObserving` fires
     * before the WhiskerView attaches to its window — fires once the host
     * becomes available and finishes the deferred `addCallback`.
     */
    private var pendingAttachListener: HostAttachedListener? = null

    override fun definition(): ModuleDefinition = ModuleDefinition {
        Log.e(TAG, "definition() built — PredictiveBack module instantiated")
        Name("PredictiveBack")
        Events("backStarted", "backProgressed", "backCancelled", "backInvoked")

        // Any of the four events being observed registers the single
        // dispatcher callback; `backProgressed` is the canonical one the
        // Rust component subscribes first. Registration is idempotent.
        OnStartObserving("backProgressed") {
            Log.e(TAG, "OnStartObserving(backProgressed)")
            ensureRegistered()
        }
        OnStartObserving("backStarted") {
            Log.e(TAG, "OnStartObserving(backStarted)")
            ensureRegistered()
        }
        OnStartObserving("backCancelled") {
            Log.e(TAG, "OnStartObserving(backCancelled)")
            ensureRegistered()
        }
        OnStartObserving("backInvoked") {
            Log.e(TAG, "OnStartObserving(backInvoked)")
            ensureRegistered()
        }

        OnStopObserving("backProgressed") { teardown() }
        OnStopObserving("backStarted") { teardown() }
        OnStopObserving("backCancelled") { teardown() }
        OnStopObserving("backInvoked") { teardown() }
    }

    private fun ensureRegistered() {
        if (callback != null) {
            Log.e(TAG, "ensureRegistered: callback already present")
            return
        }
        if (tryRegisterCallback()) return

        // The Rust runtime renders the first tree from inside
        // WhiskerView's constructor (before `onAttachedToWindow`), so on
        // cold start the host Activity isn't resolvable yet. Defer the
        // dispatcher registration until WhiskerAppContext signals attach.
        if (pendingAttachListener != null) {
            Log.e(TAG, "ensureRegistered: host not attached; attach listener already pending")
            return
        }
        Log.e(TAG, "ensureRegistered: host not attached yet — deferring via addOnHostAttachedListener")
        val listener = HostAttachedListener {
            Log.e(TAG, "host attached — retrying callback registration")
            if (callback != null) return@HostAttachedListener
            tryRegisterCallback()
        }
        pendingAttachListener = listener
        appContext.addOnHostAttachedListener(listener)
    }

    private fun teardown() {
        pendingAttachListener?.let { appContext.removeOnHostAttachedListener(it) }
        pendingAttachListener = null
        callback?.remove()
        callback = null
    }

    /**
     * Register `OnBackPressedCallback` (with predictive-back hooks)
     * against the host Activity's dispatcher. Returns true if registered
     * (or already installed); false if the host isn't attached yet.
     *
     * The `handleOnBackStarted` / `handleOnBackProgressed` /
     * `handleOnBackCancelled` overrides are only invoked on API 34+; on
     * older platforms only `handleOnBackPressed` (commit) fires, so the
     * component gracefully degrades to commit-only.
     */
    private fun tryRegisterCallback(): Boolean {
        if (callback != null) return true
        val activity = appContext.currentActivity as? ComponentActivity
        if (activity == null) {
            Log.e(TAG, "tryRegisterCallback: currentActivity is null / not a ComponentActivity")
            return false
        }
        val cb = object : OnBackPressedCallback(true) {
            override fun handleOnBackStarted(backEvent: BackEventCompat) {
                Log.e(TAG, "handleOnBackStarted")
                sendEvent("backStarted")
            }

            override fun handleOnBackProgressed(backEvent: BackEventCompat) {
                Log.e(TAG, "handleOnBackProgressed progress=${backEvent.progress}")
                sendEvent(
                    "backProgressed",
                    WhiskerValue.Map(
                        mapOf(
                            "progress" to WhiskerValue.Float(backEvent.progress.toDouble()),
                            "swipeEdge" to WhiskerValue.Int(backEvent.swipeEdge.toLong()),
                        ),
                    ),
                )
            }

            override fun handleOnBackCancelled() {
                Log.e(TAG, "handleOnBackCancelled")
                sendEvent("backCancelled")
            }

            override fun handleOnBackPressed() {
                Log.e(TAG, "handleOnBackPressed (commit)")
                sendEvent("backInvoked")
            }
        }
        activity.onBackPressedDispatcher.addCallback(cb)
        Log.e(TAG, "registered OnBackPressedCallback on dispatcher (activity=${activity.javaClass.simpleName})")
        callback = cb
        return true
    }
}
