// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-local-store` module package's
// iOS half (Phase 7-Φ.G). See `whisker-hello-element/Package.swift`
// for the architectural rationale.

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
    name: "whisker-local-store",
    // macOS 13 is required because the SwiftPM build plugin
    // (`WhiskerModuleCodegenPlugin`) is hosted by SwiftSyntax,
    // which requires that floor at build time. The module's
    // runtime artefacts only need iOS 13.
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerLocalStore", targets: ["WhiskerLocalStore"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.3"),
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
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "OrderedCollections", package: "swift-collections"),
            ],
            // Swift sources under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerLocalStore",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
