// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-keyboard` module package.
//
// Mirrors `whisker-safe-area`'s shape: one library target with sources
// under `ios/Sources/WhiskerKeyboard`, the WhiskerModuleCodegenPlugin
// wired so the Module subclass registration lands in
// `<Target>+Generated.swift` at build time.

import PackageDescription

let package = Package(
    name: "whisker-keyboard",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerKeyboard", targets: ["WhiskerKeyboard"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.2"),
    ],
    targets: [
        .target(
            name: "WhiskerKeyboard",
            dependencies: [
                // WhiskerModule provides the Module base + DSL;
                // WhiskerRuntime is pulled for parity with the other
                // module packages (and to keep the codegen plugin's
                // generated registration compiling against the same
                // runtime symbols).
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "WhiskerRuntime", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerKeyboard",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
