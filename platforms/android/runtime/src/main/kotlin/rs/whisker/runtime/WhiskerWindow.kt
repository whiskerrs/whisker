package rs.whisker.runtime

import android.app.Activity
import android.graphics.Color
import android.os.Build
import android.view.WindowManager
import androidx.core.view.WindowCompat

/** Window-level defaults shared by generated and manually-authored Android Hosts. */
public object WhiskerWindow {
    /**
     * Lets the Whisker surface own the complete window, including the areas
     * behind the status/navigation bars and display cutout.
     *
     * Safe-area handling remains an application/module concern: this method
     * changes the viewport and system-bar appearance but never injects padding.
     */
    @JvmStatic
    @JvmOverloads
    public fun enableEdgeToEdge(activity: Activity, darkSystemBarIcons: Boolean = false) {
        val window = activity.window
        WindowCompat.setDecorFitsSystemWindows(window, false)
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = darkSystemBarIcons
            isAppearanceLightNavigationBars = darkSystemBarIcons
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            window.attributes = window.attributes.apply {
                layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isStatusBarContrastEnforced = false
            window.isNavigationBarContrastEnforced = false
        }
    }
}
