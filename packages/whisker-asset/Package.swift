// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-asset` module package.
//
// Same shape as `whisker-audio` / `whisker-image`: one library target
// with sources under `ios/Sources/WhiskerAsset`, and the
// `WhiskerModuleCodegenPlugin` wired so the `Module`-subclass
// discovery auto-registers `AssetModule` at app launch.
//
// whisker-asset's iOS half is **view-less and startup-only**: at
// registration time (`AssetModule.definition()` is read once at
// launch) it installs the runtime resolver base by calling the
// app-cdylib C export `whisker_asset_set_ios_base` with
// `file://<Bundle.main.bundlePath>/whisker_assets`. The bundle path is
// only known at runtime, which is exactly why this MUST come from
// native Swift rather than a Rust compile-time constant.

import PackageDescription

let package = Package(
    name: "whisker-asset",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerAsset", targets: ["WhiskerAsset"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.1"),
    ],
    targets: [
        .target(
            name: "WhiskerAsset",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerAsset",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
