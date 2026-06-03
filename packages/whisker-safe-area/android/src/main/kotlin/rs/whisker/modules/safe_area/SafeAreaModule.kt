// `whisker-safe-area` Module (Android).
//
// View-less. Hooks `ViewCompat.setOnApplyWindowInsetsListener` onto
// the host Activity's decor view while at least one Rust listener is
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
// ## Lifecycle + configuration change handling
//
// Activities without `android:configChanges="orientation|screenSize"`
// get destroyed + recreated on rotation; the new WhiskerView calls
// `WhiskerAppContext.pushHost`, which re-fires every registered
// `HostAttachedListener`. We exploit that: the inset listener is
// (re)installed inside the HostAttachedListener body so each
// recreation transparently picks up the fresh decor view. The old
// listener pins nothing (Android stores it on the dead view) and
// gets GC'd with its host.
//
// * `OnStartObserving("insetsChanged")` — register a permanent
//   HostAttachedListener. It fires synchronously if a host is
//   already attached (covering steady-state), and every subsequent
//   `pushHost` (covering rotation, multi-window resize that
//   recreates the activity, etc.).
// * `OnStopObserving("insetsChanged")` — drop the host listener.
//   The most recently installed inset listener stays on its view
//   until the view dies, but no more Rust events fire because the
//   bridge has no Rust subscribers.

package rs.whisker.modules.safe_area

import android.app.Activity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import rs.whisker.runtime.HostAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

public class SafeAreaModule : Module() {

    /**
     * Live host-attached listener. `null` between `OnStopObserving`
     * tearing it down and the next `OnStartObserving` re-registering.
     * The listener body re-installs the inset listener on whichever
     * decor view is current at fire time, so a config-change
     * Activity recreation transparently rewires.
     */
    private var attachListener: HostAttachedListener? = null

    public override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("SafeArea")
        Events("insetsChanged")

        OnStartObserving("insetsChanged") {
            if (attachListener != null) return@OnStartObserving

            val listener = HostAttachedListener { installOnCurrentHost() }
            attachListener = listener
            // `addOnHostAttachedListener` fires synchronously once if
            // a host is already attached (the steady-state case) and
            // every time a new host attaches afterwards (the rotation
            // case). Either way we end up with the inset listener
            // installed on the live decor view.
            appContext.addOnHostAttachedListener(listener)
        }

        OnStopObserving("insetsChanged") {
            attachListener?.let {
                appContext.removeOnHostAttachedListener(it)
            }
            attachListener = null
            // Note: we don't bother clearing the inset listener off
            // the most recently used decor view — when the Rust side
            // has no subscribers, `sendEvent` becomes a no-op on the
            // bridge, so the listener firing harmlessly drops into
            // the void. The view will GC its listener slot when it
            // itself goes away.
        }
    }

    /**
     * Install (or re-install) the inset listener on the current host
     * Activity's decor view, and seed the Rust signal with the
     * currently-known insets so a fresh activity doesn't sit at the
     * last rotation's values until the next genuine inset change.
     */
    private fun installOnCurrentHost() {
        val activity = appContext.currentActivity ?: return
        val decor = activity.window?.decorView ?: return

        ViewCompat.setOnApplyWindowInsetsListener(decor) { _, insetsCompat ->
            dispatch(activity, insetsCompat)
            // Pass through unmodified so other consumers in the
            // hierarchy (e.g. Lynx's own listeners) still see the
            // same `WindowInsetsCompat`. Consuming here would mask
            // the insets from the rest of the view tree.
            insetsCompat
        }

        // Seed with current insets so a late subscriber (or a
        // post-rotation freshly-attached activity) doesn't sit at
        // `default()` until the next genuine WindowInsets dispatch.
        // `getRootWindowInsets` returns null until the view has been
        // measured at least once — that's fine, the listener will
        // pick the first real fire up.
        ViewCompat.getRootWindowInsets(decor)?.let { dispatch(activity, it) }
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
