// Phase L-3 — `whisker-local-store` ModuleDefinition (iOS).
//
// Replaces the pre-L-3 `@WhiskerModule("WhiskerLocalStore")`-
// annotated `WhiskerLocalStoreImpl`. A view-less DSL module:
// `definition()` has no `View(...)` block, just module-level
// `Function`s. The SwiftPM codegen plugin discovers the
// `WhiskerModule` subclass, emits a `@_cdecl` dispatch shim, and
// registers it via `whisker_bridge_register_module_dispatch(
// "WhiskerLocalStore", shim)` — so
// `whisker::platform_module::invoke("WhiskerLocalStore", ...)`
// from Rust routes into these handlers.
//
// The storage logic lives in `LocalStore.swift`.

import WhiskerModuleApi

public final class LocalStoreModule: WhiskerModule {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerLocalStore")

            // save(key, value) -> Bool
            Function("save") { (key: String, value: String) -> Bool in
                LocalStore.save(key, value)
            }
            // load(key) -> String? | nil  (Rust lifts nil into Option::None)
            Function("load") { (key: String) -> String? in
                LocalStore.load(key)
            }
            // remove(key) -> Void  (collapses to .null → ())
            Function("remove") { (key: String) in
                LocalStore.remove(key)
            }
        }
    }
}
