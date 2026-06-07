// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-svg` module package.
//
// Mirrors `whisker-image` / `whisker-safe-area`'s shape: one
// library target (the Module + WhiskerSvgView + replayer) plus a
// test target that pins the binary display-list format against
// inline-encoded reference bytes — those reference bytes are
// snapshots of `packages/whisker-svg/tests/fixtures/*.bin`, so
// the Rust producer and the Swift replayer cannot drift without
// the test catching it.

import PackageDescription

// Resolve WhiskerRuntime + macros paths. Order of preference:
//   1. WHISKER_IOS_RUNTIME / WHISKER_IOS_MACROS env vars. The whisker
//      CLI's orchestrated flow (`whisker build` / `whisker run`) sets
//      these. Xcode-driven builds where the scheme sets them via env
//      vars also land here.
//   2. Monorepo fallback: this package lives at `packages/<crate>/`,
//      so `../../platforms/ios` resolves to WhiskerRuntime and
//      `../../platforms/ios/macros` to the macros package. This is
//      what makes Xcode-driven `xcodebuild` (Step 7) succeed without
//      a wrapping CLI invocation.
//
// Step-7 note: the cng-rendered `gen/ios/whisker_modules/Package.swift`
// reaches its module deps by absolute path (the cng renderer baked it
// in at sync time), so by the time SPM resolves THIS file, env vars
// from the CLI may or may not be in the inherited environment. The
// monorepo fallback lets the resolve succeed in either case.

let whiskerRuntimePath: String
let whiskerMacrosPath: String
if let r = Context.environment["WHISKER_IOS_RUNTIME"],
   let m = Context.environment["WHISKER_IOS_MACROS"] {
    whiskerRuntimePath = r
    whiskerMacrosPath = m
} else {
    whiskerRuntimePath = Context.packageDirectory + "/../../platforms/ios"
    whiskerMacrosPath = whiskerRuntimePath + "/macros"
}

let package = Package(
    name: "whisker-svg",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSvg", targets: ["WhiskerSvg"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerSvg",
            dependencies: [
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
            ],
            path: "ios/Sources/WhiskerSvg",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
        .testTarget(
            name: "WhiskerSvgTests",
            dependencies: ["WhiskerSvg"],
            path: "ios/Tests/WhiskerSvgTests"
        ),
    ]
)
