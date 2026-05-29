// `whisker-module-codegen` — Swift executable that scans .swift
// source files for concrete subclasses of `Module` (the
// ModuleDefinition DSL base class from `WhiskerModule`) and emits
// `<TargetName>+Generated.swift` containing the matching DSL-module
// registration calls.
//
// Invoked at SwiftPM build time by `WhiskerModuleCodegenPlugin`
// (under `../../Plugins/`). The plugin passes:
//   --target-name <name>        ← the SwiftPM target being built
//   --output <path>             ← target for the generated .swift
//   <input1.swift> <input2.swift> …
//
// Apple's SwiftSyntax parses each input file into an AST; we walk
// it to find any class declaration whose inheritance clause names
// `Module` (or `WhiskerModule.Module`) and produce one registration
// block per match.
//
// Discovery is **inheritance-based** — Phase M (Issue #59) dropped
// the `@WhiskerModule` marker macro. A Whisker module is now
// defined by exactly one signal: `extends WhiskerModule.Module`.
// Same shape as Android's KSP processor, just over SwiftSyntax
// instead of KSP's `Resolver`.
//
// Phase 7-Φ.G: emitted per-module. Each module's SwiftPM target
// owns its registration code (`_whiskerRegisterModules_<TargetName>`),
// and the whisker-build-generated aggregator imports + calls every
// per-module register fn from its own `WhiskerModuleBehaviors.
// registerAll()`. The previous `WhiskerModuleBehaviors` class emitted
// here is gone — the codegen now produces a top-level fn only.

import Foundation
import SwiftSyntax
import SwiftParser

// ---- CLI argument parsing ----------------------------------------------------

struct Args {
    let targetName: String
    /// Cargo crate name (e.g. "whisker-hello-element"), passed from
    /// the SwiftPM plugin via `context.package.displayName`. Used as
    /// the element tag namespace so two modules' identical local
    /// tag names don't collide in Lynx's behaviour registry.
    /// Phase 7-Φ.H.2.
    let crateName: String
    let outputPath: String
    let inputs: [String]
}

func parseArgs(_ argv: [String]) -> Args? {
    var target: String?
    var crate: String?
    var output: String?
    var inputs: [String] = []
    var i = 1 // skip argv[0]
    while i < argv.count {
        switch argv[i] {
        case "--target-name":
            guard i + 1 < argv.count else { return nil }
            target = argv[i + 1]
            i += 2
        case "--crate-name":
            guard i + 1 < argv.count else { return nil }
            crate = argv[i + 1]
            i += 2
        case "--output":
            guard i + 1 < argv.count else { return nil }
            output = argv[i + 1]
            i += 2
        default:
            inputs.append(argv[i])
            i += 1
        }
    }
    guard let target, let crate, let output else { return nil }
    return Args(targetName: target, crateName: crate, outputPath: output, inputs: inputs)
}

// ---- AST walker --------------------------------------------------------------

/// One discovered concrete subclass of `WhiskerModule.Module`.
/// The codegen emits a registration block that instantiates the
/// class, reads its `definitionLazy`, and — for view-bearing
/// modules — registers a Lynx behavior using the view class from
/// the `View(...)` block, then calls `module.registerWithLynx()`
/// so the DSL's Prop / Function dispatchers install via the
/// Obj-C-runtime path (L-2b). View-less modules register their
/// `Function`s through `whisker_bridge_register_module_dispatch`.
struct DSLModuleHit {
    let className: String
}

final class WhiskerAnnotationCollector: SyntaxVisitor {
    var dslModules: [DSLModuleHit] = []

    override func visit(_ node: ClassDeclSyntax) -> SyntaxVisitorContinueKind {
        // ---- DSL module path: subclass of `Module` ----
        //
        // Discovery is purely syntactic — SwiftSyntax has no semantic
        // resolver, so we match the inheritance clause's first
        // identifier against the unqualified base name. `Module` is
        // the convention; `WhiskerModule.Module` is also accepted in
        // case a user fully-qualifies to disambiguate. Subclassing
        // the base IS the registration trigger; no annotation is
        // applied at the declaration site. Companion of Android's
        // KSP inheritance walk.
        //
        // Protocols come after the base class in `inheritanceClause`,
        // so we only need to inspect the first inherited type.
        if let inheritance = node.inheritanceClause,
           let first = inheritance.inheritedTypes.first?.type
        {
            let inheritedName: String?
            if let id = first.as(IdentifierTypeSyntax.self) {
                inheritedName = id.name.text
            } else if let member = first.as(MemberTypeSyntax.self) {
                inheritedName = member.name.text
            } else {
                inheritedName = nil
            }
            if inheritedName == "Module" {
                dslModules.append(DSLModuleHit(className: node.name.text))
            }
        }
        return .skipChildren
    }
}

// ---- Codegen -----------------------------------------------------------------

