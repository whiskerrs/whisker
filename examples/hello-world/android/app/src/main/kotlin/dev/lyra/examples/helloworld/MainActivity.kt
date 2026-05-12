package dev.lyra.examples.helloworld

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import dev.lyra.runtime.LyraView

class MainActivity : AppCompatActivity() {
    private var lyraView: LyraView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = LyraView(this)
        lyraView = view
        setContentView(view)
    }

    override fun onDestroy() {
        lyraView?.destroy()
        lyraView = null
        super.onDestroy()
    }
}
