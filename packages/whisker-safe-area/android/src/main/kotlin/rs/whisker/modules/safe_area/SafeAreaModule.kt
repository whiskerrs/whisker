// `whisker-safe-area` Module (Android).
//
// View-less. Subscribes to the runtime's shared
// `WhiskerInsetsDispatcher` while at least one Rust listener is
// registered against the `insetsChanged` event; converts the system-
// bar + display-cutout insets to dp and dispatches.
//
// The Rust side (`packages/whisker-safe-area/src/lib.rs`) is the only
// consumer — it holds the `RwSignal<SafeAreaInsets>` returned by
// `safe_area_insets()` and updates it from this module's events.
//
// ## Inset semantics
//
// `WhiskerActivity` enforces edge-to-edge
// (`WindowCompat.setDecorFitsSystemWindows(window, false)` +
// `isNavigationBarContrastEnforced = false`), so WhiskerView always
// fills the entire window. The values forwarded here — read from
// `getRootWindowInsets(decorView).getInsets(systemBars() or
// displayCutout())` — therefore map 1:1 to padding on the WhiskerView
// (no double-padding to dodge, regardless of theme).
//
// ## Why `WhiskerInsetsDispatcher` (not a private decor listener)
//
// Android stores exactly one `OnApplyWindowInsetsListener` per view, so
// this module and `whisker-keyboard` used to clobber each other's
// listener on the shared decor view (last installer wins; the loser's
// inset signal freezes). We subscribe through the runtime's shared
// `WhiskerInsetsDispatcher` instead — it owns the single decor slot,
// handles config-change re-installation (rotation, multi-window
// resize), seeds late subscribers, and fans the raw insets out to every
// subscriber. See `WhiskerInsetsDispatcher` for the lifecycle.

package rs.whisker.modules.safe_area

import android.app.Activity
import androidx.core.view.WindowInsetsCompat
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerInsetsDispatcher
import rs.whisker.runtime.WhiskerValue

public class SafeAreaModule : Module() {

    /**
     * Live inset-dispatcher subscription. `null` between
     * `OnStopObserving` tearing it down and the next `OnStartObserving`
     * re-subscribing.
     */
    private var insetsRegistration: WhiskerInsetsDispatcher.Registration? = null

    public override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("SafeArea")
        Events("insetsChanged")

        OnStartObserving("insetsChanged") {
            if (insetsRegistration != null) return@OnStartObserving
            insetsRegistration = WhiskerInsetsDispatcher.addListener { insets ->
                val activity = appContext.currentActivity ?: return@addListener
                dispatch(activity, insets)
            }
        }

        OnStopObserving("insetsChanged") {
            insetsRegistration?.let { WhiskerInsetsDispatcher.removeListener(it) }
            insetsRegistration = null
        }
    }

    /**
     * Convert system-bar + display-cutout insets to dp and forward as
     * a `WhiskerValue.Map` payload.
     *
     * `decorFitsSystemWindows = true` (Whisker default) means the
     * activity has already cut out the insets before they reach
     * `decorView`, so `getInsets(systemBars() or displayCutout())`
     * returns zero — see the module-level doc for the graceful-
     * degrade reasoning.
     */
    private fun dispatch(activity: Activity, insets: WindowInsetsCompat) {
        val mask = WindowInsetsCompat.Type.systemBars() or
            WindowInsetsCompat.Type.displayCutout()
        val raw = insets.getInsets(mask)

        val density = activity.resources.displayMetrics.density.takeIf { it > 0f } ?: 1f

        val payload = mapOf(
            "top" to WhiskerValue.Float((raw.top / density).toDouble()),
            "leading" to WhiskerValue.Float((raw.left / density).toDouble()),
            "trailing" to WhiskerValue.Float((raw.right / density).toDouble()),
            "bottom" to WhiskerValue.Float((raw.bottom / density).toDouble()),
        )
        sendEvent("insetsChanged", WhiskerValue.Map(payload))
    }
}
