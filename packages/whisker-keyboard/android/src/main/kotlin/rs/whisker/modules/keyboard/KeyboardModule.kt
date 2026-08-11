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
//    `EditText` keeps focus, meaning the cursor stays and a hardware
//    keyboard keeps delivering key events to it. So the focused view is
//    also cleared, which fires `onFocusChange(false)` and flows back to
//    the per-input `on_blur`, keeping Rust state consistent.
//
// ## Why an inset listener (not `windowSoftInputMode`)
//
// `WhiskerActivity` forces edge-to-edge
// (`WindowCompat.setDecorFitsSystemWindows(window, false)`), so the OS
// does NOT resize the window for the IME regardless of
// `android:windowSoftInputMode`. An edge-to-edge app has to read the IME
// inset itself, which is what this module surfaces to Rust.
//
// ## Why `WhiskerInsetsDispatcher` (not a private decor listener)
//
// Android stores exactly one `OnApplyWindowInsetsListener` per view, so
// this module and `whisker-safe-area` would clobber each other on the
// shared decor view and the loser's inset signal would freeze. The
// runtime's `WhiskerInsetsDispatcher` owns that single slot, handles
// config-change re-installation, and fans the raw insets out to every
// subscriber.

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

        // Marshalled to the UI thread: this body may run on the Lynx TASM
        // thread, and clearFocus / IMM are View work.
        Function("dismiss") {
            val activity = appContext.currentActivity
            activity?.runOnUiThread { dismissOn(activity) }
            WhiskerValue.Null
        }
    }

    /** Hide the IME and clear focus on the currently-focused view. */
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
     * Forward the IME inset height in dp as `{ height }`. `Type.ime()`
     * reports the full keyboard overlap from the bottom of the window,
     * already inclusive of the navigation bar it sits over, so padding a
     * bottom-anchored container by it clears the keyboard exactly.
     */
    private fun dispatch(activity: Activity, insets: WindowInsetsCompat) {
        val imeBottom = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
        val density = activity.resources.displayMetrics.density.takeIf { it > 0f } ?: 1f
        val heightDp = (imeBottom / density).toDouble()
        sendEvent("keyboardChanged", WhiskerValue.Map(mapOf("height" to WhiskerValue.Float(heightDp))))
    }
}
