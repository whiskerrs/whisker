// Phase 7-A.4 skeleton.
//
// Final shape (Phase 7-B.6 + 7-C):
//
//   open class WhiskerView<V: UIView>: LynxUI<V> { … }
//
//   @attached(member, names: arbitrary)
//   public macro WhiskerElement(_ tag: String) = #externalMacro(
//       module: "WhiskerNativeRuntimeMacros",
//       type: "WhiskerElementMacro")
//
//   @attached(member, names: arbitrary)
//   public macro WhiskerModule(_ name: String) = #externalMacro(
//       module: "WhiskerNativeRuntimeMacros",
//       type: "WhiskerModuleMacro")
//
// For now this file just declares the module's existence so the
// SPM package builds cleanly and downstream module crates can
// declare a dep on it.

import Foundation

/// Sentinel value so `swift build` produces a non-empty module.
/// Removed in Phase 7-B.6 once the real surface lands.
public enum WhiskerNativeRuntime {
    public static let schema: String = "phase-7-a.4"
}
