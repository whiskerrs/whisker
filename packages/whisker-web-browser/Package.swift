// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "whisker-web-browser",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerWebBrowser", targets: ["WhiskerWebBrowser"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.3"),
    ],
    targets: [
        .target(
            name: "WhiskerWebBrowser",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "WhiskerRuntime", package: "whisker"),
            ],
            path: "ios/Sources/WhiskerWebBrowser",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
