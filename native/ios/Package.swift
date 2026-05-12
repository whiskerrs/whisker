// swift-tools-version:5.9
import PackageDescription

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
