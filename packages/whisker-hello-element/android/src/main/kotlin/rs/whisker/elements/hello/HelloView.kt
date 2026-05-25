// Plain Lynx UI subclass for the `Hello` element. No annotation;
// instantiated by the Lynx behavior `HelloModule`'s `definition()`
// registers (see `HelloModule.kt`). The single-arg `(WhiskerContext)`
// constructor matches the convention the KSP L-2c registration code
// expects.

package rs.whisker.elements.hello

import android.content.Context
import android.graphics.Color
import android.view.View
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

open class HelloView(context: WhiskerContext) : WhiskerUI<View>(context) {
    override fun createView(context: Context): View {
        val view = View(context)
        view.setBackgroundColor(Color.YELLOW)
        return view
    }
}
