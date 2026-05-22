// `<x-hello>` Whisker native element on Android, registered via the
// `@WhiskerElement` Kotlin annotation (Phase 7-Φ.H.2 — companion of
// the iOS `@WhiskerElement` Swift Macro).
//
// The `whisker-android-ksp` KSP processor consumes this annotation
// at the user app's compile time and generates
// `rs.whisker.runtime.generated.WhiskerModuleBehaviors`, whose
// `registerAll()` call (driven from `WhiskerApplication.onCreate()`)
// registers `WhiskerHelloElement` against Lynx's behaviour registry
// before any `WhiskerView` is constructed.
//
// The Whisker module-system staging path
// (`whisker-build::android::stage_module_kotlin_sources`) copies
// this file into the consuming app's gen tree at
// `gen/android/app/src/main/whisker_modules/whisker-hello-element/
// WhiskerHelloElement.kt`, which gradle's main source-set
// (`kotlin.srcDirs(..., "src/main/whisker_modules")`) picks up.
// KSP then scans the resulting compilation graph for
// `@WhiskerElement` applications — finds this class, emits the
// registration.

package rs.whisker.elements.hello

import android.content.Context
import android.graphics.Color
import android.view.View
import com.lynx.tasm.behavior.LynxContext
import com.lynx.tasm.behavior.ui.LynxUI
import rs.whisker.annotations.WhiskerElement

/**
 * `<x-hello>` Whisker native element on Android. Renders as a yellow
 * rectangle; the consuming app sets size + position via the
 * `style:` prop on `XHello`.
 */
@WhiskerElement("x-hello")
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
