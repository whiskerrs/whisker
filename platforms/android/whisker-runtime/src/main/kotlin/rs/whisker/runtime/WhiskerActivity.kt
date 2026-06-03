package rs.whisker.runtime

import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsControllerCompat

/**
 * Base Activity for Whisker apps.
 *
 * Extends `androidx.activity.ComponentActivity` (NOT
 * `AppCompatActivity`) on purpose: AppCompat injects a `subDecor`
 * LinearLayout + status-bar-guard view between the decor view and
 * the user's content. The guard takes up status-bar height even with
 * `setDecorFitsSystemWindows(false)`, which prevents WhiskerView from
 * actually filling the window and ruins the edge-to-edge story. Apps
 * built on Whisker don't use AppCompat's actual feature set (themed
 * dialogs, toolbar action bar, day-night, …) — they render exclusively
 * through Lynx — so the dependency was carrying nothing but the bug.
 *
 * The CNG-generated `MainActivity` extends this and sets a
 * `WhiskerView` as its content view. Lifecycle events forward to the
 * Rust runtime via JNI; plugins are the typical consumers — user app
 * code doesn't see lifecycle events directly.
 *
 * ## Edge-to-edge
 *
 * Whisker forces every host into edge-to-edge mode — `WhiskerView`
 * always fills the entire window, including the area behind the
 * status bar and navigation bar. Apps respect system UI by reading
 * `safe_area_insets()` from `whisker-safe-area` and laying padding
 * accordingly, **not** by relying on the activity to inset the
 * content frame.
 *
 * Two motivations:
 *
 * 1. **API 35+ mandates it.** Apps targeting Android 15+ get
 *    edge-to-edge unconditionally — `setDecorFitsSystemWindows(true)`
 *    is silently ignored. Pre-empting that change here means Whisker
 *    behaves the same across every supported Android version.
 * 2. **Consistent inset semantics.** With the activity-inset path,
 *    the insets `safe_area_insets()` reports (the window-level
 *    system-bar dimensions) are *already* accounted for in the
 *    WhiskerView's frame, so naively applying `padding-top: insets.top`
 *    double-pads. Edge-to-edge collapses that ambiguity — the
 *    reported values are exactly the padding the user component
 *    needs.
 *
 * The corollary: `setStatusBarContrastEnforced` /
 * `setNavigationBarContrastEnforced` (API 29+) are both forced to
 * `false` so the system doesn't paint its own translucent scrim
 * behind either bar — the app's background draws through cleanly.
 */
open class WhiskerActivity : ComponentActivity() {

    private var whiskerView: WhiskerView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Edge-to-edge. AndroidX wrapper handles the API-level branching
        // (the underlying platform call is `Window.setDecorFitsSystemWindows`,
        // API 30+; below that the wrapper sets the legacy `SYSTEM_UI_FLAG_*`
        // bits that achieve the same effect).
        WindowCompat.setDecorFitsSystemWindows(window, false)

        // Drop the theme-provided window background. With edge-to-edge
        // + transparent system bars, anything WhiskerView doesn't paint
        // over shows through; the default `Theme.*.NoActionBar` window
        // background bleeds visibly through the status-bar area on
        // dark-themed apps. Black is a safer default — apps that
        // render light backgrounds tend to do so via a full-bleed root
        // view that covers the window anyway, so an unseen black
        // background hurts nobody.
        window.setBackgroundDrawable(ColorDrawable(Color.BLACK))

        // Drop the theme-provided system-bar background colours so the
        // app's own background paints all the way to the screen edges.
        // Modern Android (API 35+) ignores both setters — they're a
        // no-op there — but on 21..34 they're how we actually clear the
        // status / nav bar tint that themes default to.
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT

        // Stop the system from forcing its own translucent scrims behind
        // status / navigation bars. The contrast-enforcement heuristic
        // decides when to apply each based on the app's drawn pixel
        // contrast against the system-bar foreground — we always want
        // the app's own background to show through unmodified. Both
        // setters are available from API 29; on API 35+ they're the
        // *only* way to suppress the scrim, since the platform ignores
        // `statusBarColor` / `navigationBarColor` from that release on.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isStatusBarContrastEnforced = false
            window.isNavigationBarContrastEnforced = false
        }

        val view = WhiskerView(this).also { whiskerView = it }
        setContentView(view)

        // `windowDrawsSystemBarBackgrounds=true` (AppCompat default,
        // inherited by `Theme.AppCompat.NoActionBar` even when we
        // base on ComponentActivity) inflates a screen layout that
        // includes a `statusBarBackground` / `navigationBarBackground`
        // pair as sibling Views of the content frame inside a
        // LinearLayout. Their non-zero `layout_height` /
        // `layout_width` pushes the content down by the status-bar
        // height (and right by the nav-bar width in landscape), so
        // WhiskerView still doesn't actually fill the window even
        // with edge-to-edge enabled. Walk the tree and collapse each
        // to zero so the content frame snaps to (0,0).
        view.post { collapseSystemBarGuards(window.decorView) }

        // Force light foreground (icons / time) on the system bars.
        // Default is light-when-the-window-background-is-dark, which
        // the system computes from the *window* background colour
        // (which we already pinned to black above). Setting both flags
        // to `false` (= light foreground) is explicit, matches the
        // dark baseline Whisker apps actually render, and survives a
        // hosting MainActivity that flips the window background later.
        // Hosts shipping a light theme can flip these in their own
        // `onCreate` after calling `super.onCreate`.
        WindowInsetsControllerCompat(window, view).apply {
            isAppearanceLightStatusBars = false
            isAppearanceLightNavigationBars = false
        }
    }

    /**
     * Walk `root` and collapse any `statusBarBackground` /
     * `navigationBarBackground` View added by the platform's screen
     * layout (PhoneWindow.generateLayout) when
     * `windowDrawsSystemBarBackgrounds=true`. The guards sit as
     * siblings of the content frame inside a LinearLayout, taking
     * up status-bar / nav-bar dimensions — the LinearLayout's
     * sequential positioning is what pushes the content (and our
     * WhiskerView) down. Zeroing them out lets the content snap
     * to (0, 0).
     */
    private fun collapseSystemBarGuards(root: android.view.View) {
        if (root.id != android.view.View.NO_ID) {
            val entryName = try {
                root.resources.getResourceEntryName(root.id)
            } catch (_: android.content.res.Resources.NotFoundException) {
                null
            }
            if (entryName == "statusBarBackground" ||
                entryName == "navigationBarBackground"
            ) {
                val lp = root.layoutParams
                lp.height = 0
                lp.width = 0
                root.layoutParams = lp
            }
        }
        if (root is android.view.ViewGroup) {
            for (i in 0 until root.childCount) {
                collapseSystemBarGuards(root.getChildAt(i))
            }
        }
    }

    override fun onResume() {
        super.onResume()
        whiskerView?.onEnterForeground()
    }

    override fun onPause() {
        super.onPause()
        whiskerView?.onEnterBackground()
    }

    override fun onDestroy() {
        whiskerView?.destroy()
        whiskerView = null
        super.onDestroy()
    }
}
