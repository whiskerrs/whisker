package rs.whisker.runtime

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity

/**
 * Base Activity for Whisker apps.
 *
 * The CNG-generated `MainActivity` extends this and sets a `WhiskerView` as
 * its content view. Lifecycle events are forwarded to the Rust runtime via
 * JNI, but plugins are the typical consumers — user app code does not see
 * lifecycle events directly.
 */
open class WhiskerActivity : AppCompatActivity() {

    private var whiskerView: WhiskerView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = WhiskerView(this).also { whiskerView = it }
        setContentView(view)
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
