// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-safe-area` module package.
//
// Mirrors `whisker-image` / `whisker-video`'s shape: one library
// target with sources under `ios/Sources/WhiskerSafeArea`, the
// WhiskerModuleCodegenPlugin wired so the Module subclass
// registration lands in `<Target>+Generated.swift` at build time.
//
// `whisker-build` injects the absolute location of Whisker's iOS
// runtime + macros packages via these env vars, so this module
// resolves them no matter where the crate lives — in the monorepo,
// in a user's whisker project, or unpacked from the cargo registry.

import PackageDescription

// WhiskerRuntime + the WhiskerModuleCodegenPlugin resolve from the
// remote `whisker` SwiftPM package (the repo-root Package.swift,
// pinned by tag). No monorepo `platforms/ios` local path is required,
// so this module builds for an app created outside the whisker repo.
let package = Package(
    name: "whisker-safe-area",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSafeArea", targets: ["WhiskerSafeArea"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.0"),
    ],
    targets: [
        .target(
            name: "WhiskerSafeArea",
            dependencies: [
                // WhiskerRuntime re-exports both WhiskerModule (the
                // Module base + DSL) and WhiskerDriver (the
                // NotificationCenter name constant the safeAreaInsetsDidChange
                // hook posts under).
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "WhiskerRuntime", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerSafeArea",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
