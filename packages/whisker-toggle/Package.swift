// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "whisker-toggle",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerToggle", targets: ["WhiskerToggle"]),
    ],
    dependencies: [
        .package(name: "whisker", path: "../.."),
    ],
    targets: [
        .target(
            name: "WhiskerToggle",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerToggle",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
