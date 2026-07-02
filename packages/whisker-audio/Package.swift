// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-audio` module package. Same
// shape as `whisker-video`'s manifest — the SwiftPM codegen plugin
// scans `ios/Sources/WhiskerAudio/` for `Module` subclasses and
// auto-registers them with Lynx.

import PackageDescription

// WhiskerRuntime + the WhiskerModuleCodegenPlugin resolve from the
// remote `whisker` SwiftPM package (the repo-root Package.swift,
// pinned by tag). No monorepo `platforms/ios` local path is required,
// so this module builds for an app created outside the whisker repo.
let package = Package(
    name: "whisker-audio",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerAudio", targets: ["WhiskerAudio"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.1"),
    ],
    targets: [
        .target(
            name: "WhiskerAudio",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerAudio",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
