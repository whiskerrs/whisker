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
// a user's whisker project, or unpacked from the cargo registry. The
// relative-path fallback applies only when building this package
// standalone (e.g. opening it directly in Xcode from the monorepo).
let whiskerRuntimePath = Context.environment["WHISKER_IOS_RUNTIME"] ?? "../../platforms/ios"
let whiskerMacrosPath = Context.environment["WHISKER_IOS_MACROS"] ?? "../../platforms/ios/macros"

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
                .product(name: "WhiskerComponents", package: "macros"),
                // WhiskerModuleApi re-exports Lynx transitively, so
                // no separate `Lynx` product dep is needed.
                .product(name: "WhiskerModuleApi", package: "WhiskerRuntime"),
            ],
            // Swift sources live under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerVideo",
            plugins: [
                .plugin(name: "WhiskerComponentsCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
