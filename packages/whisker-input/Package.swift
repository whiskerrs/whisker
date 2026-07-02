// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-input` module package.
//
// Mirrors `whisker-safe-area`'s shape (WhiskerModule + WhiskerRuntime):
// one library target with sources under `ios/Sources/WhiskerInput`, the
// WhiskerModuleCodegenPlugin wired so `Module`-subclass registration
// lands in `<Target>+Generated.swift` at build time.
//
// `WhiskerRuntime` is pulled in (same as `whisker-safe-area`) because
// the view fires events via `WhiskerCustomEvent`, whose definition lives
// in `WhiskerLynxAliases.swift` inside the `WhiskerModule` product;
// the import chain also picks up `LynxContext` / `LynxCustomEvent` from
// the re-exported Lynx framework.
//
// `whisker-build` injects the absolute location of Whisker's iOS
// runtime + macros packages via env vars, so this module resolves them
// no matter where the crate lives — in the monorepo, in a user's
// whisker project, or unpacked from the cargo registry. No relative
// fallback: a Whisker module is only ever built through `whisker run`
// / `whisker build`, never standalone `swift build`.

import PackageDescription

// WhiskerRuntime + the WhiskerModuleCodegenPlugin resolve from the
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
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.1"),
    ],
    targets: [
        .target(
            name: "WhiskerInput",
            dependencies: [
                // WhiskerModule re-exports Lynx transitively (including
                // LynxCustomEvent + LynxEventEmitter), which is what
                // WhiskerCustomEvent.dispatch uses.
                .product(name: "WhiskerModule", package: "whisker"),
                // WhiskerRuntime pulls in WhiskerView and the
                // WhiskerDriver symbols — mirroring whisker-safe-area.
                .product(name: "WhiskerRuntime", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerInput",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
