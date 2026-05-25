// Whisker module-system iOS attached macro — `@WhiskerModule`.
//
// `WhiskerComponents` exposes the single `@WhiskerModule` marker
// module authors apply to a `Module` subclass authored with the
// ModuleDefinition DSL. The `WhiskerComponentsCodegen` SwiftPM
// build-tool plugin discovers the attribute and emits the Lynx
// registration; the macro itself expands to nothing.

/// Marks a `Module` subclass as a Whisker module the build should
/// register.
///
/// Applying `@WhiskerModule` is the registration trigger — the
/// `WhiskerComponentsCodegen` SwiftPM build-tool plugin scans the
/// target's sources for it and emits the Lynx behaviour /
/// module-dispatch registration into `<Target>+Generated.swift`.
/// The module's local tag / name comes from the `Name("…")` entry
/// inside `definition()`, so the annotation itself takes no
/// arguments. It plays the role of Expo's
/// `expo-module.config.json` entry, but inline at the declaration
/// (idiomatic Swift: an attribute marks the entry point, like
/// `@main`).
///
/// ```swift
/// import WhiskerComponents   // @WhiskerModule
/// import WhiskerModule       // Module, ModuleDefinition, DSL
///
/// @WhiskerModule
/// public final class VideoModule: Module {
///     public override func definition() -> ModuleDefinition {
///         Name("Video")
///         View(VideoView.self) {
///             Prop("src") { (view: VideoView, value: String) in view.setSrc(value) }
///             Function("play") { (view: VideoView) in view.play() }
///         }
///     }
/// }
/// ```
///
/// Companion of Android's `@WhiskerModule` (KSP). The macro itself
/// expands to nothing — it's a pure marker; the codegen plugin
/// does the registration work by scanning for the attribute.
///
/// Declared as a `member` macro (not `peer`) so it's valid on a
/// top-level class: Swift forbids `peer` macros that introduce
/// `arbitrary` names at global scope, but a `member` macro's names
/// live inside the type's scope. The macro produces no members —
/// the role is just a vehicle for a valid marker attribute.
@attached(member, names: arbitrary)
public macro WhiskerModule() =
    #externalMacro(module: "WhiskerComponentsMacros", type: "WhiskerModuleMacro")
