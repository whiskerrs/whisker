/// Marks a concrete `Module` declaration for Whisker's generated registry.
///
/// The macro itself emits no runtime code. The SwiftPM build-tool plugin finds
/// the marker and generates one registration function for the containing
/// target, matching Rust's and Android's explicit declaration model.
@attached(peer)
public macro WhiskerModule() =
    #externalMacro(module: "WhiskerModuleMacros", type: "WhiskerModuleMacro")
