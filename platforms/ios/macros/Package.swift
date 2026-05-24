// swift-tools-version:5.9
//
// `WhiskerComponents` — Swift Macro + SwiftPM Build Tool plugin
// powering the Whisker module system's iOS `@WhiskerComponent(...)`
// declaration site.
//
// Four targets:
//
// - **`WhiskerComponents`** (library, public surface): exports the
//   `@WhiskerComponent` attached macro module authors apply to their
//   `LynxUI<View>` subclasses.
//
// - **`WhiskerComponentsMacros`** (macro plugin): the SwiftSyntax-based
//   macro expansion. Decorates the annotated class with metadata
//   the codegen plugin can introspect at SwiftPM build time.
//
// - **`whisker-components-codegen`** (executable): scans the consuming
//   target's `.swift` sources via SwiftSyntax, extracts every
//   `@WhiskerComponent(...)` application, and emits
//   `WhiskerModuleBehaviors.swift` (the iOS counterpart of Android's
//   KSP-generated `WhiskerModuleBehaviors.kt`). Invoked at SPM
//   build time by the plugin below.
//
// - **`WhiskerComponentsCodegenPlugin`** (SPM build plugin): tells
//   SwiftPM "before compiling target T, run
//   `whisker-components-codegen` against its source files; add the
//   produced file to T's compilation." Activated per-target by the
//   consuming `Package.swift`'s `plugins: [.plugin(...)]` clause.

import PackageDescription
import CompilerPluginSupport

let package = Package(
    name: "WhiskerComponents",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerComponents", targets: ["WhiskerComponents"]),
        // Exposing the plugin as a product lets the generated
        // `gen/ios/whisker_modules/Package.swift` reference it via
        // `.plugin(name: "WhiskerComponentsCodegenPlugin",
        //         package: "macros")` on its WhiskerModules
        // target. Without the .plugin product the consumer can't see
        // it (SwiftPM scopes plugins per-package by default).
        .plugin(
            name: "WhiskerComponentsCodegenPlugin",
            targets: ["WhiskerComponentsCodegenPlugin"]
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
        // Public-facing library. Module authors `import WhiskerComponents`
        // and apply `@WhiskerComponent(...)` to their LynxUI subclasses.
        .target(
            name: "WhiskerComponents",
            dependencies: ["WhiskerComponentsMacros"],
            path: "Sources/WhiskerComponents"
        ),

        // Compiler plugin that implements the macro. Loaded by the
        // Swift compiler at the consumer's build time; not linked
        // into the runtime binary.
        .macro(
            name: "WhiskerComponentsMacros",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftSyntaxMacros", package: "swift-syntax"),
                .product(name: "SwiftCompilerPlugin", package: "swift-syntax"),
            ],
            path: "Sources/WhiskerComponentsMacros"
        ),

        // SwiftSyntax-driven codegen tool, invoked by the plugin
        // below. Built once per `swift build` of the consuming
        // package, then re-used to process every WhiskerModules
        // SwiftPM target source file.
        .executableTarget(
            name: "WhiskerComponentsCodegen",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftParser", package: "swift-syntax"),
            ],
            path: "Sources/WhiskerComponentsCodegen"
        ),

        // SwiftPM build-tool plugin. Sandboxed by SwiftPM —
        // can't link Swift libraries directly, only invoke the
        // companion `WhiskerComponentsCodegen` executable. Returns
        // `Command.buildCommand(...)` entries SwiftPM schedules
        // before the consuming target's main compile.
        .plugin(
            name: "WhiskerComponentsCodegenPlugin",
            capability: .buildTool(),
            dependencies: ["WhiskerComponentsCodegen"],
            path: "Plugins/WhiskerComponentsCodegenPlugin"
        ),

        .testTarget(
            name: "WhiskerComponentsTests",
            dependencies: [
                "WhiskerComponents",
                "WhiskerComponentsMacros",
                .product(name: "SwiftSyntaxMacrosTestSupport", package: "swift-syntax"),
            ],
            path: "Tests/WhiskerComponentsTests"
        ),
    ]
)
