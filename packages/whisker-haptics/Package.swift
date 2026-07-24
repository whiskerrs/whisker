// swift-tools-version:5.9
//
// SwiftPM manifest for the local `whisker-haptics` module's iOS
// half.
//
// No external SwiftPM dependency: `UIImpactFeedbackGenerator` /
// `UISelectionFeedbackGenerator` / `UINotificationFeedbackGenerator`
// live in the system `UIKit` framework (auto-linked).

import PackageDescription

let package = Package(
    name: "whisker-haptics",
    // macOS 13 is required because the SwiftPM build plugin
    // (`WhiskerModuleCodegenPlugin`) is hosted by SwiftSyntax, which
    // requires that floor at build time. Runtime artefacts only need
    // iOS 13.
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerHaptics", targets: ["WhiskerHaptics"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.3"),
    ],
    targets: [
        .target(
            name: "WhiskerHaptics",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            // Swift sources under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerHaptics",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
