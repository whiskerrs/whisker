package dev.tuft.examples.helloworld

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import dev.tuft.runtime.TuftView

class MainActivity : AppCompatActivity() {
    private var tuftView: TuftView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = TuftView(this)
        tuftView = view
        setContentView(view)
    }

    override fun onDestroy() {
        tuftView?.destroy()
        tuftView = null
        super.onDestroy()
    }
}
