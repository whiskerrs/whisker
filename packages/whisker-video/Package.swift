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
// WhiskerRuntime + the WhiskerModuleCodegenPlugin resolve from the
// remote `whisker` SwiftPM package (the repo-root Package.swift,
// pinned by tag). No monorepo `platforms/ios` local path is required,
// so this module builds for an app created outside the whisker repo.
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
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.2"),
    ],
    targets: [
        .target(
            name: "WhiskerVideo",
            dependencies: [
                // WhiskerModule re-exports Lynx transitively, so
                // no separate `Lynx` product dep is needed.
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            // Swift sources live under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerVideo",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
