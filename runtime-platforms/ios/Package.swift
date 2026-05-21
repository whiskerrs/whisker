// swift-tools-version:5.9

// WhiskerNativeRuntime — the platform-side runtime that Whisker
// native-module / native-element authors build against.
//
// Phase 7-A.4 skeleton only. Today's contents are stubs that exist
// so:
//   - The workspace's CI (or a manual `swift build`) catches package-
//     manifest drift early.
//   - Native module crates can declare a dependency on this package
//     before the real `@WhiskerModule` / `@WhiskerElement` Swift
//     Macro implementation lands in Phase 7-B.6 / 7-C.
//
// Real `WhiskerView<T>` base class, `@WhiskerModule` /
// `@WhiskerElement` Swift Macro plugins, and `WhiskerContext` /
// `WhiskerCallback` arrive with the matching Phase 7 subtasks.

import PackageDescription

let package = Package(
    name: "WhiskerNativeRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "WhiskerNativeRuntime", targets: ["WhiskerNativeRuntime"]),
    ],
    targets: [
        .target(
            name: "WhiskerNativeRuntime",
            path: "Sources/WhiskerNativeRuntime"
        ),
    ]
)
