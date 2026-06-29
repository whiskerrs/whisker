// `whisker-secure-store` ModuleDefinition (iOS).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// module-level `Function`s. The SwiftPM codegen plugin discovers the
// `Module` subclass, emits a `@_cdecl` dispatch shim, and registers it
// so `whisker::platform_module::invoke("WhiskerSecureStore", ...)` from
// Rust routes into these handlers.
//
// Unlike `whisker-local-store`, the storage backend can fail, so each
// handler maps a `.failure(message)` from `SecureStore` to
// `WhiskerValue.error(_)` — which the Rust wrapper lifts into
// `Err(WhiskerModuleError)`.
//
// The storage logic lives in `SecureStore.swift`.

import WhiskerModule    // Module, ModuleDefinition, DSL

public final class SecureStoreModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerSecureStore")

            // save(key, value) -> Bool | Error
            Function("save") { (args: [WhiskerValue]) -> WhiskerValue in
                let key = args.first?.asString ?? ""
                let value = args.count > 1 ? (args[1].asString ?? "") : ""
                switch SecureStore.save(key, value) {
                case .success(let ok): return .bool(ok)
                case .failure(let err): return .error(err.message)
                }
            }
            // load(key) -> String | Null | Error  (Rust lifts Null into Option::None)
            Function("load") { (args: [WhiskerValue]) -> WhiskerValue in
                switch SecureStore.load(args.first?.asString ?? "") {
                case .success(let value): return value.map { WhiskerValue.string($0) } ?? .null
                case .failure(let err): return .error(err.message)
                }
            }
            // remove(key) -> Null | Error
            Function("remove") { (args: [WhiskerValue]) -> WhiskerValue in
                switch SecureStore.remove(args.first?.asString ?? "") {
                case .success: return .null
                case .failure(let err): return .error(err.message)
                }
            }
        }
    }
}
