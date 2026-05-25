// Phase L-2c smoke sample — exercises the new `ModuleDefinition`
// DSL alongside the existing `@WhiskerComponent` annotation-based
// `WhiskerHelloComponent`. Both paths register against the same
// Lynx behaviour registry; tag names are namespaced (`Hello` vs.
// `HelloDSL`) so they don't collide.
//
// The KSP processor's L-2c addition discovers this class by
// walking the superclass chain looking for
// `rs.whisker.runtime.WhiskerModule`, then emits a registration
// call inside `<Module>Behaviors.registerAll()` that:
//
//   - Builds an instance: `WhiskerHelloDSLModule()`
//   - Reads `definitionLazy.view` to find the target UI class
//     (`WhiskerHelloDSLView` below)
//   - Registers a `Behavior` against the Lynx tag
//     `whisker-hello-element:HelloDSL`
//   - Calls `module.registerWithLynx()` so the Prop / Function
//     dispatchers install via L-1's
//     `PropsUpdater.registerSetter(Class, Settable)` +
//     `LynxUIMethodsExecutor.registerMethodInvoker(Class, Invoker)`.

package rs.whisker.elements.hello

import android.content.Context
import android.graphics.Color
import android.view.View
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.Name
import rs.whisker.runtime.Prop
import rs.whisker.runtime.View
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerUI

/**
 * Plain Lynx UI subclass — same shape as
 * [WhiskerHelloComponent] in the annotation-based path, just
 * without the `@WhiskerComponent` annotation. Registration is
 * driven entirely by [WhiskerHelloDSLModule] below.
 */
open class WhiskerHelloDSLView(context: WhiskerContext) : WhiskerUI<View>(context) {

    private var labelView: HelloDSLLabel? = null

    override fun createView(context: Context): View {
        val v = HelloDSLLabel(context)
        labelView = v
        return v
    }

    fun setLabel(value: String) {
        labelView?.text = value
    }
}

internal class HelloDSLLabel(context: Context) : View(context) {
    var text: String = ""
        set(value) {
            field = value
            invalidate()
        }

    init {
        setBackgroundColor(Color.parseColor("#FFB347"))
    }
}

/**
 * DSL-driven module. Subclasses [WhiskerModule] and overrides
 * `definition()` to declare the Lynx-visible tag, view class,
 * prop setter, and function dispatcher.
 *
 * Lynx call site (Rust):
 * ```rust
 * render! { HelloDSL(label: "from DSL") }
 * ```
 */
class WhiskerHelloDSLModule : WhiskerModule() {
    override fun definition() = ModuleDefinition {
        Name("HelloDSL")

        View(WhiskerHelloDSLView::class.java) {
            Prop("label") { view: WhiskerHelloDSLView, value: String ->
                view.setLabel(value)
            }
        }
    }
}
