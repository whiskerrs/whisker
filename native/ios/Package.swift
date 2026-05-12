// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "FlintRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "FlintRuntime", targets: ["FlintRuntime"]),
    ],
    targets: [
        .target(
            name: "FlintRuntime",
            dependencies: [],
            path: "Sources/FlintRuntime"
        ),
    ]
)
