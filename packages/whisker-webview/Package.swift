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
// The generated Xcode project links this package directly. Its checked-in
// manifest and sources remain ordinary SwiftPM inputs; Whisker CNG only
// assembles the app's package dependency graph.

import PackageDescription

let package = Package(
    name: "whisker-webview",
    platforms: [.iOS(.v13)],
    products: [
        .library(name: "WhiskerWebview", targets: ["WhiskerWebview"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.13"),
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
