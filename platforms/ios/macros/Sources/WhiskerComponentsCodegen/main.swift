// `whisker-components-codegen` — Swift executable that scans .swift
// source files for `@WhiskerComponent("x-tag")` AND `@WhiskerModule(
// "Name")` applications and emits `<TargetName>+Generated.swift`
// containing the matching `LynxComponentRegistry.registerUI(...)`
// and `_whiskerRegister_<ClassName>()` calls.
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

/// One discovered `(name, className)` pair from a `@WhiskerModule`
/// annotation. Same naming convention as `ElementHit`. The
/// platform-side dispatch (`whisker_bridge_invoke_module`) routes
/// through the per-module `@_cdecl` shim the macro emits — we
/// reference that fn by name in the registration call.
struct ModuleHit {
    let name: String
    let className: String
}

final class WhiskerAnnotationCollector: SyntaxVisitor {
    var elements: [ElementHit] = []
    var modules: [ModuleHit] = []

    override func visit(_ node: ClassDeclSyntax) -> SyntaxVisitorContinueKind {
        for attribute in node.attributes {
            guard let attr = attribute.as(AttributeSyntax.self) else { continue }
            guard let attrName = attr.attributeName.as(IdentifierTypeSyntax.self) else { continue }
            let name = attrName.name.text
            guard name == "WhiskerComponent" || name == "WhiskerModule" else { continue }
            guard let value = firstStringArgument(of: attr) else { continue }
            let className = node.name.text
            if name == "WhiskerComponent" {
                elements.append(ElementHit(tag: value, className: className))
            } else {
                modules.append(ModuleHit(name: value, className: className))
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

func render(targetName: String, crateName: String, elements: [ElementHit], modules: [ModuleHit]) -> String {
    // Deterministic order — sort each list independently so two
    // builds with the same input produce byte-identical output
    // (helps SwiftPM's incremental-rebuild fingerprinting).
    let sortedElements = elements.sorted { $0.tag < $1.tag }
    let sortedModules = modules.sorted { $0.name < $1.name }

    let registerFn = "_whiskerRegisterModules_\(targetName)"

    var out = """
        // AUTO-GENERATED by `whisker-components-codegen` (SwiftPM build plugin).
        // DO NOT EDIT — re-runs automatically on next `swift build`.
        //
        // Sourced from `@WhiskerComponent("LocalTag")` and
        // `@WhiskerModule("Name")` applications in the `\(targetName)`
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
        // Element registrations: \(sortedElements.count)
        // Module  registrations: \(sortedModules.count)

        import Foundation
        import Lynx
        // WhiskerRuntime re-exports WhiskerDriver, which carries the
        // C ABI declarations (`whisker_bridge_register_module_dispatch`,
        // `WhiskerValueRaw`, …) the module registration touches.
        import WhiskerRuntime

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

    if sortedElements.isEmpty && sortedModules.isEmpty {
        out += "    // (no @WhiskerComponent / @WhiskerModule-annotated class found)\n"
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
    for hit in sortedModules {
        // Just call the per-module register fn the macro emits in
        // the annotated class's source file. The macro hard-codes
        // the module-name string inside that fn, so we don't need
        // to pass it here.
        let registerName = "_whiskerRegister_\(hit.className)"
        out += """
                \(registerName)()

            """
    }
    out += """
        }

        """
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
    modules: collector.modules
)
do {
    try generated.write(toFile: args.outputPath, atomically: true, encoding: .utf8)
} catch {
    FileHandle.standardError.write(Data(
        "whisker-components-codegen: cannot write \(args.outputPath): \(error)\n".utf8
    ))
    exit(1)
}
