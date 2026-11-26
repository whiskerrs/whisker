// swift-tools-version:5.9

import PackageDescription

let package = Package(
    name: "whisker-surface",
    platforms: [.iOS(.v13), .macOS(.v14)],
    products: [
        .library(name: "WhiskerSurface", targets: ["WhiskerSurface"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.24"),
    ],
    targets: [
        .target(
            name: "WhiskerSurface",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerSurface",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
