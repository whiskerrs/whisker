// Tests for the @WhiskerElement / @WhiskerModule macro expansions.
//
// Uses Swift's `SwiftSyntaxMacrosTestSupport.assertMacroExpansion`
// to verify the macros produce the expected declarations without
// actually loading Lynx or compiling end-to-end Swift code.

import SwiftSyntax
import SwiftSyntaxMacros
import SwiftSyntaxMacrosTestSupport
import XCTest
@testable import WhiskerElementsMacros

final class WhiskerElementMacroTests: XCTestCase {
    private let testMacros: [String: Macro.Type] = [
        "WhiskerElement": WhiskerElementMacro.self,
        "WhiskerModule":  WhiskerModuleMacro.self,
    ]

    func testElementEmitsTagConstantOnClass() {
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

    func testElementMissingTagArgumentLeavesClassEmpty() {
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

    /// `@WhiskerModule` emits a top-level `@_cdecl` dispatch shim
    /// with one switch arm per instance method on the class. The
    /// shim is a peer of the annotated class, not a member —
    /// `@_cdecl` requires top-level placement.
    func testModuleEmitsCDeclDispatch() {
        assertMacroExpansion(
            """
            @WhiskerModule("LocalStore")
            public class LocalStoreImpl {
                func save(_ args: [WhiskerValue]) -> WhiskerValue { .null }
                func load(_ args: [WhiskerValue]) -> WhiskerValue { .null }
            }
            """,
            expandedSource: """
            public class LocalStoreImpl {
                func save(_ args: [WhiskerValue]) -> WhiskerValue { .null }
                func load(_ args: [WhiskerValue]) -> WhiskerValue { .null }
            }

            @_cdecl("_whiskerDispatch_LocalStore")
            public func _whiskerDispatch_LocalStore(
                methodName: UnsafePointer<CChar>?,
                argsPtr: UnsafePointer<WhiskerValueRaw>?,
                argCount: Int
            ) -> WhiskerValueRaw {
                let method = methodName == nil ? "" : String(cString: methodName!)
                let decoded = WhiskerValue.decodeArray(argsPtr, count: argCount)
                let instance = LocalStoreImpl()
                let result: WhiskerValue
                switch method {
                case "save":
                    result = instance.save(decoded)
                case "load":
                    result = instance.load(decoded)
                default:
                    result = .error("unknown method \\(method) on LocalStore")
                }
                return result.toRaw()
            }
            """,
            macros: testMacros
        )
    }

    /// Module name containing a hyphen — sanitised to `_` in the
    /// `@_cdecl` symbol so the C linker is happy.
    func testModuleNameSanitisedForCDeclSymbol() {
        assertMacroExpansion(
            """
            @WhiskerModule("Whisker-Store")
            public class StoreImpl {
                func ping(_ args: [WhiskerValue]) -> WhiskerValue { .null }
            }
            """,
            expandedSource: """
            public class StoreImpl {
                func ping(_ args: [WhiskerValue]) -> WhiskerValue { .null }
            }

            @_cdecl("_whiskerDispatch_Whisker_Store")
            public func _whiskerDispatch_Whisker_Store(
                methodName: UnsafePointer<CChar>?,
                argsPtr: UnsafePointer<WhiskerValueRaw>?,
                argCount: Int
            ) -> WhiskerValueRaw {
                let method = methodName == nil ? "" : String(cString: methodName!)
                let decoded = WhiskerValue.decodeArray(argsPtr, count: argCount)
                let instance = StoreImpl()
                let result: WhiskerValue
                switch method {
                case "ping":
                    result = instance.ping(decoded)
                default:
                    result = .error("unknown method \\(method) on Whisker-Store")
                }
                return result.toRaw()
            }
            """,
            macros: testMacros
        )
    }

    /// Class with no methods — dispatch shim still emitted so
    /// registration resolves at the C linker level; every call
    /// drops into the default arm and returns an error value.
    func testModuleWithNoMethodsEmitsDefaultOnlySwitch() {
        assertMacroExpansion(
            """
            @WhiskerModule("Empty")
            public class EmptyImpl {
            }
            """,
            expandedSource: """
            public class EmptyImpl {
            }

            @_cdecl("_whiskerDispatch_Empty")
            public func _whiskerDispatch_Empty(
                methodName: UnsafePointer<CChar>?,
                argsPtr: UnsafePointer<WhiskerValueRaw>?,
                argCount: Int
            ) -> WhiskerValueRaw {
                let method = methodName == nil ? "" : String(cString: methodName!)
                let decoded = WhiskerValue.decodeArray(argsPtr, count: argCount)
                let instance = EmptyImpl()
                let result: WhiskerValue
                switch method {
                default:
                    result = .error("unknown method \\(method) on Empty")
                }
                return result.toRaw()
            }
            """,
            macros: testMacros
        )
    }

    /// `static` and `private` methods are filtered out of the
    /// dispatch switch — they aren't part of the module's C-bridge
    /// surface.
    func testModuleSkipsStaticAndPrivateMethods() {
        assertMacroExpansion(
            """
            @WhiskerModule("Demo")
            public class DemoImpl {
                static func makeOne() -> DemoImpl { DemoImpl() }
                private func helper(_ args: [WhiskerValue]) -> WhiskerValue { .null }
                func ping(_ args: [WhiskerValue]) -> WhiskerValue { .null }
            }
            """,
            expandedSource: """
            public class DemoImpl {
                static func makeOne() -> DemoImpl { DemoImpl() }
                private func helper(_ args: [WhiskerValue]) -> WhiskerValue { .null }
                func ping(_ args: [WhiskerValue]) -> WhiskerValue { .null }
            }

            @_cdecl("_whiskerDispatch_Demo")
            public func _whiskerDispatch_Demo(
                methodName: UnsafePointer<CChar>?,
                argsPtr: UnsafePointer<WhiskerValueRaw>?,
                argCount: Int
            ) -> WhiskerValueRaw {
                let method = methodName == nil ? "" : String(cString: methodName!)
                let decoded = WhiskerValue.decodeArray(argsPtr, count: argCount)
                let instance = DemoImpl()
                let result: WhiskerValue
                switch method {
                case "ping":
                    result = instance.ping(decoded)
                default:
                    result = .error("unknown method \\(method) on Demo")
                }
                return result.toRaw()
            }
            """,
            macros: testMacros
        )
    }
}
