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

    /// `@WhiskerModule` is a pure marker after the discovery
    /// overhaul — it expands to nothing. Discovery + registration is
    /// done by the `WhiskerComponentsCodegen` SwiftPM build plugin,
    /// which scans sources for the attribute and emits the DSL
    /// module's registration. The macro exists only so
    /// `@WhiskerModule` is a valid Swift attribute.
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
