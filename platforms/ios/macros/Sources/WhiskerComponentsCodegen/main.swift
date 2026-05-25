// `whisker-components-codegen` — Swift executable that scans .swift
// source files for `@WhiskerComponent("x-tag")` AND `@WhiskerModule`
// applications and emits `<TargetName>+Generated.swift`
// containing the matching `LynxComponentRegistry.registerUI(...)`
// and DSL-module registration calls.
//
// Invoked at SwiftPM build time by `WhiskerComponentsCodegenPlugin`
// (under `../../Plugins/`). The plugin passes:
//   --target-name <name>        ← the SwiftPM target being built
//   --output <path>             ← target for the generated .swift
//   <input1.swift> <input2.swift> …
//
// Apple's SwiftSyntax parses each input file into an AST; we walk
// it to find any class declaration carrying either annotation,
// pull the string literal argument, and produce one registration
// line per match.
//
// Companion of `WhiskerComponentProcessor.kt` (Android KSP). Same
// shape — annotations in, generated registry out. Apple's
// SwiftSyntax is the iOS equivalent of KSP's `Resolver`.
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

/// One discovered `(tag, className)` pair from a `@WhiskerComponent`
/// annotation. `className` is the bare Swift class name as
/// written; the generated registry's `NSClassFromString` lookup
/// applies both the `<TargetName>.` SwiftPM-target prefix AND
/// the bare name (to support authors who add their own
/// `@objc(BareName)`).
struct ElementHit {
    let tag: String
    let className: String
}

/// One discovered `@WhiskerModule`-annotated class. Phase L-3 +
/// the discovery overhaul: `@WhiskerModule` is the explicit
/// opt-in (companion of Android's KSP `@WhiskerModule`). The
/// codegen emits a registration block that instantiates the
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
    var elements: [ElementHit] = []
    var dslModules: [DSLModuleHit] = []

    override func visit(_ node: ClassDeclSyntax) -> SyntaxVisitorContinueKind {
        for attribute in node.attributes {
            guard let attr = attribute.as(AttributeSyntax.self) else { continue }
            guard let attrName = attr.attributeName.as(IdentifierTypeSyntax.self) else { continue }
            let name = attrName.name.text
            let className = node.name.text

            // ---- Element path: @WhiskerComponent("tag") ----
            if name == "WhiskerComponent" {
                guard let value = firstStringArgument(of: attr) else { continue }
                elements.append(ElementHit(tag: value, className: className))
            }

            // ---- DSL module path: @WhiskerModule (no argument) ----
            //
            // The attribute IS the opt-in — matches the Android KSP
            // convention. The module's local name comes from the
            // `Name("…")` entry inside `definition()`, so the
            // attribute itself takes no arguments.
            if name == "WhiskerModule" {
                dslModules.append(DSLModuleHit(className: className))
            }
        }
        return .skipChildren
    }
}

/// Extract the first positional string-literal argument from an
/// `@Attr("...")` invocation. Returns nil for any deviation
/// (multi-segment interpolation, missing args, non-string expr).
private func firstStringArgument(of attr: AttributeSyntax) -> String? {
    guard let args = attr.arguments?.as(LabeledExprListSyntax.self) else { return nil }
    guard let first = args.first else { return nil }
    guard let strLit = first.expression.as(StringLiteralExprSyntax.self) else { return nil }
    guard strLit.segments.count == 1 else { return nil }
    guard let seg = strLit.segments.first?.as(StringSegmentSyntax.self) else { return nil }
    return seg.content.text
}

// ---- Codegen -----------------------------------------------------------------

func render(
    targetName: String,
    crateName: String,
    elements: [ElementHit],
    dslModules: [DSLModuleHit]
) -> String {
    // Deterministic order — sort each list independently so two
    // builds with the same input produce byte-identical output
    // (helps SwiftPM's incremental-rebuild fingerprinting).
    let sortedElements = elements.sorted { $0.tag < $1.tag }
    let sortedDSLModules = dslModules.sorted { $0.className < $1.className }

    let registerFn = "_whiskerRegisterModules_\(targetName)"

    var out = """
        // AUTO-GENERATED by `whisker-components-codegen` (SwiftPM build plugin).
        // DO NOT EDIT — re-runs automatically on next `swift build`.
        //
        // Sourced from `@WhiskerComponent("LocalTag")` and
        // `@WhiskerModule` applications in the `\(targetName)`
        // SwiftPM target's source set. Each `@WhiskerComponent` is
        // registered against Lynx with the fully-qualified tag
        // `\(crateName):<LocalTag>`; the cargo crate name (this
        // package's SwiftPM `displayName`) is the namespace, so two
        // unrelated module packages can both declare a `Hello`
        // element without colliding. Each module package owns its
        // own copy of this generated file; the whisker-build-
        // generated aggregator imports every module and calls each
        // per-target register fn from a top-level
        // `WhiskerModuleBehaviors.registerAll()`.
        //
        // Sibling of Android's `rs.whisker.ksp.WhiskerComponentProcessor`.
        //
        // Element registrations:    \(sortedElements.count)
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

    if sortedElements.isEmpty && sortedDSLModules.isEmpty {
        out += "    // (no @WhiskerComponent / @WhiskerModule found)\n"
    }
    for hit in sortedElements {
        // Dual-resolution shape: prefixed name first (default
        // SwiftPM-target mangled name `<TargetName>.<ClassName>`),
        // bare name fallback (for authors who declare
        // `@objc(BareName)` themselves).
        //
        // The Lynx tag is namespaced by the cargo crate name so two
        // unrelated module packages can both declare an element
        // named `Video` without colliding. Matches what the
        // Rust-side `#[whisker::platform_component]` proc macro emits
        // via `concat!(env!("CARGO_PKG_NAME"), ":", tag_local)`.
        let qualifiedTag = "\(crateName):\(hit.tag)"
        out += """
                do {
                    let cls: AnyClass? = NSClassFromString("\(targetName).\(hit.className)")
                        ?? NSClassFromString("\(hit.className)")
                    if let cls = cls {
                        LynxComponentRegistry.registerUI(cls, withName: "\(qualifiedTag)")
                    }
                }

            """
    }
    // ---- Phase L-3 — DSL modules ---------------------------------------
    //
    // For each `@WhiskerModule`-annotated class found in this
    // target's sources, the registration block reads its `definitionLazy`
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
                            whisker_bridge_register_module_dispatch(name, \(shim))
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
        "usage: whisker-components-codegen --target-name <name> --crate-name <pkg> --output <path> <input.swift>...\n".utf8
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
            "whisker-components-codegen: cannot read \(input): \(error)\n".utf8
        ))
        exit(1)
    }
    let tree = Parser.parse(source: source)
    collector.walk(tree)
}

let generated = render(
    targetName: args.targetName,
    crateName: args.crateName,
    elements: collector.elements,
    dslModules: collector.dslModules
)
do {
    try generated.write(toFile: args.outputPath, atomically: true, encoding: .utf8)
} catch {
    FileHandle.standardError.write(Data(
        "whisker-components-codegen: cannot write \(args.outputPath): \(error)\n".utf8
    ))
    exit(1)
}
