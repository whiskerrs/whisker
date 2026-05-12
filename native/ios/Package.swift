// swift-tools-version:5.9
import PackageDescription

// Phase 1: pulls in the LyraMobile xcframework (Rust static lib + C ABI).
// Generate it with `scripts/build-ios-xcframework.sh` before opening the
// example project in Xcode.

let package = Package(
    name: "LyraRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "LyraRuntime", targets: ["LyraRuntime"]),
    ],
    targets: [
        .binaryTarget(
            name: "LyraMobile",
            // Local path during development. Will switch to a release URL
            // (`url:` + `checksum:`) once we cut binary releases.
            path: "../../target/lyra-mobile/LyraMobile.xcframework"
        ),
        .target(
            name: "LyraRuntime",
            dependencies: ["LyraMobile"],
            path: "Sources/LyraRuntime"
        ),
    ]
)
