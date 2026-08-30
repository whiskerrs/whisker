// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-webview` module package.
//
// Mirrors `whisker-input`'s shape (WhiskerModule product,
// codegen plugin). One library target with sources under
// `ios/Sources/WhiskerWebview`, the WhiskerModuleCodegenPlugin wired so
// `Module`-subclass registration lands in `<Target>+Generated.swift` at
// build time.
//
// `whisker-build` injects the absolute location of Whisker's iOS runtime +
// macros packages via env vars, so this module resolves them no matter where
// the crate lives — in the monorepo, in a user's whisker project, or
// unpacked from the cargo registry. No relative fallback: a Whisker module
// is only ever built through `whisker run` / `whisker build`, never
// standalone `swift build`.

import PackageDescription

let package = Package(
    name: "whisker-webview",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerWebview", targets: ["WhiskerWebview"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.12"),
    ],
    targets: [
        .target(
            name: "WhiskerWebview",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerWebview",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
