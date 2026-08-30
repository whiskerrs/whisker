// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-input` module package.
//
// Mirrors `whisker-safe-area`'s shape (WhiskerModule + codegen plugin):
// one library target with sources under `ios/Sources/WhiskerInput`, the
// WhiskerModuleCodegenPlugin wired so `Module`-subclass registration
// lands in `<Target>+Generated.swift` at build time.
//
// `whisker-build` injects the absolute location of Whisker's iOS
// runtime + macros packages via env vars, so this module resolves them
// no matter where the crate lives — in the monorepo, in a user's
// whisker project, or unpacked from the cargo registry. No relative
// fallback: a Whisker module is only ever built through `whisker run`
// / `whisker build`, never standalone `swift build`.

import PackageDescription

// WhiskerModule + the WhiskerModuleCodegenPlugin resolve from the
// remote `whisker` SwiftPM package (the repo-root Package.swift,
// pinned by tag). No monorepo `platforms/ios` local path is required,
// so this module builds for an app created outside the whisker repo.
let package = Package(
    name: "whisker-input",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerInput", targets: ["WhiskerInput"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.12"),
    ],
    targets: [
        .target(
            name: "WhiskerInput",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerInput",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
