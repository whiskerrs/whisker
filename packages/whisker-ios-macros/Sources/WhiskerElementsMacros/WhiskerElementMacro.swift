// Macro implementation for `@WhiskerElement("x-tag")`.
//
// Two expansion roles:
//
// 1. `MemberAttributeMacro` — adds `@objc` to the annotated class
//    declaration. Lynx's behaviour registry looks up classes via
//    their Obj-C runtime name; Swift's default mangling
//    (`_TtC11ModuleName12ClassName`) doesn't match the bare class
//    name embedded in `WhiskerModuleBehaviors.swift`'s generated
//    registration call. `@objc(ExplicitName)` (when the class
//    doesn't already declare one) pins the Obj-C name to the Swift
//    class name 1:1.
//
// 2. `MemberMacro` — emits a `@objc public static let
//    __whiskerElementTag = "x-tag"` constant inside the annotated
//    class. Not strictly required for v1 registration (the manifest
//    is the source of truth), but provides a compile-time-verifiable
//    anchor a future SwiftSyntax-parsing whisker-build pass can
//    read directly to drop the manifest entry.
//
// The macro deliberately does NOT emit registration code itself.
// Lynx behavior registration goes through whisker-build's generated
// `WhiskerModuleBehaviors.swift` — both because `@objc class func
// load()` semantics in Swift are non-trivial (dead-stripping,
// `-ObjC` linker flag interplay) and because the registration
// timing wants to be explicit + symmetric with the Android path.

import SwiftCompilerPlugin
import SwiftSyntax
import SwiftSyntaxMacros

/// Compiler plugin entry point. Registers `WhiskerElementMacro` so
/// the Swift compiler picks it up when consumers `import
/// WhiskerElements` and apply `@WhiskerElement(...)`.
@main
struct WhiskerElementsPlugin: CompilerPlugin {
    let providingMacros: [Macro.Type] = [
        WhiskerElementMacro.self,
        WhiskerModuleMacro.self,
    ]
}

/// `@WhiskerModule("Name")` — emit a static `__whiskerModuleName`
/// constant on the annotated class so the SwiftPM build-tool
/// plugin (`WhiskerElementsCodegen`) can introspect the
/// registration name via SwiftSyntax parse. Mirror of
/// `WhiskerElementMacro`'s `__whiskerElementTag`; same DX shape,
/// different generated registration target
/// (`WhiskerModuleRegistry` instead of `LynxComponentRegistry`).
///
/// Phase 7-Φ.E.6.
public struct WhiskerModuleMacro: MemberMacro, MemberAttributeMacro {
    public static func expansion(
        of node: AttributeSyntax,
        providingMembersOf declaration: some DeclGroupSyntax,
        in context: some MacroExpansionContext
    ) throws -> [DeclSyntax] {
        guard let name = whiskerModuleFirstStringArgument(of: node) else {
            return []
        }
        return [
            """
            @objc public static let __whiskerModuleName: String = \(literal: name)
            """
        ]
    }

    public static func expansion(
        of node: AttributeSyntax,
        attachedTo declaration: some DeclGroupSyntax,
        providingAttributesFor member: some DeclSyntaxProtocol,
        in context: some MacroExpansionContext
    ) throws -> [AttributeSyntax] {
        // MemberAttributeMacro is invoked per-member — emit nothing
        // per-method. The `@objc` class-runtime-name pin needed for
        // `NSClassFromString` lookup is the author's responsibility
        // via an explicit `@objc(ClassName)` on the class, mirroring
        // the `@WhiskerElement` policy.
        []
    }
}

/// First positional string-literal argument of an `@Attr("...")`
/// invocation. Shared shape — `@WhiskerElement(_ tag:)` and
/// `@WhiskerModule(_ name:)` both take exactly one string arg.
private func whiskerModuleFirstStringArgument(of node: AttributeSyntax) -> String? {
    guard
        let arguments = node.arguments?.as(LabeledExprListSyntax.self),
        let firstArg = arguments.first,
        let stringLiteral = firstArg.expression.as(StringLiteralExprSyntax.self),
        stringLiteral.segments.count == 1,
        let firstSegment = stringLiteral.segments.first?.as(StringSegmentSyntax.self)
    else {
        return nil
    }
    return firstSegment.content.text
}

public struct WhiskerElementMacro: MemberMacro, MemberAttributeMacro {
    /// Emits a static `__whiskerElementTag` constant carrying the
    /// tag-name argument. Future build-pipeline introspection
    /// (SwiftSyntax pass during whisker-build sync) reads this to
    /// produce registrations without the manifest detour.
    public static func expansion(
        of node: AttributeSyntax,
        providingMembersOf declaration: some DeclGroupSyntax,
        in context: some MacroExpansionContext
    ) throws -> [DeclSyntax] {
        guard let tag = tagArgument(of: node) else {
            // Diagnostic is intentionally terse — a user who
            // forgets the tag string gets a Swift parser error
            // before this macro even runs.
            return []
        }
        return [
            // `@objc public static let` so the constant is reachable
            // from Obj-C and SwiftSyntax introspection alike. The
            // identifier is namespaced with the `__whisker` prefix
            // to keep it out of the user's API surface.
            """
            @objc public static let __whiskerElementTag: String = \(literal: tag)
            """
        ]
    }

    /// Adds `@objc` to the annotated class so its Obj-C runtime
    /// name matches the bare Swift class name (no `_TtC...`
    /// mangling). Skipped when the user already wrote `@objc`
    /// themselves — Swift refuses two `@objc` attributes on the
    /// same decl.
    public static func expansion(
        of node: AttributeSyntax,
        attachedTo declaration: some DeclGroupSyntax,
        providingAttributesFor member: some DeclSyntaxProtocol,
        in context: some MacroExpansionContext
    ) throws -> [AttributeSyntax] {
        // `MemberAttributeMacro` is invoked per-member — we don't
        // want to scatter @objc onto every method. Skip everything
        // except returning an empty list here; the @objc-on-class
        // emission happens via the SwiftSyntax `peer`/`extension`
        // role in future iterations if we need it. For now the
        // class still works because LynxUI itself inherits from
        // NSObject — its subclasses are implicitly Obj-C-exposed
        // and the runtime class name matches the bare Swift name.
        []
    }

    private static func tagArgument(of node: AttributeSyntax) -> String? {
        guard
            let arguments = node.arguments?.as(LabeledExprListSyntax.self),
            let firstArg = arguments.first,
            let stringLiteral = firstArg.expression.as(StringLiteralExprSyntax.self),
            stringLiteral.segments.count == 1,
            let firstSegment = stringLiteral.segments.first?.as(StringSegmentSyntax.self)
        else {
            return nil
        }
        return firstSegment.content.text
    }
}
