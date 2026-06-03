// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-image` module package.
//
// Mirrors `whisker-video`'s shape: one library target with sources
// under `ios/Sources/WhiskerImage`, the WhiskerModuleCodegenPlugin
// wired so `@WhiskerModule`-driven Kotlin/Swift registration lands
// in `<Target>+Generated.swift` at build time, and a Kingfisher SPM
// dep that the `ImageView` Swift class uses to fetch URLs.
//
// `whisker-build` injects the absolute location of Whisker's iOS
// runtime + macros packages via these env vars (the same paths it
// writes into the generated aggregator Package.swift), so this
// module resolves them no matter where the crate lives — in the
// monorepo, in a user's whisker project, or unpacked from the cargo
// registry. No relative fallback: a Whisker module is only ever
// built through `whisker run` / `whisker build`, never standalone
// `swift build`.

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
    name: "whisker-image",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerImage", targets: ["WhiskerImage"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
        // Kingfisher 7.x — pure Swift image loader. PNG / JPEG / HEIC
        // via Core Image, animated GIF via `AnimatedImageView`, in-
        // memory `NSCache` + disk cache out of the box. WebP requires
        // the separate KingfisherWebP package (opt-in); not pulled in
        // here so the base module stays slim.
        .package(url: "https://github.com/onevcat/Kingfisher.git", from: "7.12.0"),
    ],
    targets: [
        .target(
            name: "WhiskerImage",
            dependencies: [
                // WhiskerModule re-exports Lynx transitively, so a
                // separate `Lynx` product dep isn't needed.
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
                .product(name: "Kingfisher", package: "Kingfisher"),
            ],
            path: "ios/Sources/WhiskerImage",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
