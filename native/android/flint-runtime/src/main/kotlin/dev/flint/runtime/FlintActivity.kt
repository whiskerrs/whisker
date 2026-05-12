package dev.flint.runtime

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity

/**
 * Base Activity for Flint apps.
 *
 * The CNG-generated `MainActivity` extends this and sets a `FlintView` as
 * its content view. Lifecycle events are forwarded to the Rust runtime via
 * JNI, but plugins are the typical consumers — user app code does not see
 * lifecycle events directly.
 */
open class FlintActivity : AppCompatActivity() {

    private var flintView: FlintView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = FlintView(this).also { flintView = it }
        setContentView(view)
    }

    override fun onResume() {
        super.onResume()
        flintView?.onEnterForeground()
    }

    override fun onPause() {
        super.onPause()
        flintView?.onEnterBackground()
    }

    override fun onDestroy() {
        flintView?.destroy()
        flintView = null
        super.onDestroy()
    }
}
