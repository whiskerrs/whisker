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

import android.os.Build
import android.view.RoundedCorner
import androidx.activity.BackEventCompat
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import rs.whisker.runtime.HostAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

/** Fallback corner radius (dp) for devices that don't expose the
 *  `RoundedCorner` API (< API 31) or report a square display. Matches
 *  the Material predictive-back default. */
private const val DEFAULT_CORNER_RADIUS_DP = 24.0

/** Sentinel for "the host Activity/insets aren't attached yet" — a
 *  transient condition the caller should retry, distinct from the
 *  permanent [DEFAULT_CORNER_RADIUS_DP] fallback. Negative so Rust can
 *  treat any `<= 0` reading as "not ready, don't latch". */
private const val NOT_READY = -1.0

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

        // Static device info: the display's top-left rounded-corner radius
        // in dp, so the predictive-back preview can round its card to match
        // the screen. Rust calls this once (not per frame) and caches it.
        // Returns [NOT_READY] (negative) when the host Activity/insets
        // aren't attached yet, so Rust keeps the default and retries on the
        // first gesture instead of latching the early fallback.
        Function("getDeviceCornerRadius") { _ ->
            WhiskerValue.Float(deviceCornerRadiusDp())
        }
    }

    /**
     * The display's top-left rounded-corner radius in **dp** (px / density),
     * via the API 31+ `RoundedCorner` API.
     *
     * Distinguishes two kinds of "no real reading":
     *  - **Transient** (host Activity / decor / insets not attached yet):
     *    returns [NOT_READY] (negative) so the caller doesn't latch and
     *    retries once the Activity is up.
     *  - **Permanent** (API < 31, or a genuinely square display): returns
     *    [DEFAULT_CORNER_RADIUS_DP] so the caller latches a sane default.
     */
    private fun deviceCornerRadiusDp(): Double {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return DEFAULT_CORNER_RADIUS_DP
        val activity = appContext.currentActivity as? ComponentActivity ?: return NOT_READY
        val decor = activity.window?.decorView ?: return NOT_READY
        val insets = decor.rootWindowInsets ?: return NOT_READY
        val radiusPx = insets.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)?.radius ?: 0
        if (radiusPx <= 0) return DEFAULT_CORNER_RADIUS_DP
        val density = activity.resources.displayMetrics.density.takeIf { it > 0f } ?: 1f
        return (radiusPx / density).toDouble()
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
