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
        // Paths are relative to the package root.
        .package(name: "macros", path: "../../platforms/ios/macros"),
        .package(name: "WhiskerRuntime", path: "../../platforms/ios"),
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
