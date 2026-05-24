// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-video` module package — Phase
// 7-Φ.H.2.6 sample.
//
// Mirrors the `whisker-hello-element` shape (per-package SwiftPM
// library that the user app's aggregator imports). Demonstrates
// adding `@WhiskerUIMethod`s to a `WhiskerUI<UIView>` subclass for
// imperative Rust-side dispatch via `ElementRef<T>`.

import PackageDescription

let package = Package(
    name: "whisker-video",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerVideo", targets: ["WhiskerVideo"]),
    ],
    dependencies: [
        .package(name: "macros", path: "../../platforms/ios/macros"),
        .package(name: "WhiskerRuntime", path: "../../platforms/ios"),
    ],
    targets: [
        .target(
            name: "WhiskerVideo",
            dependencies: [
                .product(name: "WhiskerComponents", package: "macros"),
                .product(name: "WhiskerRuntime", package: "WhiskerRuntime"),
                .product(name: "Lynx", package: "WhiskerRuntime"),
            ],
            path: "src/ios",
            plugins: [
                .plugin(name: "WhiskerComponentsCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
