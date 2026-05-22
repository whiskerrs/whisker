// Phase 7-Φ.C smoke test — Android counterpart of
// `whisker_hello_element.mm`. Registers an `x-hello` Lynx UI tag
// that renders as a yellow `View`, mirroring the iOS path's
// system-pink `UIView`. Together with the iOS source these are the
// canonical reference implementation third-party Whisker module
// crates follow.
//
// ## How registration works
//
// Lynx's behaviour registry is populated at app init via classes
// annotated with `@LynxBehavior(tagName = "...")`. Lynx ships an
// annotation processor (under `platform/android/lynx_processor/`)
// that scans the app's classpath, finds every such annotation, and
// generates a `BehaviorClassWarmer` entry that's loaded when a
// `LynxView` is created. So all we need to do on the Android side
// is annotate a `LynxUI` subclass and gradle does the rest — no
// equivalent of iOS's `+load` constructor magic.
//
// The Whisker module-system v1 staging path
// (`whisker-build::android::stage_module_kotlin_sources`) copies
// this file into the consuming app's gen tree at
// `gen/android/app/src/main/whisker_modules/whisker-hello-element/
// WhiskerHelloElement.kt`. Gradle's main source-set
// (`kotlin.srcDirs(..., "src/main/whisker_modules")`) picks it up
// and the existing Lynx annotation processor handles registration.

package rs.whisker.elements.hello

import android.content.Context
import android.graphics.Color
import android.view.View
import com.lynx.tasm.behavior.LynxBehavior
import com.lynx.tasm.behavior.LynxContext
import com.lynx.tasm.behavior.ui.LynxUI

/**
 * `<x-hello>` Whisker native element on Android. Renders as a yellow
 * rectangle; the consuming app sets size + position via the
 * `style:` prop on `XHello`.
 *
 * The `style` prop is the only one declared on the Whisker side
 * (see `packages/whisker-hello-element/src/lib.rs`); it routes
 * through Lynx's standard inline-style pipeline, so we don't need
 * to override `setProperty` or implement a `@LynxProp` here.
 */
@LynxBehavior(tagName = ["x-hello"])
open class WhiskerHelloElement(context: LynxContext) : LynxUI<View>(context) {

    override fun createView(context: Context): View {
        val view = View(context)
        // System yellow to make the smoke test visually obvious —
        // if you see a yellow rectangle, the tag-by-name dispatch
        // is working end-to-end (render! → Rust → C ABI → Lynx →
        // this class).
        view.setBackgroundColor(Color.YELLOW)
        return view
    }
}
