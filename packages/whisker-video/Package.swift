// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-video` module package — Phase
// 7-Φ.H.2.6 sample.
//
// Mirrors the `whisker-hello-element` shape (per-package SwiftPM
// library that the user app's aggregator imports). Demonstrates
// adding `@WhiskerUIMethod`s to a `WhiskerUI<UIView>` subclass for
// imperative Rust-side dispatch via `ElementRef<T>`.

import PackageDescription

// whisker-build injects the absolute location of Whisker's iOS
// runtime + macros packages via these env vars (the same paths it
// writes into the generated aggregator Package.swift), so this module
// resolves them no matter where the crate lives — in the monorepo, in
// a user's whisker project, or unpacked from the cargo registry. No
// relative fallback: a Whisker module is only ever built through
// `whisker run` / `whisker build`, never standalone `swift build`.
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
    name: "whisker-video",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerVideo", targets: ["WhiskerVideo"]),
    ],
    dependencies: [
        // Package.swift lives at the package root — SwiftPM requires
        // it there, and the package identity (the crate's dir name)
        // is unique, so the app aggregator references it via
        // `.package(path: …)` without the `ios`-dir-name collision.
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerVideo",
            dependencies: [
                // WhiskerModule re-exports Lynx transitively, so
                // no separate `Lynx` product dep is needed.
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
            ],
            // Swift sources live under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerVideo",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
