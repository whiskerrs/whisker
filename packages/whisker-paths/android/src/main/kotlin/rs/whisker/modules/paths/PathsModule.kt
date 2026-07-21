// `whisker-paths` ModuleDefinition (Android).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// a module-level `Function`. The KSP processor finds the `Module`
// subclass and registers its functions with `WhiskerModuleRegistry`
// under the `Name(...)`, so `whisker::module!("WhiskerPaths").invoke(...)`
// from Rust routes into this handler.
//
// The resolution logic lives in `Paths.kt`.

package rs.whisker.modules.paths

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

class PathsModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("WhiskerPaths")

        // directories() -> Map { cache, document, support, temp }
        Function("directories") {
            WhiskerValue.Map(Paths.directories().mapValues { WhiskerValue.Str(it.value) })
        }
    }
}
