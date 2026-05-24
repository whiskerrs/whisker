// Tests for the @WhiskerComponent / @WhiskerModule macro expansions.
//
// Uses Swift's `SwiftSyntaxMacrosTestSupport.assertMacroExpansion`
// to verify the macros produce the expected declarations without
// actually loading Lynx or compiling end-to-end Swift code.

import SwiftSyntax
import SwiftSyntaxMacros
import SwiftSyntaxMacrosTestSupport
import XCTest
@testable import WhiskerComponentsMacros

final class WhiskerComponentMacroTests: XCTestCase {
    private let testMacros: [String: Macro.Type] = [
        "WhiskerComponent":  WhiskerComponentMacro.self,
        "WhiskerModule":   WhiskerModuleMacro.self,
        "WhiskerUIMethod": WhiskerUIMethodMacro.self,
        "WhiskerProp":     WhiskerPropMacro.self,
    ]

    func testElementEmitsTagConstantOnClass() {
        // The macro stores the local tag verbatim — the
        // `<crate-name>:<tag>` namespacing is applied at codegen
        // time by `WhiskerComponentsCodegenPlugin`, not in the
        // macro itself, so the constant here is just `"Hello"`.
        assertMacroExpansion(
            """
            @WhiskerComponent("Hello")
            public class WhiskerHelloComponent {
            }
            """,
            expandedSource: """
            public class WhiskerHelloComponent {

                @objc public static let __whiskerElementTag: String = "Hello"
            }
            """,
            macros: testMacros
        )
    }

    func testElementMissingTagArgumentLeavesClassEmpty() {
        // Compile-time argument validation happens at the parser
        // level — a call like `@WhiskerComponent()` won't reach the
        // expansion. Pass an invalid string-literal expression to
        // confirm we don't crash and just emit nothing.
        assertMacroExpansion(
            """
            @WhiskerComponent(123)
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

            @_cdecl("_whiskerDispatch_LocalStoreImpl")
            public func _whiskerDispatch_LocalStoreImpl(
                _ methodName: UnsafePointer<CChar>?,
                _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
                _ argCount: Int
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

            public func _whiskerRegister_LocalStoreImpl() {
                whisker_bridge_register_module_dispatch(
                    "LocalStore", _whiskerDispatch_LocalStoreImpl)
            }
            """,
            macros: testMacros
        )
    }

    /// Module-name annotation argument is preserved verbatim in
    /// the default-arm error message AND in the registration call
    /// (it's the registration key, not the symbol name) — hyphens
    /// are kept untouched.
    func testModuleNameUsedVerbatimInRegister() {
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

            @_cdecl("_whiskerDispatch_StoreImpl")
            public func _whiskerDispatch_StoreImpl(
                _ methodName: UnsafePointer<CChar>?,
                _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
                _ argCount: Int
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

            public func _whiskerRegister_StoreImpl() {
                whisker_bridge_register_module_dispatch(
                    "Whisker-Store", _whiskerDispatch_StoreImpl)
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

            @_cdecl("_whiskerDispatch_EmptyImpl")
            public func _whiskerDispatch_EmptyImpl(
                _ methodName: UnsafePointer<CChar>?,
                _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
                _ argCount: Int
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

            public func _whiskerRegister_EmptyImpl() {
                whisker_bridge_register_module_dispatch(
                    "Empty", _whiskerDispatch_EmptyImpl)
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

            @_cdecl("_whiskerDispatch_DemoImpl")
            public func _whiskerDispatch_DemoImpl(
                _ methodName: UnsafePointer<CChar>?,
                _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
                _ argCount: Int
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

            public func _whiskerRegister_DemoImpl() {
                whisker_bridge_register_module_dispatch(
                    "Demo", _whiskerDispatch_DemoImpl)
            }
            """,
            macros: testMacros
        )
    }

    /// `@WhiskerUIMethod` emits the `LYNX_UI_METHOD` pair —
    /// `__lynx_ui_method_config__<name>` class method (reflection
    /// probe Lynx walks for) + `<name>:withResult:` Obj-C-selector
    /// dispatch wrapper that delegates to the user method.
    func testUIMethodEmitsConfigProbeAndDispatchPair() {
        assertMacroExpansion(
            """
            @WhiskerUIMethod
            public func play(_ args: [WhiskerValue]) -> WhiskerValue {
                return .null
            }
            """,
            expandedSource: """
            public func play(_ args: [WhiskerValue]) -> WhiskerValue {
                return .null
            }

            @objc public class func __lynx_ui_method_config__play() -> String {
                return "play"
            }

            @objc public func play(
                _ params: NSDictionary?,
                withResult callback: @escaping LynxUIMethodCallbackBlock
            ) {
                let args = WhiskerValue.fromNSDictionary(params)
                let result = self.play(args)
                callback(Int32(kUIMethodSuccess.rawValue), WhiskerValue.toAnyObject(result))
            }
            """,
            macros: testMacros
        )
    }

    /// Visibility is preserved on the emitted peers — an `internal`
    /// (default) user method produces `internal` config probe and
    /// dispatch wrapper too, not `public` ones. Lynx reflection
    /// only needs Obj-C runtime visibility, which `@objc` provides
    /// independently of Swift access control.
    func testUIMethodPreservesInternalVisibility() {
        assertMacroExpansion(
            """
            @WhiskerUIMethod
            func seek(_ args: [WhiskerValue]) -> WhiskerValue {
                return .null
            }
            """,
            expandedSource: """
            func seek(_ args: [WhiskerValue]) -> WhiskerValue {
                return .null
            }

            @objc class func __lynx_ui_method_config__seek() -> String {
                return "seek"
            }

            @objc func seek(
                _ params: NSDictionary?,
                withResult callback: @escaping LynxUIMethodCallbackBlock
            ) {
                let args = WhiskerValue.fromNSDictionary(params)
                let result = self.seek(args)
                callback(Int32(kUIMethodSuccess.rawValue), WhiskerValue.toAnyObject(result))
            }
            """,
            macros: testMacros
        )
    }

    /// `@WhiskerProp("src")` emits the `__lynx_prop_config__src`
    /// class method that Lynx's PropsProcessor walks for at
    /// runtime. The third array slot is the Obj-C type string
    /// inferred from the setter's first parameter.
    func testPropEmitsConfigClassMethodForNSString() {
        assertMacroExpansion(
            """
            @WhiskerProp("src")
            @objc public func setSrc(_ value: NSString, requestReset: Bool) {
            }
            """,
            expandedSource: """
            @objc public func setSrc(_ value: NSString, requestReset: Bool) {
            }

            @objc public class func __lynx_prop_config__src() -> [String] {
                return ["src", "setSrc", "NSString*"]
            }
            """,
            macros: testMacros
        )
    }

    /// Bool param → `"BOOL"` in the type slot. Lynx's PropsProcessor
    /// keys on this string to unbox the incoming value.
    func testPropInfersBoolTypeString() {
        assertMacroExpansion(
            """
            @WhiskerProp("autoplay")
            @objc func setAutoplay(_ value: Bool, requestReset: Bool) {
            }
            """,
            expandedSource: """
            @objc func setAutoplay(_ value: Bool, requestReset: Bool) {
            }

            @objc class func __lynx_prop_config__autoplay() -> [String] {
                return ["autoplay", "setAutoplay", "BOOL"]
            }
            """,
            macros: testMacros
        )
    }
}
