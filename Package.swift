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
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.4/Lynx-3.8.0-whisker.4.xcframework.zip",
            checksum: "0e612e7a3edf9c628b7f750f24eed65162718cf84f30fcb693e1d6868f610bea"
        ),
        .binaryTarget(
            name: "LynxBase",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.4/LynxBase-3.8.0-whisker.4.xcframework.zip",
            checksum: "d65a856dd5caacc14b2c285af179c5bce5c80594cfa3a9e0bea077e3272f4fcd"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.4/LynxServiceAPI-3.8.0-whisker.4.xcframework.zip",
            checksum: "ab26dc419701a2c1ccc5578b8eb2b6281c6c727aa5d91f9ebb01f20a18a4c14c"
        ),
        .binaryTarget(
            name: "PrimJS",
            url: "https://github.com/whiskerrs/lynx/releases/download/v3.8.0-whisker.4/PrimJS-3.8.0-whisker.4.xcframework.zip",
            checksum: "4835ac846af4db8610a4a9d2517c8d5ba1c0744c67969740905b967cd6f8e5b6"
        ),

        // WhiskerCBridge — see swiftpm/Sources/WhiskerCBridge/include/module.modulemap
        // for why this is header-only. Replaces the `WhiskerDriver`
        // binaryTarget that the monorepo-local Package.swift carries.
        .target(
            name: "WhiskerCBridge",
            path: "swiftpm/Sources/WhiskerCBridge",
            publicHeadersPath: "include"
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
