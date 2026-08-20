package {{android_application_id}}

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.view.Gravity
import android.widget.TextView{{main_activity_imports}}

/**
 * Minimal native Host shell. The retained Rust renderer is attached in the
 * next mobile slice; this launch path deliberately contains no Lynx runtime.
 */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
{{main_activity_pre_super}}        super.onCreate(savedInstanceState)

        val content = TextView(this).apply {
            setBackgroundColor(Color.rgb(32, 36, 42))
            setTextColor(Color.rgb(245, 247, 250))
            textSize = 18f
            gravity = Gravity.CENTER
            text = "{{app_name}}"
        }
        setContentView(content)
{{main_activity_post_super}}    }
}
