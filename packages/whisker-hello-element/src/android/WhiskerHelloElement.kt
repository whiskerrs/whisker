// `<x-hello>` Whisker native element on Android — Phase 7-Φ.H.1
// migration target. Demonstrates the Whisker-only API surface
// (no `com.lynx.tasm.*` imports in author code).
//
// `WhiskerUI` / `WhiskerContext` are typealiases provided by
// `rs.whisker.runtime` that resolve to the underlying Lynx types
// at Kotlin's type-system level. Stack traces / debugger views
// still show the real `LynxUI` class names — typealiases are a
// source-level concept only.

package rs.whisker.elements.hello

import android.content.Context
import android.graphics.Color
import android.view.View
import rs.whisker.annotations.WhiskerElement
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

@WhiskerElement("x-hello")
open class WhiskerHelloElement(context: WhiskerContext) : WhiskerUI<View>(context) {

    override fun createView(context: Context): View {
        val view = View(context)
        view.setBackgroundColor(Color.YELLOW)
        return view
    }
}
