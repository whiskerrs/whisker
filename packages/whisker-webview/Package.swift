// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-webview` module package.
//
// Mirrors `whisker-input`'s shape (WhiskerModule + WhiskerRuntime products,
// codegen plugin). One library target with sources under
// `ios/Sources/WhiskerWebview`, the WhiskerModuleCodegenPlugin wired so
// `Module`-subclass registration lands in `<Target>+Generated.swift` at
// build time.
//
// `WhiskerRuntime` is pulled in (same as `whisker-input`) because the view
// fires events via `WhiskerCustomEvent`, whose definition lives in
// `WhiskerLynxAliases.swift` inside the `WhiskerModule` product; the import
// chain also picks up `LynxContext` / `LynxCustomEvent` from the
// re-exported Lynx framework.
//
// `whisker-build` injects the absolute location of Whisker's iOS runtime +
// macros packages via env vars, so this module resolves them no matter where
// the crate lives — in the monorepo, in a user's whisker project, or
// unpacked from the cargo registry. No relative fallback: a Whisker module
// is only ever built through `whisker run` / `whisker build`, never
// standalone `swift build`.

import PackageDescription

let package = Package(
    name: "whisker-webview",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerWebview", targets: ["WhiskerWebview"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.1"),
    ],
    targets: [
        .target(
            name: "WhiskerWebview",
            dependencies: [
                // WhiskerModule re-exports Lynx transitively (including
                // LynxCustomEvent + LynxEventEmitter), which is what
                // WhiskerCustomEvent.dispatch uses.
                .product(name: "WhiskerModule", package: "whisker"),
                // WhiskerRuntime pulls in WhiskerView and the
                // WhiskerDriver symbols — mirroring whisker-input.
                .product(name: "WhiskerRuntime", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerWebview",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
