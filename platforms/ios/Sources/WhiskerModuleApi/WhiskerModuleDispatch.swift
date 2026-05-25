// Phase L-3 — module-level (view-less) `Function` dispatch (iOS).
//
// For view-less DSL modules (no `View(...)` block), the SwiftPM
// codegen plugin registers a `@_cdecl` shim with the C bridge via
// `whisker_bridge_register_module_dispatch(name, shim)`. The shim
// calls `dispatchModuleFunctionRaw(...)`, which decodes the C args
// into `[WhiskerValue]`, looks the function up in `definitionLazy`
// by name, runs the closure, and re-encodes the result.
//
// Kept in its own file (separate from `WhiskerModuleRegistrar.swift`)
// because it pulls in `WhiskerValue` / `WhiskerValueRaw` — the
// C-ABI surface that can't be host-compiled. The L-2b view-install
// path in the registrar stays free of that dependency so its
// runtime smoke test keeps building on plain host macOS.

import Foundation

extension WhiskerModule {
    /// Dispatch a module-level `Function` by name. Public so it's
    /// unit-testable against `[WhiskerValue]` without the C ABI.
    public func dispatchModuleFunction(
        _ method: String,
        _ args: [WhiskerValue]
    ) -> WhiskerValue {
        guard let fn = self.definitionLazy.functions.first(where: { $0.name == method }) else {
            return .error("unknown method `\(method)`")
        }
        // Unwrap WhiskerValue → Swift-native for the closure's
        // `as? A` casts; module-level functions get `nil` for the
        // view argument.
        let rawArgs: [Any?] = args.map { WhiskerDSLValue.toSwift($0) }
        let result = fn.handler(nil, rawArgs)
        return WhiskerDSLValue.fromSwift(result)
    }

    /// C-ABI bridge the codegen-emitted `@_cdecl` shim calls. Lives
    /// in `WhiskerModuleApi` (same module as `WhiskerValue`) so it
    /// can reach the `internal` `decodeArray` / `toRaw` helpers —
    /// keeping those off the public surface while still letting the
    /// generated shim (in the module author's target) dispatch with
    /// a single public call.
    public func dispatchModuleFunctionRaw(
        _ methodName: UnsafePointer<CChar>?,
        _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
        _ argCount: Int
    ) -> WhiskerValueRaw {
        let method = methodName == nil ? "" : String(cString: methodName!)
        let args = WhiskerValue.decodeArray(argsPtr, count: argCount)
        return dispatchModuleFunction(method, args).toRaw()
    }
}

// MARK: - DSL <-> WhiskerValue scalar conversion

/// Conversions between `WhiskerValue` and the Swift-native values
/// DSL `Function` closures take / return. Distinct from
/// `WhiskerValue.toAnyObject` (which boxes numbers as `NSNumber`
/// for Lynx callbacks) — here we keep Swift-native `Bool` / `Int64`
/// / `Double` / `String` so the closures' `as? T` downcasts hit.
internal enum WhiskerDSLValue {
    static func toSwift(_ v: WhiskerValue) -> Any? {
        switch v {
        case .null: return nil
        case .bool(let b): return b
        case .int(let i): return i
        case .float(let f): return f
        case .string(let s): return s
        case .bytes(let d): return d
        case .array(let a): return a.map { toSwift($0) }
        case .map(let m): return m.mapValues { toSwift($0) }
        case .error(let e): return e
        }
    }

    static func fromSwift(_ value: Any?) -> WhiskerValue {
        // Flatten one level of Optional so a closure returning
        // `String?` == nil collapses to `.null` rather than
        // tripping the `default` arm (Swift boxes `Optional<String>
        // .none` inside `Optional<Any>.some`).
        guard let unwrapped = unwrapOptional(value) else { return .null }
        switch unwrapped {
        case let v as WhiskerValue: return v
        case let b as Bool: return .bool(b)
        case let i as Int: return .int(Int64(i))
        case let i as Int64: return .int(i)
        case let d as Double: return .float(d)
        case let f as Float: return .float(Double(f))
        case let s as String: return .string(s)
        case let d as Data: return .bytes(d)
        default:
            return .error("unsupported return type \(type(of: unwrapped))")
        }
    }

    /// Returns `nil` for `Optional.none` of any wrapped type (incl.
    /// the `Optional<Optional<…>>` shape that arises when a closure
    /// returning `T?` is stored as `Any?`); otherwise the unwrapped
    /// value.
    private static func unwrapOptional(_ value: Any?) -> Any? {
        guard let value = value else { return nil }
        let mirror = Mirror(reflecting: value)
        guard mirror.displayStyle == .optional else { return value }
        guard let inner = mirror.children.first else { return nil }
        return inner.value
    }
}
