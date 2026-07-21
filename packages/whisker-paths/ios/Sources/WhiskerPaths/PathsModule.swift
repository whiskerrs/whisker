// `whisker-paths` ModuleDefinition (iOS).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// a module-level `Function`. The SwiftPM codegen plugin discovers the
// `Module` subclass, emits a `@_cdecl` dispatch shim, and registers it
// so `whisker::module!("WhiskerPaths").invoke(...)` from Rust routes
// into this handler.
//
// The resolution logic lives in `Paths.swift`.

import WhiskerModule    // Module, ModuleDefinition, DSL

public final class PathsModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerPaths")

            // directories() -> Map { cache, document, support, temp }
            Function("directories") { (_: [WhiskerValue]) -> WhiskerValue in
                .map(Paths.directories().mapValues { WhiskerValue.string($0) })
            }
        }
    }
}
