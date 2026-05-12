package dev.lyra.runtime

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity

/**
 * Base Activity for Lyra apps.
 *
 * The CNG-generated `MainActivity` extends this and sets a `LyraView` as
 * its content view. Lifecycle events are forwarded to the Rust runtime via
 * JNI, but plugins are the typical consumers — user app code does not see
 * lifecycle events directly.
 */
open class LyraActivity : AppCompatActivity() {

    private var lyraView: LyraView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = LyraView(this).also { lyraView = it }
        setContentView(view)
    }

    override fun onResume() {
        super.onResume()
        lyraView?.onEnterForeground()
    }

    override fun onPause() {
        super.onPause()
        lyraView?.onEnterBackground()
    }

    override fun onDestroy() {
        lyraView?.destroy()
        lyraView = null
        super.onDestroy()
    }
}
