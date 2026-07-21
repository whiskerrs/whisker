// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-svg` module package.
//
// Mirrors `whisker-image` / `whisker-safe-area`'s shape: one
// library target (the Module + WhiskerSvgView + replayer) plus a
// test target that pins the binary display-list format against
// inline-encoded reference bytes — those reference bytes are
// snapshots of `packages/whisker-svg/tests/fixtures/*.bin`, so
// the Rust producer and the Swift replayer cannot drift without
// the test catching it.

import PackageDescription

// WhiskerRuntime + the WhiskerModuleCodegenPlugin resolve from the
// remote `whisker` SwiftPM package (the repo-root Package.swift,
// pinned by tag). No monorepo `platforms/ios` local path is required,
// so this module builds for an app created outside the whisker repo.
let package = Package(
    name: "whisker-svg",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSvg", targets: ["WhiskerSvg"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.3"),
    ],
    targets: [
        .target(
            name: "WhiskerSvg",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerSvg",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
        .testTarget(
            name: "WhiskerSvgTests",
            dependencies: ["WhiskerSvg"],
            path: "ios/Tests/WhiskerSvgTests"
        ),
    ]
)
