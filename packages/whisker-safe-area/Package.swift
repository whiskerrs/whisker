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

guard let whiskerRuntimePath = Context.environment["WHISKER_IOS_RUNTIME"],
      let whiskerMacrosPath = Context.environment["WHISKER_IOS_MACROS"]
else {
    fatalError("""
        WHISKER_IOS_RUNTIME / WHISKER_IOS_MACROS not set. Build this Whisker \
        module through `whisker run` / `whisker build`, which inject these paths.
        """)
}

let package = Package(
    name: "whisker-safe-area",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSafeArea", targets: ["WhiskerSafeArea"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerSafeArea",
            dependencies: [
                // WhiskerRuntime re-exports both WhiskerModule (the
                // Module base + DSL) and WhiskerDriver (the
                // NotificationCenter name constant the safeAreaInsetsDidChange
                // hook posts under).
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
                .product(name: "WhiskerRuntime", package: "WhiskerRuntime"),
            ],
            path: "ios/Sources/WhiskerSafeArea",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
