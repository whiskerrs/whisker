package dev.tuft.runtime

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity

/**
 * Base Activity for Tuft apps.
 *
 * The CNG-generated `MainActivity` extends this and sets a `TuftView` as
 * its content view. Lifecycle events are forwarded to the Rust runtime via
 * JNI, but plugins are the typical consumers — user app code does not see
 * lifecycle events directly.
 */
open class TuftActivity : AppCompatActivity() {

    private var tuftView: TuftView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = TuftView(this).also { tuftView = it }
        setContentView(view)
    }

    override fun onResume() {
        super.onResume()
        tuftView?.onEnterForeground()
    }

    override fun onPause() {
        super.onPause()
        tuftView?.onEnterBackground()
    }

    override fun onDestroy() {
        tuftView?.destroy()
        tuftView = null
        super.onDestroy()
    }
}
