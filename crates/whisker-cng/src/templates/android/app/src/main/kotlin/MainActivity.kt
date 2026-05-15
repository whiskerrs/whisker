package {{android_application_id}}

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import rs.whisker.runtime.WhiskerView

class MainActivity : AppCompatActivity() {
    private var whiskerView: WhiskerView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val view = WhiskerView(this)
        whiskerView = view
        setContentView(view)
    }

    override fun onDestroy() {
        whiskerView?.destroy()
        whiskerView = null
        super.onDestroy()
    }
}
