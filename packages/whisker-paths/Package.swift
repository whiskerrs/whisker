// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-paths` module package's iOS half.
// See `whisker-secure-store/Package.swift` for the architectural
// rationale (env-injected whisker package path, codegen plugin, etc.).
//
// No external SwiftPM dependency: the directory APIs live in
// `Foundation`, auto-linked on Apple platforms.

import PackageDescription

let package = Package(
    name: "whisker-paths",
    // macOS 13 is required because the SwiftPM build plugin
    // (`WhiskerModuleCodegenPlugin`) is hosted by SwiftSyntax, which
    // requires that floor at build time. Runtime artefacts only need
    // iOS 13.
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerPaths", targets: ["WhiskerPaths"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.3"),
    ],
    targets: [
        .target(
            name: "WhiskerPaths",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            // Swift sources under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerPaths",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
