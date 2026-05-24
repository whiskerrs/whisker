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
        .package(name: "whisker-ios-macros", path: "../whisker-ios-macros"),
        .package(name: "WhiskerRuntime", path: "../../native/ios"),
        // Phase 7-Φ.G PoC — an external SwiftPM dependency.
        // swift-collections is Apple-maintained, header-only Swift
        // (no Obj-C interop layer to negotiate), small enough that
        // resolving it is fast even on a cold cache. Use of the
        // library is intentionally minimal — see WhiskerLocalStoreImpl
        // for the import. The point of this declaration is to prove
        // module packages CAN pull external SPM URLs without any
        // Whisker-side build plumbing.
        .package(
            url: "https://github.com/apple/swift-collections.git",
            from: "1.1.0"
        ),
    ],
    targets: [
        .target(
            name: "WhiskerLocalStore",
            dependencies: [
                .product(name: "WhiskerComponents", package: "whisker-ios-macros"),
                .product(name: "WhiskerRuntime", package: "WhiskerRuntime"),
                .product(name: "OrderedCollections", package: "swift-collections"),
            ],
            path: "src/ios",
            plugins: [
                .plugin(name: "WhiskerComponentsCodegenPlugin", package: "whisker-ios-macros"),
            ]
        ),
    ]
)