func render(
    targetName: String,
    crateName: String,
    dslModules: [DSLModuleHit]
) -> String {
    // Deterministic order — sort so two builds with the same input
    // produce byte-identical output (helps SwiftPM's incremental-
    // rebuild fingerprinting).
    let sortedDSLModules = dslModules.sorted { $0.className < $1.className }

    let registerFn = "_whiskerRegisterModules_\(targetName)"

    var out = """
        // AUTO-GENERATED by `whisker-module-codegen` (SwiftPM build plugin).
        // DO NOT EDIT — re-runs automatically on next `swift build`.
        //
        // Sourced from `Module` subclasses in the `\(targetName)`
        // SwiftPM target's source set. Each view-bearing module
        // registers against Lynx with the fully-qualified tag
        // `\(crateName):<Name>`; the cargo crate name (this package's
        // SwiftPM `displayName`) is the namespace, so two unrelated
        // module packages can both declare a `Hello` element without
        // colliding. Each module package owns its own copy of this
        // generated file; the whisker-build-generated aggregator
        // imports every module and calls each per-target register fn
        // from a top-level `WhiskerModuleBehaviors.registerAll()`.
        //
        // Sibling of Android's `rs.whisker.ksp.WhiskerModuleProcessor`.
        //
        // DSL Module registrations: \(sortedDSLModules.count)

        import Foundation
        import Lynx
        // WhiskerRuntime re-exports WhiskerDriver, which carries the
        // C ABI declarations (`whisker_bridge_register_module_dispatch`,
        // `WhiskerValueRaw`, …) the module registration touches.
        import WhiskerRuntime
        // Phase L-3 — the DSL discovery path emits
        // `MyModule().registerWithLynx()` calls; `registerWithLynx`
        // lives in `WhiskerModule` (`WhiskerModuleRegistrar.swift`)
        // as an extension on `WhiskerModule`.
        import WhiskerModule

        /// Per-target registration entry point. The aggregator
        /// (`gen/ios/whisker_modules/Sources/WhiskerModules/RegisterAll.swift`,
        /// emitted by whisker-build) imports the target's SwiftPM
        /// module and calls this fn from
        /// `WhiskerModuleBehaviors.registerAll()`.
        ///
        /// Top-level fn rather than a class method so it doesn't
        /// shadow the aggregator's `WhiskerModuleBehaviors` symbol
        /// when both modules end up in the same compiled product.
        public func \(registerFn)() {

        """

    if sortedDSLModules.isEmpty {
        out += "    // (no Module subclass found)\n"
    }
    // ---- Phase L-3 — DSL modules ---------------------------------------
    //
    // For each `Module` subclass found in this target's sources,
    // the registration block reads its `definitionLazy`
    // (via a top-level instance referenced directly — same SwiftPM
    // target, so no `NSClassFromString` / `@objc` pinning needed),
    // then branches at runtime:
    //
    //   - **View-bearing** (`def.view != nil`): register a Lynx
    //     behavior bound to `def.view!.viewClass`, then
    //     `module.registerWithLynx()` to install Prop / Function
    //     dispatch (Obj-C-runtime path, L-2b).
    //   - **View-less** (module-level `Function`s): register the
    //     `@_cdecl` dispatch shim (emitted as a top-level decl
    //     below) with the C bridge via
    //     `whisker_bridge_register_module_dispatch(name, shim)`.
    //
    // The `@_cdecl` shim + the top-level module instance it
    // dispatches against are emitted *after* the register fn (a
    // forward reference within the same file is legal).
    if !sortedDSLModules.isEmpty {
        out += "        // ---- DSL modules (Phase L-3) ----\n"
    }
    let tagPrefix = "\(crateName):"
    for hit in sortedDSLModules {
        let instance = "_whiskerDSLInstance_\(hit.className)"
        let shim = "_whiskerDSLDispatch_\(hit.className)"
        out += """
                do {
                    let module = \(instance)
                    let def = module.definitionLazy
                    if let name = def.name {
                        if let view = def.view {
                            LynxComponentRegistry.registerUI(view.viewClass, withName: "\(tagPrefix)" + name)
                            module.registerWithLynx()
                        } else {
                            // Namespace the dispatch key with the crate
                            // (`<crate>:Name`) so two crates can ship
                            // same-named function-only modules — matches
                            // the Rust `module!("Name")` prefix.
                            whisker_bridge_register_module_dispatch("\(tagPrefix)" + name, \(shim))
                        }
                    }
                }

            """
    }

    // Close the register fn.
    out += """
        }

        """

    // ---- Top-level @_cdecl shims for DSL modules -----------------------
    //
    // One per DSL module. Always emitted (codegen can't know at
    // build time whether a module is view-less); only registered at
    // runtime when `def.view == nil`. The shim forwards the C-ABI
    // call straight into `WhiskerModule.dispatchModuleFunctionRaw`.
    for hit in sortedDSLModules {
        let instance = "_whiskerDSLInstance_\(hit.className)"
        let shim = "_whiskerDSLDispatch_\(hit.className)"
        out += """
            // Top-level instance + C-ABI dispatch shim for the DSL
            // module `\(hit.className)`. The `let` is lazily
            // initialised on first reference (Swift global semantics).
            private let \(instance) = \(hit.className)()

            @_cdecl("\(shim)")
            public func \(shim)(
                _ methodName: UnsafePointer<CChar>?,
                _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
                _ argCount: Int
            ) -> WhiskerValueRaw {
                return \(instance).dispatchModuleFunctionRaw(methodName, argsPtr, argCount)
            }

        """
    }
    return out
}

// ---- Main --------------------------------------------------------------------

guard let args = parseArgs(CommandLine.arguments) else {
    FileHandle.standardError.write(Data(
        "usage: whisker-module-codegen --target-name <name> --crate-name <pkg> --output <path> <input.swift>...\n".utf8
    ))
    exit(2)
}

let collector = WhiskerAnnotationCollector(viewMode: .sourceAccurate)
for input in args.inputs {
    let source: String
    do {
        source = try String(contentsOfFile: input, encoding: .utf8)
    } catch {
        FileHandle.standardError.write(Data(
            "whisker-module-codegen: cannot read \(input): \(error)\n".utf8
        ))
        exit(1)
    }
    let tree = Parser.parse(source: source)
    collector.walk(tree)
}

let generated = render(
    targetName: args.targetName,
    crateName: args.crateName,
    dslModules: collector.dslModules
)
do {
    try generated.write(toFile: args.outputPath, atomically: true, encoding: .utf8)
} catch {
    FileHandle.standardError.write(Data(
        "whisker-module-codegen: cannot write \(args.outputPath): \(error)\n".utf8
    ))
    exit(1)
}
