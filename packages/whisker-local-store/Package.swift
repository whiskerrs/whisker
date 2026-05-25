// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-local-store` module package's
// iOS half (Phase 7-Φ.G). See `whisker-hello-element/Package.swift`
// for the architectural rationale.

import PackageDescription

let package = Package(
    name: "whisker-local-store",
    // macOS 13 is required because the SwiftPM build plugin
    // (`WhiskerComponentsCodegenPlugin`) is hosted by SwiftSyntax,
    // which requires that floor at build time. The module's
    // runtime artefacts only need iOS 13.
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerLocalStore", targets: ["WhiskerLocalStore"]),
    ],
    dependencies: [
        // Paths relative to this `ios/` directory (one level below
        // the package root).
        .package(name: "macros", path: "../../platforms/ios/macros"),
        .package(name: "WhiskerRuntime", path: "../../platforms/ios"),
        // PoC — an external SwiftPM dependency. swift-collections is
        // Apple-maintained, header-only Swift, small enough that
        // resolving it is fast even on a cold cache. Use is minimal
        // — the point is to prove module packages CAN pull external
        // SPM URLs without Whisker-side build plumbing.
        .package(
            url: "https://github.com/apple/swift-collections.git",
            from: "1.1.0"
        ),
    ],
    targets: [
        .target(
            name: "WhiskerLocalStore",
            dependencies: [
                .product(name: "WhiskerComponents", package: "macros"),
                .product(name: "WhiskerModuleApi", package: "WhiskerRuntime"),
                .product(name: "OrderedCollections", package: "swift-collections"),
            ],
            // Swift sources under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerLocalStore",
            plugins: [
                .plugin(name: "WhiskerComponentsCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
