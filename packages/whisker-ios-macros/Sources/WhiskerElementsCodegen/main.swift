// `whisker-elements-codegen` — Swift executable that scans .swift
// source files for `@WhiskerElement("x-tag")` AND `@WhiskerModule(
// "Name")` applications and emits `WhiskerModuleBehaviors.swift`
// containing the matching `LynxComponentRegistry.registerUI(...)`
// and `whisker_bridge_register_module_dispatch(name, dispatch_fn)`
// calls.
//
// Invoked at SwiftPM build time by `WhiskerElementsCodegenPlugin`
// (under `../../Plugins/`). The plugin passes:
//   --output <path>             ← target for the generated .swift
//   <input1.swift> <input2.swift> …
//
// Apple's SwiftSyntax parses each input file into an AST; we walk
// it to find any class declaration carrying either annotation,
// pull the string literal argument, and produce one registration
// line per match.
//
// Companion of `WhiskerElementProcessor.kt` (Android KSP). Same
// shape — annotations in, generated registry out. Apple's
// SwiftSyntax is the iOS equivalent of KSP's `Resolver`.
//
// Phase 7-Φ.F: module registrations now point at the per-module
// `@_cdecl` C dispatch shim the macro itself emits (one top-level
// `_whiskerDispatch_<sanitised name>` function per `@WhiskerModule`).
// The previous Obj-C `WhiskerModuleRegistry` class is gone.

import Foundation
import SwiftSyntax
import SwiftParser

// ---- CLI argument parsing ----------------------------------------------------

struct Args {
    let outputPath: String
    let inputs: [String]
}

func parseArgs(_ argv: [String]) -> Args? {
    var output: String?
    var inputs: [String] = []
    var i = 1 // skip argv[0]
    while i < argv.count {
        switch argv[i] {
        case "--output":
            guard i + 1 < argv.count else { return nil }
            output = argv[i + 1]
            i += 2
        default:
            inputs.append(argv[i])
            i += 1
        }
    }
    guard let output else { return nil }
    return Args(outputPath: output, inputs: inputs)
}

// ---- AST walker --------------------------------------------------------------

/// One discovered `(tag, className)` pair from a `@WhiskerElement`
/// annotation. `className` is the bare Swift class name as
/// written; the generated registry's `NSClassFromString` lookup
/// applies both the `WhiskerModules.` SwiftPM-target prefix AND
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
            guard name == "WhiskerElement" || name == "WhiskerModule" else { continue }
            guard let value = firstStringArgument(of: attr) else { continue }
            let className = node.name.text
            if name == "WhiskerElement" {
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

/// Make a Swift identifier out of a module name. Replaces any
/// non-`[A-Za-z0-9_]` byte with `_` so the dispatch fn name is
/// always a legal Swift identifier. Must match the sanitisation
/// `WhiskerElementMacro.swift` does at macro-emission time —
/// otherwise the registration call would reference an undefined
/// symbol.
private func sanitiseIdentifier(_ raw: String) -> String {
    var out = ""
    out.reserveCapacity(raw.count)
    for char in raw.unicodeScalars {
        if (char.value >= 0x30 && char.value <= 0x39) ||  // 0-9
           (char.value >= 0x41 && char.value <= 0x5A) ||  // A-Z
           (char.value >= 0x61 && char.value <= 0x7A) ||  // a-z
           char.value == 0x5F {                           // _
            out.unicodeScalars.append(char)
        } else {
            out.append("_")
        }
    }
    return out
}

// ---- Codegen -----------------------------------------------------------------

func render(elements: [ElementHit], modules: [ModuleHit]) -> String {
    // Deterministic order — sort each list independently so two
    // builds with the same input produce byte-identical output
    // (helps SwiftPM's incremental-rebuild fingerprinting).
    let sortedElements = elements.sorted { $0.tag < $1.tag }
    let sortedModules = modules.sorted { $0.name < $1.name }

    var out = """
        // AUTO-GENERATED by `whisker-elements-codegen` (SwiftPM build plugin).
        // DO NOT EDIT — re-runs automatically on next `swift build`.
        //
        // Sourced from `@WhiskerElement("x-tag")` and `@WhiskerModule(
        // "Name")` applications in the WhiskerModules SwiftPM target's
        // source set — i.e. every module crate's `[ios].swift_sources`
        // staged into
        // `gen/ios/whisker_modules/Sources/WhiskerModules/<crate>/`.
        //
        // Sibling of Android's `rs.whisker.ksp.WhiskerElementProcessor`
        // (under `packages/whisker-android-ksp/`).
        //
        // Element registrations: \(sortedElements.count)
        // Module  registrations: \(sortedModules.count)

        import Foundation
        import Lynx
        // WhiskerRuntime re-exports WhiskerDriver, which carries the
        // C ABI declarations (`whisker_bridge_register_module_dispatch`,
        // `WhiskerValueRaw`, …) the module registration touches.
        import WhiskerRuntime

        // Forward declarations for the per-module `@_cdecl` dispatch
        // shims the macro emits as top-level peers of each
        // `@WhiskerModule`-annotated class. Each shim's Swift symbol
        // name matches its `@_cdecl` exposed name, so we can take
        // its address by Swift identifier — the conversion to the
        // C function-pointer type happens implicitly at the call
        // site below.
        \(sortedModules.isEmpty ? "// (no module dispatch shims)\n" : "")
        @objc public final class WhiskerModuleBehaviors: NSObject {
            private static var registered = false
            private static let lock = NSLock()

            @objc public static func registerAll() {
                lock.lock()
                defer { lock.unlock() }
                if registered { return }
                registered = true

        """

    if sortedElements.isEmpty && sortedModules.isEmpty {
        out += "        // (no @WhiskerElement / @WhiskerModule-annotated class found)\n"
    }
    for hit in sortedElements {
        // Dual-resolution shape: prefixed name first
        // (default SwiftPM-target mangled), bare name fallback
        // (for authors who declare `@objc(BareName)` themselves).
        out += """
                    do {
                        let cls = NSClassFromString("WhiskerModules.\(hit.className)")
                            ?? NSClassFromString("\(hit.className)")
                        if let cls = cls {
                            LynxComponentRegistry.registerUI(cls, withName: "\(hit.tag)")
                        }
                    }

            """
    }
    for hit in sortedModules {
        // Hand `_whiskerDispatch_<sanitised name>` directly to the
        // bridge's register fn. Swift converts the function
        // reference to the C `WhiskerModuleDispatchFn` typedef
        // automatically because the macro-emitted decl is
        // `@_cdecl` (C calling convention).
        let symbol = "_whiskerDispatch_\(sanitiseIdentifier(hit.name))"
        out += """
                    whisker_bridge_register_module_dispatch(
                        "\(hit.name)", \(symbol))

            """
    }
    out += """
            }
        }

        """
    return out
}

// ---- Main --------------------------------------------------------------------

guard let args = parseArgs(CommandLine.arguments) else {
    FileHandle.standardError.write(Data(
        "usage: whisker-elements-codegen --output <path> <input.swift>...\n".utf8
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
            "whisker-elements-codegen: cannot read \(input): \(error)\n".utf8
        ))
        exit(1)
    }
    let tree = Parser.parse(source: source)
    collector.walk(tree)
}

let generated = render(elements: collector.elements, modules: collector.modules)
do {
    try generated.write(toFile: args.outputPath, atomically: true, encoding: .utf8)
} catch {
    FileHandle.standardError.write(Data(
        "whisker-elements-codegen: cannot write \(args.outputPath): \(error)\n".utf8
    ))
    exit(1)
}
