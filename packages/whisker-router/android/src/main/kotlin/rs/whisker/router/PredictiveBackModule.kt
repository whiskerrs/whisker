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

import androidx.activity.BackEventCompat
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import rs.whisker.runtime.HostAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

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
        Name("PredictiveBack")
        Events("backStarted", "backProgressed", "backCancelled", "backInvoked")

        // Any of the four events being observed registers the single
        // dispatcher callback; `backProgressed` is the canonical one the
        // Rust component subscribes first. Registration is idempotent.
        OnStartObserving("backProgressed") { ensureRegistered() }
        OnStartObserving("backStarted") { ensureRegistered() }
        OnStartObserving("backCancelled") { ensureRegistered() }
        OnStartObserving("backInvoked") { ensureRegistered() }

        OnStopObserving("backProgressed") { teardown() }
        OnStopObserving("backStarted") { teardown() }
        OnStopObserving("backCancelled") { teardown() }
        OnStopObserving("backInvoked") { teardown() }
    }

    private fun ensureRegistered() {
        if (callback != null) return
        if (tryRegisterCallback()) return

        // The Rust runtime renders the first tree from inside
        // WhiskerView's constructor (before `onAttachedToWindow`), so on
        // cold start the host Activity isn't resolvable yet. Defer the
        // dispatcher registration until WhiskerAppContext signals attach.
        if (pendingAttachListener != null) return
        val listener = HostAttachedListener {
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
        val activity = appContext.currentActivity as? ComponentActivity ?: return false
        val cb = object : OnBackPressedCallback(true) {
            override fun handleOnBackStarted(backEvent: BackEventCompat) {
                sendEvent("backStarted", edgePayload(backEvent))
            }

            override fun handleOnBackProgressed(backEvent: BackEventCompat) {
                sendEvent("backProgressed", progressPayload(backEvent))
            }

            override fun handleOnBackCancelled() {
                sendEvent("backCancelled")
            }

            override fun handleOnBackPressed() {
                sendEvent("backInvoked")
            }
        }
        activity.onBackPressedDispatcher.addCallback(cb)
        callback = cb
        return true
    }

    /** `{ swipeEdge }` — sent on `backStarted` so Rust knows the edge up
     *  front (drives the left/right Material pose). */
    private fun edgePayload(e: BackEventCompat): WhiskerValue =
        WhiskerValue.Map(mapOf("swipeEdge" to WhiskerValue.Int(e.swipeEdge.toLong())))

    /** `{ progress, swipeEdge }` — sent each `backProgressed` frame. */
    private fun progressPayload(e: BackEventCompat): WhiskerValue =
        WhiskerValue.Map(
            mapOf(
                "progress" to WhiskerValue.Float(e.progress.toDouble()),
                "swipeEdge" to WhiskerValue.Int(e.swipeEdge.toLong()),
            ),
        )
}
