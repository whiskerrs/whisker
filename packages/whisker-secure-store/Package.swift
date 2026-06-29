// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-secure-store` module package's iOS
// half. See `whisker-local-store/Package.swift` for the architectural
// rationale (env-injected whisker package path, codegen plugin, etc.).
//
// No external SwiftPM dependency: the Keychain APIs live in the system
// `Security` framework, which `import Security` auto-links on Apple
// platforms.

import PackageDescription

let package = Package(
    name: "whisker-secure-store",
    // macOS 13 is required because the SwiftPM build plugin
    // (`WhiskerModuleCodegenPlugin`) is hosted by SwiftSyntax, which
    // requires that floor at build time. Runtime artefacts only need
    // iOS 13.
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSecureStore", targets: ["WhiskerSecureStore"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.0"),
    ],
    targets: [
        .target(
            name: "WhiskerSecureStore",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
            ],
            // Swift sources under the package's `ios/` directory
            // (Expo-style layout), next to `android/` and `src/`.
            path: "ios/Sources/WhiskerSecureStore",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
