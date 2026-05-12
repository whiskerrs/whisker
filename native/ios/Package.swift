// swift-tools-version:5.9
import PackageDescription

// Phase 0: pure Swift target, no binary frameworks yet.
//
// In Phase 1 we add LyraRustRuntime as a binaryTarget (Rust cdylib wrapped
// in xcframework). In Phase 2 we add Lynx and LyraBridge as binaryTargets.
// All of those become dependencies of the LyraRuntime target.

let package = Package(
    name: "LyraRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "LyraRuntime", targets: ["LyraRuntime"]),
    ],
    targets: [
        .target(
            name: "LyraRuntime",
            dependencies: [],
            path: "Sources/LyraRuntime"
        ),
    ]
)
