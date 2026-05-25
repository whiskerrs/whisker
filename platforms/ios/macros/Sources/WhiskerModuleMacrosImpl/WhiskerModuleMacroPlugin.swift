// Macro implementation for `@WhiskerModule`.
//
// `@WhiskerModule` is a pure marker — it expands to nothing. It's
// applied to a `Module` subclass authored with the ModuleDefinition
// DSL:
//
//   ```swift
//   @WhiskerModule
//   public final class LocalStoreModule: Module {
//       public override func definition() -> ModuleDefinition {
//           ModuleDefinition {
//               Name("WhiskerLocalStore")
//               Function("save") { (key: String, value: String) in … }
//           }
//       }
//   }
//   ```
//
// The `WhiskerModuleCodegen` SwiftPM build-tool plugin scans
// each target's sources for the `@WhiskerModule` attribute and
// emits the registration block (Lynx behavior for view-bearing
// modules; `whisker_bridge_register_module_dispatch` for view-less
// ones) into `<Target>+Generated.swift`. The macro itself does no
// codegen — it exists only so `@WhiskerModule` is a valid Swift
// attribute the plugin can key on.

import SwiftCompilerPlugin
import SwiftSyntax
import SwiftSyntaxMacros

/// Compiler plugin entry point. Registers the `@WhiskerModule` macro
/// so the Swift compiler picks it up when consumers
/// `import WhiskerModuleMacros`.
@main
struct WhiskerModuleMacrosPlugin: CompilerPlugin {
    let providingMacros: [Macro.Type] = [
        WhiskerModuleMacro.self,
    ]
}

/// `@WhiskerModule` — a pure marker attribute applied to a `Module`
/// subclass. The `WhiskerModuleCodegen` SwiftPM plugin
/// discovers the attribute via SwiftSyntax and emits the
/// registration; the macro itself expands to nothing.
///
/// Implemented as a `MemberMacro` (rather than `PeerMacro`) so the
/// attribute is valid on a top-level class — Swift rejects a
/// `peer` macro with `names: arbitrary` at global scope, but a
/// `member` macro's names are scoped to the type. It adds no
/// members; the role is just a vehicle for a valid marker.
public struct WhiskerModuleMacro: MemberMacro {
    public static func expansion(
        of node: AttributeSyntax,
        providingMembersOf declaration: some DeclGroupSyntax,
        in context: some MacroExpansionContext
    ) throws -> [DeclSyntax] {
        []
    }
}
