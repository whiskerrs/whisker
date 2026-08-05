// swift-tools-version:5.9
//
// Whisker SwiftPM package — the remote-consumable iOS entry point for
// apps built outside the monorepo (`cargo install whisker-cli` users).
// Mirrors the `platforms/ios/Package.swift` surface but with the two
// differences that make it resolvable from a tagged git URL:
//
//   1. Lynx binaryTargets reference the fork's GitHub Release zips via
//      `url: + checksum:` (same as platforms/ios/Package.swift).
//   2. Targets reference `platforms/ios/Sources/...` via `path:` so the
//      Swift sources stay in one place — no duplication between this
//      root package and the monorepo-local `platforms/ios/Package.swift`.
//
// `WhiskerDriver` is NOT a binaryTarget: it wraps the user's
// `#[whisker::main]` Rust crate, so it's compiled per-app by the
// `whisker-build ios` Run Script Build Phase that whisker-cng injects
// into the generated pbxproj. The Swift sources `@_exported import
// WhiskerCBridge` (a header-only systemLibrary mirroring the C ABI); the
// undefined refs resolve against the per-app `WhiskerDriver.framework`
// at the host app's link step.
//
// SPM requires the manifest at the repository root, so this lives at the
// repo root (it cannot point at the `platforms/ios/` subdirectory from a
// remote URL). `platforms/ios/Package.swift` stays in place for monorepo
// developers iterating on the Swift sources before tagging — the cli
// emits an `XCLocalSwiftPackageReference` against it for in-workspace
// builds and an `XCRemoteSwiftPackageReference` against this root package
// (by tag) for external users.
//
// Keep the Lynx tag + checksums here in lockstep with
// `platforms/ios/Package.swift` and the `LYNX_FORK_TAG` /
// `LYNX_*_SHA256` pins in the build pipeline.

import PackageDescription

let package = Package(
    name: "Whisker",
    platforms: [
        .iOS(.v13),
        // macOS floor for the SwiftPM build-tool plugin's codegen
        // executable, which runs on the host during a build.
        .macOS(.v13),
    ],
    products: [
        .library(name: "WhiskerModule", targets: ["WhiskerModule"]),
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
        // Lynx surface exposed as products so a module's `Package.swift`
        // can pull individual frameworks via
        // `.product(name: "Lynx", package: "whisker")`.
        .library(name: "Lynx", targets: ["Lynx"]),
        .library(name: "LynxBase", targets: ["LynxBase"]),
        .library(name: "LynxServiceAPI", targets: ["LynxServiceAPI"]),
        .library(name: "PrimJS", targets: ["PrimJS"]),
        // The module-system codegen build-tool plugin. Consolidated into
        // this package (rather than the separate `platforms/ios/macros`
        // package) so module manifests resolve BOTH `WhiskerRuntime` and
        // the plugin from a single remote SPM identity (`whisker`) — a
        // split across two packages would give the build graph two
        // identities to reconcile.
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            targets: ["WhiskerModuleCodegenPlugin"]
        ),
    ],
    dependencies: [
        // SwiftSyntax for the codegen executable. Only resolved/built
        // when a consumer actually uses the WhiskerModuleCodegenPlugin
        // (i.e. when building module SwiftPM targets).
        .package(url: "https://github.com/swiftlang/swift-syntax.git", from: "510.0.0"),
    ],
    targets: [
        // Lynx fork's xcframework zips, served off the `whiskerrs/lynx`
        // repo's Releases. Checksums come from the
        // `swiftpm-manifest-<ver>.txt` published alongside each release.
        .binaryTarget(
            name: "Lynx",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/Lynx-4.0.1-whisker.1.xcframework.zip",
            checksum: "e5161bf110a1d22869412689b89362798fd35e53dad9467980e9390918261c1d"
        ),
        .binaryTarget(
            name: "LynxBase",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/LynxBase-4.0.1-whisker.1.xcframework.zip",
            checksum: "875c389e2a33ad846ae43a29bcb1dad131d8b1e75ab36a2d41f39355e68cd25e"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/LynxServiceAPI-4.0.1-whisker.1.xcframework.zip",
            checksum: "9347769f21d9c41e0274cdb555698d7489f67ab9ed26fe2146688061f55a9b24"
        ),
        .binaryTarget(
            name: "PrimJS",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/PrimJS-4.0.1-whisker.1.xcframework.zip",
            checksum: "76502d535310b42d7c15d9dcb8e802622d17f8541bcee5a8dcf8863b59d8f45b"
        ),

        // Header-only mirror of `WhiskerDriver`'s public C ABI. Source
        // lives under `platforms/ios/Sources/` so both this root package
        // and the monorepo-local `platforms/ios/Package.swift` can point
        // their `path:` at it.
        .systemLibrary(
            name: "WhiskerCBridge",
            path: "platforms/ios/Sources/WhiskerCBridge/include"
        ),

        .target(
            name: "WhiskerModule",
            dependencies: ["Lynx", "WhiskerCBridge"],
            path: "platforms/ios/Sources/WhiskerModule"
        ),

        .target(
            name: "WhiskerRuntime",
            dependencies: [
                "WhiskerModule",
                "WhiskerCBridge",
                "Lynx",
                "LynxBase",
                "LynxServiceAPI",
                "PrimJS",
            ],
            path: "platforms/ios/Sources/WhiskerRuntime",
            linkerSettings: [
                // System frameworks Lynx pulls transitively. Mirrors the
                // monorepo-local Package.swift.
                .linkedFramework("JavaScriptCore"),
                .linkedFramework("NaturalLanguage"),
                .linkedLibrary("c++"),
            ]
        ),

        // ---- Module-system codegen (was platforms/ios/macros) --------
        // SwiftSyntax-driven codegen tool, invoked by the build-tool
        // plugin below. Scans a consuming module target's sources for
        // `Module` subclasses and emits `<Target>+Generated.swift` (the
        // iOS counterpart of Android's KSP-generated registration).
        .executableTarget(
            name: "WhiskerModuleCodegen",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftParser", package: "swift-syntax"),
            ],
            path: "platforms/ios/macros/Sources/WhiskerModuleCodegen"
        ),
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            capability: .buildTool(),
            dependencies: ["WhiskerModuleCodegen"],
            path: "platforms/ios/macros/Plugins/WhiskerModuleCodegenPlugin"
        ),
    ]
)
