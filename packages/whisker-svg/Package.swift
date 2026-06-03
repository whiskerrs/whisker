// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-svg` module package.
//
// Mirrors `whisker-image` / `whisker-safe-area`'s shape: one
// library target (the Module + WhiskerSvgView + replayer) plus a
// test target that pins the binary display-list format against
// inline-encoded reference bytes — those reference bytes are
// snapshots of `crates/whisker-svg-core/tests/fixtures/*.bin`, so
// the Rust producer and the Swift replayer cannot drift without
// the test catching it.

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
    name: "whisker-svg",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSvg", targets: ["WhiskerSvg"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerSvg",
            dependencies: [
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
            ],
            path: "ios/Sources/WhiskerSvg",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
        .testTarget(
            name: "WhiskerSvgTests",
            dependencies: ["WhiskerSvg"],
            path: "ios/Tests/WhiskerSvgTests"
        ),
    ]
)
