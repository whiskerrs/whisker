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
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.9/Lynx-3.8.0-whisker.9.xcframework.zip",
            checksum: "98f29de0577960f52f13b895de7c36714afb947ba8719df35a7ce605f1262d13"
        ),
        .binaryTarget(
            name: "LynxBase",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.9/LynxBase-3.8.0-whisker.9.xcframework.zip",
            checksum: "44e22f9d39199fd6ebbd84f851e5186a6900f1953362f878c039a472d2b8e268"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.9/LynxServiceAPI-3.8.0-whisker.9.xcframework.zip",
            checksum: "f65968b15c40484d864ba776aac42f94c032fcdb62d113a488b8f29062581774"
        ),
        .binaryTarget(
            name: "PrimJS",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.9/PrimJS-3.8.0-whisker.9.xcframework.zip",
            checksum: "bd4000889b4337889e143056edf0a7a441d43fade5915b72d7701d7c9688bfa9"
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
