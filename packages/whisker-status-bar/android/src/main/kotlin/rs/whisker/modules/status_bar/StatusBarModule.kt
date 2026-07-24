// `whisker-status-bar` ModuleDefinition (Android).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// module-level `Function`s. The KSP processor finds the `Module`
// subclass and registers its functions with `WhiskerModuleRegistry`
// under the `Name(...)`, so `whisker::platform_module::invoke(
// "WhiskerStatusBar", ...)` from Rust routes into these handlers.
//
// Status-bar visibility + icon style are driven through
// `WindowInsetsControllerCompat` (androidx.core), which backports the
// modern `WindowInsetsController` API down to API 21. All mutation runs
// on the UI thread (`activity.runOnUiThread`) since window changes are
// main-thread-only and a Rust caller may invoke from any thread.

package rs.whisker.modules.status_bar

import android.app.Activity
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

public class StatusBarModule : Module() {
    public override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("WhiskerStatusBar")

        // setHidden(hidden: Bool) -> Null | Err
        Function("setHidden") { args ->
            try {
                val hidden = args.getOrNull(0)?.asBool() ?: false
                onActivity { activity ->
                    StatusBar.setHidden(activity, hidden)
                }
                WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerStatusBar.setHidden failed: ${t.message}")
            }
        }

        // setStyle(style: "light" | "dark") -> Null | Err
        Function("setStyle") { args ->
            try {
                val style = args.getOrNull(0)?.asString() ?: "dark"
                onActivity { activity ->
                    StatusBar.setStyle(activity, style)
                }
                WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerStatusBar.setStyle failed: ${t.message}")
            }
        }
    }

    /// Resolve the host activity and run `block` on its UI thread. No-op
    /// if no activity is attached (e.g. dispatch during teardown).
    private fun onActivity(block: (Activity) -> Unit) {
        val activity = appContext.currentActivity ?: return
        activity.runOnUiThread { block(activity) }
    }
}

/// Plain helper — no Whisker types. Kept separate from `StatusBarModule`
/// so the window logic is testable/readable on its own, matching the
/// `whisker-haptics` `HapticsModule`/`Haptics` split.
private object StatusBar {
    private fun controller(activity: Activity): WindowInsetsControllerCompat =
        WindowInsetsControllerCompat(activity.window, activity.window.decorView)

    fun setHidden(activity: Activity, hidden: Boolean) {
        val controller = controller(activity)
        if (hidden) {
            controller.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            controller.hide(WindowInsetsCompat.Type.statusBars())
        } else {
            controller.show(WindowInsetsCompat.Type.statusBars())
        }
    }

    fun setStyle(activity: Activity, style: String) {
        // `isAppearanceLightStatusBars = true` → dark icons (for a light
        // background); expo's `style="dark"`. `false` → light/white icons
        // (for a dark background); expo's `style="light"`.
        controller(activity).isAppearanceLightStatusBars = (style == "dark")
    }
}
