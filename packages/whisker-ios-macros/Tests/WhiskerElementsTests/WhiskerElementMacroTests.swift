// Tests for the @WhiskerElement macro expansion.
//
// Uses Swift's `SwiftSyntaxMacrosTestSupport.assertMacroExpansion`
// to verify the macro produces the expected member declarations
// without actually loading Lynx or compiling end-to-end Swift code.

import SwiftSyntax
import SwiftSyntaxMacros
import SwiftSyntaxMacrosTestSupport
import XCTest
@testable import WhiskerElementsMacros

final class WhiskerElementMacroTests: XCTestCase {
    private let testMacros: [String: Macro.Type] = [
        "WhiskerElement": WhiskerElementMacro.self,
    ]

    func testEmitsTagConstantOnClass() {
        assertMacroExpansion(
            """
            @WhiskerElement("x-hello")
            public class WhiskerHelloElement {
            }
            """,
            expandedSource: """
            public class WhiskerHelloElement {

                @objc public static let __whiskerElementTag: String = "x-hello"
            }
            """,
            macros: testMacros
        )
    }

    func testMissingTagArgumentLeavesClassEmpty() {
        // Compile-time argument validation happens at the parser
        // level — a call like `@WhiskerElement()` won't reach the
        // expansion. Pass an invalid string-literal expression to
        // confirm we don't crash and just emit nothing.
        assertMacroExpansion(
            """
            @WhiskerElement(123)
            public class BadElement {
            }
            """,
            expandedSource: """
            public class BadElement {
            }
            """,
            macros: testMacros
        )
    }
}
