// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-audio` module package. Same
// shape as `whisker-video`'s manifest — the SwiftPM codegen plugin
// scans `ios/Sources/WhiskerAudio/` for `Module` subclasses and
// auto-registers them with Lynx.

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
    name: "whisker-audio",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerAudio", targets: ["WhiskerAudio"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerAudio",
            dependencies: [
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
            ],
            path: "ios/Sources/WhiskerAudio",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
