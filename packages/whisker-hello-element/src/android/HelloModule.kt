// Phase L-3 — `whisker-hello-element` migrated to the new
// `ModuleDefinition` DSL.
//
// Replaces the pre-L-3 `@WhiskerComponent("Hello")`-annotated
// `WhiskerHelloComponent` class. The Lynx tag stays
// `whisker-hello-element:Hello` (the cargo crate name is the
// namespace, prepended by the KSP processor / codegen plugin).
//
// Same shape on iOS — `HelloModule.swift` next to this file.

package rs.whisker.elements.hello

import android.content.Context
import android.graphics.Color
import android.view.View
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.Name
import rs.whisker.runtime.View as DSLView
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerUI

/**
 * Plain Lynx UI subclass. No annotation; instantiated by the
 * Lynx behavior the DSL registers below via reflection. The
 * single-arg `(WhiskerContext)` constructor matches the convention
 * the KSP L-2c registration code expects.
 */
open class HelloView(context: WhiskerContext) : WhiskerUI<View>(context) {
    override fun createView(context: Context): View {
        val view = View(context)
        view.setBackgroundColor(Color.YELLOW)
        return view
    }
}

/**
 * DSL-driven module. The KSP L-2c processor discovers this class
 * by walking the superclass chain looking for [WhiskerModule],
 * then emits a registration call into
 * `WhiskerHelloElementBehaviors.registerAll()` that:
 *
 *   - Registers a `Behavior("whisker-hello-element:Hello")` whose
 *     `createUI` instantiates `HelloView(context)`.
 *   - Calls `module.registerWithLynx()` so any Prop / Function
 *     declared below installs via L-1's
 *     `PropsUpdater.registerSetter(Class, Settable)` etc.
 */
class HelloModule : WhiskerModule() {
    override fun definition() = ModuleDefinition {
        Name("Hello")
        DSLView(HelloView::class.java) {
            // Hello is style-only (`Hello(style: "...")` on the
            // Rust side); no Prop / Function declarations needed.
            // Style props flow through Lynx's CSS path, not the
            // module API's Prop registration.
        }
    }
}
