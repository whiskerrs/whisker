// Phase L-3 — `whisker-local-store` ModuleDefinition (Android).
//
// Replaces the pre-L-3 `@WhiskerModule("WhiskerLocalStore")`-
// annotated `WhiskerLocalStoreImpl`. A view-less DSL module:
// `definition()` has no `View(...)` block, just module-level
// `Function`s. The KSP processor discovers the `WhiskerModule`
// subclass and calls `registerWithLynx()`, which registers the
// functions with `WhiskerModuleRegistry` under the `Name(...)` —
// so `whisker::platform_module::invoke("WhiskerLocalStore", ...)`
// from Rust routes into these handlers.
//
// The storage logic lives in `LocalStore.kt`.

package rs.whisker.modules.localstore

import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule

class LocalStoreModule : WhiskerModule() {
    override fun definition() = ModuleDefinition {
        Name("WhiskerLocalStore")

        // save(key, value) -> Bool
        Function("save") { key: String, value: String ->
            LocalStore.save(key, value)
        }
        // load(key) -> Str | Null  (Rust lifts Null into Option::None)
        Function("load") { key: String ->
            LocalStore.load(key)
        }
        // remove(key) -> Null  (Unit return collapses to Null → ())
        Function("remove") { key: String ->
            LocalStore.remove(key)
        }
    }
}
