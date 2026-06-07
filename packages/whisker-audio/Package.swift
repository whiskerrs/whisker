// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-audio` module package. Same
// shape as `whisker-video`'s manifest — the SwiftPM codegen plugin
// scans `ios/Sources/WhiskerAudio/` for `Module` subclasses and
// auto-registers them with Lynx.

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

import Foundation

let whiskerRuntimePath: String
let whiskerMacrosPath: String
if let r = Context.environment["WHISKER_IOS_RUNTIME"],
   let m = Context.environment["WHISKER_IOS_MACROS"] {
    whiskerRuntimePath = r
    whiskerMacrosPath = m
} else {
    // URL.standardized resolves `../` so SwiftPM sees the same
    // canonical absolute path the cng-rendered
    // `gen/ios/whisker_modules/Package.swift` uses. Otherwise the
    // build graph treats this and the aggregator as two instances
    // of the same package and the WhiskerModuleCodegenPlugin's
    // executable dep doesn't land in `Build/Products/Release/`.
    let raw = Context.packageDirectory + "/../../platforms/ios"
    whiskerRuntimePath = URL(fileURLWithPath: raw).standardized.path
    whiskerMacrosPath = URL(fileURLWithPath: whiskerRuntimePath + "/macros").standardized.path
}

let package = Package(
    name: "whisker-audio",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerAudio", targets: ["WhiskerAudio"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerAudio",
            dependencies: [
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
            ],
            path: "ios/Sources/WhiskerAudio",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
