// swift-tools-version:5.9
//
// `WhiskerModuleMacros` — SwiftPM Build Tool plugin powering the
// Whisker module system's iOS module-discovery codegen.
//
// The package name is kept as `WhiskerModuleMacros` for source
// compatibility with user packages that reference
// `.package(name: "macros", ...)` and consume the plugin product.
// Phase M (Issue #59) removed the `@WhiskerModule` Swift macro
// itself; discovery now finds concrete subclasses of
// `WhiskerModule.Module` directly.
//
// Two targets:
//
// - **`whisker-module-codegen`** (executable): scans the consuming
//   target's `.swift` sources via SwiftSyntax, extracts every
//   class whose inheritance clause names `Module`, and emits
//   `<Target>+Generated.swift` (the iOS counterpart of Android's
//   KSP-generated `<Module>Behaviors.kt`). Invoked at SPM build
//   time by the plugin below.
//
// - **`WhiskerModuleCodegenPlugin`** (SPM build plugin): tells
//   SwiftPM "before compiling target T, run
//   `whisker-module-codegen` against its source files; add the
//   produced file to T's compilation." Activated per-target by the
//   consuming `Package.swift`'s `plugins: [.plugin(...)]` clause.

import PackageDescription
import CompilerPluginSupport

let package = Package(
    name: "WhiskerModuleMacros",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        // Exposing the plugin as a product lets the generated
        // `gen/ios/whisker_modules/Package.swift` reference it via
        // `.plugin(name: "WhiskerModuleCodegenPlugin",
        //         package: "macros")` on its WhiskerModules
        // target. Without the .plugin product the consumer can't see
        // it (SwiftPM scopes plugins per-package by default).
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            targets: ["WhiskerModuleCodegenPlugin"]
        ),
    ],
    dependencies: [
        // swift-syntax version compatibility: 510.0.0 series tracks
        // Swift 5.10 / Xcode 15.3+. The Lynx fork's CI uses Xcode 16
        // (objectVersion 77 pbxproj), which ships Swift 6 — both
        // 510 and 600 release lines work, 510 is the broader
        // compatibility floor.
        .package(url: "https://github.com/swiftlang/swift-syntax.git", from: "510.0.0"),
    ],
    targets: [
        // SwiftSyntax-driven codegen tool, invoked by the plugin
        // below. Built once per `swift build` of the consuming
        // package, then re-used to process every WhiskerModules
        // SwiftPM target source file.
        .executableTarget(
            name: "WhiskerModuleCodegen",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftParser", package: "swift-syntax"),
            ],
            path: "Sources/WhiskerModuleCodegen"
        ),

        // SwiftPM build-tool plugin. Sandboxed by SwiftPM —
        // can't link Swift libraries directly, only invoke the
        // companion `WhiskerModuleCodegen` executable. Returns
        // `Command.buildCommand(...)` entries SwiftPM schedules
        // before the consuming target's main compile.
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            capability: .buildTool(),
            dependencies: ["WhiskerModuleCodegen"],
            path: "Plugins/WhiskerModuleCodegenPlugin"
        ),
    ]
)
