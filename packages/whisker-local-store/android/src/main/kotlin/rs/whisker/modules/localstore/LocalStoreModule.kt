// Phase L-3 — `whisker-local-store` ModuleDefinition (Android).
//
// A view-less DSL module: `definition()` has no `View(...)` block,
// just module-level `Function`s. The KSP processor finds every
// concrete `Module` subclass and calls `registerWithLynx()`,
// which registers the functions with `WhiskerModuleRegistry` under
// the `Name(...)` — so `whisker::platform_module::invoke(
// "WhiskerLocalStore", ...)` from Rust routes into these handlers.
//
// The storage logic lives in `LocalStore.kt`.

package rs.whisker.modules.localstore

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

class LocalStoreModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("WhiskerLocalStore")

        // save(key, value) -> Bool
        Function("save") { args ->
            val key = args.getOrNull(0)?.asString() ?: ""
            val value = args.getOrNull(1)?.asString() ?: ""
            WhiskerValue.Bool(LocalStore.save(key, value))
        }
        // load(key) -> Str | Null  (Rust lifts Null into Option::None)
        Function("load") { args ->
            LocalStore.load(args.getOrNull(0)?.asString() ?: "")
                ?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null
        }
        // remove(key) -> Null
        Function("remove") { args ->
            LocalStore.remove(args.getOrNull(0)?.asString() ?: "")
            WhiskerValue.Null
        }
    }
}
