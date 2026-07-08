// `whisker-keyboard` Module (Android).
//
// View-less module with two jobs:
//
//  * `keyboardChanged` event — while a Rust listener is registered,
//    subscribe to `WhiskerInsetsDispatcher` and forward the IME inset
//    height (dp) as `{ height }`. The Rust side
//    (`packages/whisker-keyboard/src/lib.rs`) holds the `RwSignal<f64>`
//    `keyboard_height()` returns.
//
//  * `dismiss` function — a **real global unfocus**. On Android,
//    `hideSoftInputFromWindow` only hides the soft keyboard; the focused
//    `EditText` keeps focus (cursor, and — critically — a hardware
//    keyboard keeps delivering key events to it). So we `clearFocus()`
//    on the currently-focused view FIRST, then hide the IME. Clearing
//    focus fires `onFocusChange(false)`, which flows back to the
//    per-input `on_blur`, keeping Rust state consistent.
//
// ## Why an inset listener (not `windowSoftInputMode`)
//
// `WhiskerActivity` forces edge-to-edge
// (`WindowCompat.setDecorFitsSystemWindows(window, false)`), so the OS
// does NOT resize the window for the IME regardless of
// `android:windowSoftInputMode`. Edge-to-edge apps must read the IME
// inset themselves and apply it — which is exactly what this module
// surfaces to Rust so the app can pad/scroll its content.
//
// ## Why `WhiskerInsetsDispatcher` (not a private decor listener)
//
// Android stores exactly one `OnApplyWindowInsetsListener` per view, so
// this module and `whisker-safe-area` used to clobber each other's
// listener on the shared decor view (last installer wins; the loser's
// inset signal freezes). We subscribe through the runtime's shared
// `WhiskerInsetsDispatcher` instead — it owns the single decor slot,
// handles config-change re-installation, and fans the raw insets out to
// every subscriber. See `WhiskerInsetsDispatcher` for the lifecycle.

package rs.whisker.modules.keyboard

import android.app.Activity
import android.content.Context
import android.view.inputmethod.InputMethodManager
import androidx.core.view.WindowInsetsCompat
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerInsetsDispatcher
import rs.whisker.runtime.WhiskerValue

public class KeyboardModule : Module() {

    /**
     * Live inset-dispatcher subscription. `null` between
     * `OnStopObserving` tearing it down and the next `OnStartObserving`
     * re-subscribing.
     */
    private var insetsRegistration: WhiskerInsetsDispatcher.Registration? = null

    public override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("Keyboard")
        Events("keyboardChanged")

        OnStartObserving("keyboardChanged") {
            if (insetsRegistration != null) return@OnStartObserving
            insetsRegistration = WhiskerInsetsDispatcher.addListener { insets ->
                val activity = appContext.currentActivity ?: return@addListener
                dispatch(activity, insets)
            }
        }

        OnStopObserving("keyboardChanged") {
            insetsRegistration?.let { WhiskerInsetsDispatcher.removeListener(it) }
            insetsRegistration = null
        }

        // Real global unfocus. Marshalled to the UI thread because the
        // function body may run on the Lynx TASM thread and clearFocus /
        // IMM are View work.
        Function("dismiss") {
            val activity = appContext.currentActivity
            activity?.runOnUiThread { dismissOn(activity) }
            WhiskerValue.Null
        }
    }

    /**
     * Clear focus on the currently-focused view and hide the IME. Order
     * matters: clearing focus removes the hardware-keyboard target; the
     * IMM hide then dismisses the soft keyboard.
     */
    private fun dismissOn(activity: Activity) {
        val focused = activity.currentFocus
        val imm = activity.getSystemService(Context.INPUT_METHOD_SERVICE)
            as? InputMethodManager
        // Hide before clearing focus so we still have a valid window
        // token; clearing focus afterwards removes the input target.
        val token = focused?.windowToken ?: activity.window?.decorView?.windowToken
        if (token != null) {
            imm?.hideSoftInputFromWindow(token, 0)
        }
        focused?.clearFocus()
    }

    /**
     * Forward the IME inset (keyboard) height in dp as `{ height }`.
     * `Type.ime()` reports the full keyboard overlap from the bottom of
     * the window — already inclusive of the navigation bar it sits over
     * — so padding a bottom-anchored container by it clears the keyboard
     * exactly.
     */
    private fun dispatch(activity: Activity, insets: WindowInsetsCompat) {
        val imeBottom = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
        val density = activity.resources.displayMetrics.density.takeIf { it > 0f } ?: 1f
        val heightDp = (imeBottom / density).toDouble()
        sendEvent("keyboardChanged", WhiskerValue.Map(mapOf("height" to WhiskerValue.Float(heightDp))))
    }
}
