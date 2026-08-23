package rs.whisker.toggle

import android.content.Context
import android.view.View
import android.widget.Switch
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI

/** Minimal native control used as the first RFC0004 Android element. */
public class ToggleView(context: WhiskerContext) : WhiskerUI<View>(context) {
    private var control: Switch? = null
    private var applyingProperty: Boolean = false

    override fun createView(context: Context): View = Switch(context).also { toggle ->
        control = toggle
        toggle.setOnCheckedChangeListener { _, checked ->
            if (!applyingProperty) {
                WhiskerCustomEvent.dispatch(
                    ui = this,
                    name = "change",
                    params = mapOf("checked" to checked),
                )
            }
        }
    }

    public fun setChecked(value: Boolean) {
        applyingProperty = true
        control?.isChecked = value
        applyingProperty = false
    }

    public fun setDisabled(value: Boolean) {
        control?.isEnabled = !value
    }
}
