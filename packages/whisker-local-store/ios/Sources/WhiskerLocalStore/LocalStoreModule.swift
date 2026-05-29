// Phase L-3 — `whisker-local-store` ModuleDefinition (iOS).
//
// A view-less DSL module: `definition()` has no `View(...)` block,
// just module-level `Function`s. The SwiftPM codegen plugin
// discovers the `Module` subclass, emits a `@_cdecl` dispatch
// shim, and registers it via
// `whisker_bridge_register_module_dispatch("WhiskerLocalStore",
// shim)` — so `whisker::platform_module::invoke(
// "WhiskerLocalStore", ...)` from Rust routes into these handlers.
//
// The storage logic lives in `LocalStore.swift`.

import WhiskerModule    // Module, ModuleDefinition, DSL

public final class LocalStoreModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerLocalStore")

            // save(key, value) -> Bool
            Function("save") { (args: [WhiskerValue]) -> WhiskerValue in
                let key = args.first?.asString ?? ""
                let value = args.count > 1 ? (args[1].asString ?? "") : ""
                return .bool(LocalStore.save(key, value))
            }
            // load(key) -> String | Null (Rust lifts Null into Option::None)
            Function("load") { (args: [WhiskerValue]) -> WhiskerValue in
                LocalStore.load(args.first?.asString ?? "").map { WhiskerValue.string($0) } ?? .null
            }
            // remove(key) -> Null
            Function("remove") { (args: [WhiskerValue]) -> WhiskerValue in
                LocalStore.remove(args.first?.asString ?? "")
                return .null
            }
        }
    }
}
