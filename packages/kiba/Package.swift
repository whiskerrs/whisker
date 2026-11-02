// swift-tools-version:5.9

import PackageDescription

let package = Package(
    name: "kiba",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "Kiba", targets: ["Kiba"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.17"),
    ],
    targets: [
        .target(
            name: "Kiba",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            path: "ios/Sources/Kiba",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
