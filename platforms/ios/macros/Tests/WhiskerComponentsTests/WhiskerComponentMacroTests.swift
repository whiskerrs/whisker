// Tests for the @WhiskerModule macro expansion.
//
// Uses Swift's `SwiftSyntaxMacrosTestSupport.assertMacroExpansion`
// to verify the macro produces the expected declarations without
// actually loading Lynx or compiling end-to-end Swift code.

import SwiftSyntax
import SwiftSyntaxMacros
import SwiftSyntaxMacrosTestSupport
import XCTest
@testable import WhiskerComponentsMacros

final class WhiskerComponentMacroTests: XCTestCase {
    private let testMacros: [String: Macro.Type] = [
        "WhiskerModule": WhiskerModuleMacro.self,
    ]

    /// `@WhiskerModule` is a pure marker — it expands to nothing.
    /// Discovery + registration is done by the
    /// `WhiskerComponentsCodegen` SwiftPM build plugin, which scans
    /// sources for the attribute and emits the DSL module's
    /// registration. The macro exists only so `@WhiskerModule` is a
    /// valid Swift attribute.
    func testModuleMarkerExpandsToNothing() {
        assertMacroExpansion(
            """
            @WhiskerModule
            public final class LocalStoreModule: Module {
                public override func definition() -> ModuleDefinition {
                    ModuleDefinition {
                        Name("WhiskerLocalStore")
                    }
                }
            }
            """,
            expandedSource: """
            public final class LocalStoreModule: Module {
                public override func definition() -> ModuleDefinition {
                    ModuleDefinition {
                        Name("WhiskerLocalStore")
                    }
                }
            }
            """,
            macros: testMacros
        )
    }
}
