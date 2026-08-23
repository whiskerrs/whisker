import SwiftCompilerPlugin
import SwiftSyntax
import SwiftSyntaxMacros

/// Marker expansion used by the module code-generation build tool.
///
/// Registration remains target-wide work, so the attached macro intentionally
/// emits no peer declaration. Its presence gives Swift source an explicit,
/// compiler-checked `@WhiskerModule` declaration signal; the build tool uses
/// that signal to generate the target's aggregate registration function.
public struct WhiskerModuleMacro: PeerMacro {
    public static func expansion(
        of node: AttributeSyntax,
        providingPeersOf declaration: some DeclSyntaxProtocol,
        in context: some MacroExpansionContext
    ) throws -> [DeclSyntax] {
        []
    }
}

@main
struct WhiskerModuleCompilerPlugin: CompilerPlugin {
    let providingMacros: [Macro.Type] = [WhiskerModuleMacro.self]
}
