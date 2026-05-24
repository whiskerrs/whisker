// Whisker platform component on Android with local tag `Hello`. The
// Lynx registration string is `whisker-hello-element:Hello` — the
// KSP processor prepends the cargo crate name (read from
// `ksp { arg("whisker.crateName", …) }` in this module's
// `build.gradle.kts`) as the namespace, so two unrelated packages
// can both declare a `Hello` element without colliding in Lynx's
// behaviour registry. Phase 7-Φ.H.2.
//
// Demonstrates the Whisker-only API surface (no `com.lynx.tasm.*`
// imports in author code).
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
import rs.whisker.annotations.WhiskerComponent
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

@WhiskerComponent("Hello")
open class WhiskerHelloComponent(context: WhiskerContext) : WhiskerUI<View>(context) {

    override fun createView(context: Context): View {
        val view = View(context)
        view.setBackgroundColor(Color.YELLOW)
        return view
    }
}
