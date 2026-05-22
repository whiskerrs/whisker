// swift-tools-version:5.9
//
// `WhiskerElements` — Swift Macro library powering the Whisker
// module system's iOS `@WhiskerElement("x-tag")` declaration site.
//
// Two targets:
//
// - **`WhiskerElements`** (library, public surface): exports the
//   `@WhiskerElement` attached macro that module authors apply to
//   their `LynxUI<View>` subclasses. The macro itself is implemented
//   in the companion `WhiskerElementsMacros` target.
//
// - **`WhiskerElementsMacros`** (macro plugin): the SwiftSyntax-based
//   macro expansion. Adds `@objc(<ClassName>)` to the annotated
//   class so the Obj-C runtime name matches the `class = "..."`
//   string `whisker.module.toml` carries through to the generated
//   `WhiskerModuleBehaviors.swift` — that bridge between the two is
//   what makes the cargo-driven behaviour registry able to find the
//   class by string at app init.
//
// Long-term v2: drop the toml `[ios].behaviors` declaration and have
// whisker-build parse the macro applications directly via
// SwiftSyntax. v1 keeps the manifest explicit to keep the build
// pipeline simple.

import PackageDescription
import CompilerPluginSupport

let package = Package(
    name: "WhiskerElements",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerElements", targets: ["WhiskerElements"]),
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
