// swift-tools-version:5.9
//
// Whisker SwiftPM package — published-from-main-branch entry point
// for iOS consumer apps. Mirrors the platforms/ios/Package.swift
// surface but with three differences that make it remote-consumable:
//
//   1. Lynx binaryTargets reference the fork's GitHub Release zips
//      via `url: + checksum:` instead of the local
//      `target/lynx-ios/` paths the monorepo flow uses.
//   2. WhiskerDriver is NOT declared as a binaryTarget — it can't
//      be pre-built and shipped because it contains user code. The
//      Swift sources `import WhiskerCBridge` instead of
//      `WhiskerDriver`; WhiskerCBridge is a header-only C target
//      whose symbols the user app's `WhiskerDriver.framework`
//      (built per-app by `whisker-build ios`) provides at link
//      time. Static-library compilation tolerates the undefined
//      references; the host app's dyld pulls them in at runtime.
//   3. Targets reference `platforms/ios/Sources/...` via
//      `path:`-relative paths so the Swift source files stay in one
//      place (no duplication between the local and published
//      flavours).
//
// `platforms/ios/Package.swift` stays in place for monorepo
// developers iterating on `WhiskerRuntime` / `WhiskerModule`
// sources before tagging — `XCLocalSwiftPackageReference` against
// that file resolves `WhiskerDriver` via the per-app
// `target/whisker-driver/` build output.

import PackageDescription

let package = Package(
    name: "Whisker",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "WhiskerModule", targets: ["WhiskerModule"]),
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
        // Lynx surface is exposed as products so module
        // `Package.swift` files can pull individual frameworks via
        // `.product(name: "Lynx", package: "whisker")`. Matches the
        // monorepo-local Package.swift convention.
        .library(name: "Lynx", targets: ["Lynx"]),
        .library(name: "LynxBase", targets: ["LynxBase"]),
        .library(name: "LynxServiceAPI", targets: ["LynxServiceAPI"]),
        .library(name: "PrimJS", targets: ["PrimJS"]),
    ],
    targets: [
        // Lynx fork's xcframework zips, served off the
        // `whiskerrs/lynx` repo's Releases. Checksums come from the
        // `swiftpm-manifest-<ver>.txt` published alongside.
        //
        // Bump points: when LYNX_FORK_TAG / LYNX_*_SHA256 in
        // `crates/whisker-build/src/lynx.rs` move, the URL version
        // segment + checksum here must move together.
        .binaryTarget(
            name: "Lynx",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.6/Lynx-3.8.0-whisker.6.xcframework.zip",
            checksum: "a467fceb0bd6b0318c80fcc93fe9b14e26f268dc6b2b9e06bf0365f50cb76fc5"
        ),
        .binaryTarget(
            name: "LynxBase",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.6/LynxBase-3.8.0-whisker.6.xcframework.zip",
            checksum: "309dd1e544a4cd035b71e1c786532e7344653c470d7206fbb28e1493b7f8e36e"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.6/LynxServiceAPI-3.8.0-whisker.6.xcframework.zip",
            checksum: "59bc9fcf07704d288de63b78ec1717fa81ade0af1cacea2f3712b57a220cb92f"
        ),
        .binaryTarget(
            name: "PrimJS",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.6/PrimJS-3.8.0-whisker.6.xcframework.zip",
            checksum: "a7069cd487834f96af28a335da049220b61317b0448768f040c171224f891651"
        ),

        // WhiskerCBridge — see
        // platforms/ios/Sources/WhiskerCBridge/include/module.modulemap
        // for why this is header-only. Replaces the `WhiskerDriver`
        // binaryTarget that the monorepo-local Package.swift carries.
        // Source lives under platforms/ios/Sources/ so the
        // monorepo-local Package.swift can `path: "Sources/WhiskerCBridge"`
        // it (SwiftPM rejects targets outside their package root, so
        // we can't share a swiftpm/ top-level dir between the two
        // packages).
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
                // System frameworks Lynx pulls transitively. Mirrors
                // the monorepo-local Package.swift.
                .linkedFramework("JavaScriptCore"),
                .linkedFramework("NaturalLanguage"),
                .linkedLibrary("c++"),
            ]
        ),
    ]
)
