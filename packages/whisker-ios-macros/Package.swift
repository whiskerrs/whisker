// swift-tools-version:5.9
//
// `WhiskerElements` — Swift Macro + SwiftPM Build Tool plugin
// powering the Whisker module system's iOS `@WhiskerElement(...)`
// declaration site.
//
// Four targets:
//
// - **`WhiskerElements`** (library, public surface): exports the
//   `@WhiskerElement` attached macro module authors apply to their
//   `LynxUI<View>` subclasses.
//
// - **`WhiskerElementsMacros`** (macro plugin): the SwiftSyntax-based
//   macro expansion. Decorates the annotated class with metadata
//   the codegen plugin can introspect at SwiftPM build time.
//
// - **`whisker-elements-codegen`** (executable): scans the consuming
//   target's `.swift` sources via SwiftSyntax, extracts every
//   `@WhiskerElement(...)` application, and emits
//   `WhiskerModuleBehaviors.swift` (the iOS counterpart of Android's
//   KSP-generated `WhiskerModuleBehaviors.kt`). Invoked at SPM
//   build time by the plugin below.
//
// - **`WhiskerElementsCodegenPlugin`** (SPM build plugin): tells
//   SwiftPM "before compiling target T, run
//   `whisker-elements-codegen` against its source files; add the
//   produced file to T's compilation." Activated per-target by the
//   consuming `Package.swift`'s `plugins: [.plugin(...)]` clause.

import PackageDescription
import CompilerPluginSupport

let package = Package(
    name: "WhiskerElements",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerElements", targets: ["WhiskerElements"]),
        // Exposing the plugin as a product lets the generated
        // `gen/ios/whisker_modules/Package.swift` reference it via
        // `.plugin(name: "WhiskerElementsCodegenPlugin",
        //         package: "whisker-ios-macros")` on its WhiskerModules
        // target. Without the .plugin product the consumer can't see
        // it (SwiftPM scopes plugins per-package by default).
        .plugin(
            name: "WhiskerElementsCodegenPlugin",
            targets: ["WhiskerElementsCodegenPlugin"]
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
        // Public-facing library. Module authors `import WhiskerElements`
        // and apply `@WhiskerElement(...)` to their LynxUI subclasses.
        .target(
            name: "WhiskerElements",
            dependencies: ["WhiskerElementsMacros"],
            path: "Sources/WhiskerElements"
        ),

        // Compiler plugin that implements the macro. Loaded by the
        // Swift compiler at the consumer's build time; not linked
        // into the runtime binary.
        .macro(
            name: "WhiskerElementsMacros",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftSyntaxMacros", package: "swift-syntax"),
                .product(name: "SwiftCompilerPlugin", package: "swift-syntax"),
            ],
            path: "Sources/WhiskerElementsMacros"
        ),

        // SwiftSyntax-driven codegen tool, invoked by the plugin
        // below. Built once per `swift build` of the consuming
        // package, then re-used to process every WhiskerModules
        // SwiftPM target source file.
        .executableTarget(
            name: "WhiskerElementsCodegen",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftParser", package: "swift-syntax"),
            ],
            path: "Sources/WhiskerElementsCodegen"
        ),

        // SwiftPM build-tool plugin. Sandboxed by SwiftPM —
        // can't link Swift libraries directly, only invoke the
        // companion `WhiskerElementsCodegen` executable. Returns
        // `Command.buildCommand(...)` entries SwiftPM schedules
        // before the consuming target's main compile.
        .plugin(
            name: "WhiskerElementsCodegenPlugin",
            capability: .buildTool(),
            dependencies: ["WhiskerElementsCodegen"],
            path: "Plugins/WhiskerElementsCodegenPlugin"
        ),

        .testTarget(
            name: "WhiskerElementsTests",
            dependencies: [
                "WhiskerElements",
                "WhiskerElementsMacros",
                .product(name: "SwiftSyntaxMacrosTestSupport", package: "swift-syntax"),
            ],
            path: "Tests/WhiskerElementsTests"
        ),
    ]
)
