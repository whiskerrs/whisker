// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-local-store` module package's
// iOS half (Phase 7-Φ.G). See `whisker-hello-element/Package.swift`
// for the architectural rationale.

import PackageDescription

let package = Package(
    name: "whisker-local-store",
    // macOS 13 is required because the SwiftPM build plugin
    // (`WhiskerElementsCodegenPlugin`) is hosted by SwiftSyntax,
    // which requires that floor at build time. The module's
    // runtime artefacts only need iOS 13.
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerLocalStore", targets: ["WhiskerLocalStore"]),
    ],
    dependencies: [
        .package(name: "whisker-ios-macros", path: "../whisker-ios-macros"),
        .package(name: "WhiskerRuntime", path: "../../native/ios"),
    ],
    targets: [
        .target(
            name: "WhiskerLocalStore",
            dependencies: [
                .product(name: "WhiskerElements", package: "whisker-ios-macros"),
                .product(name: "WhiskerRuntime", package: "WhiskerRuntime"),
            ],
            path: "src/ios",
            plugins: [
                .plugin(name: "WhiskerElementsCodegenPlugin", package: "whisker-ios-macros"),
            ]
        ),
    ]
)
