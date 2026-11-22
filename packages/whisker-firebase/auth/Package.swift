// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "whisker-firebase-auth",
    platforms: [.iOS(.v15), .macOS(.v13)],
    products: [.library(name: "WhiskerFirebaseAuth", targets: ["WhiskerFirebaseAuth"])],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.22"),
        .package(url: "https://github.com/firebase/firebase-ios-sdk.git", exact: "12.19.2"),
        // Any 8.x: SwiftPM picks the one firebase-ios-sdk pins.
        .package(url: "https://github.com/google/GoogleUtilities.git", from: "8.0.0"),
    ],
    targets: [
        .target(
            name: "WhiskerFirebaseAuth",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "FirebaseAuth", package: "firebase-ios-sdk"),
                .product(name: "GULAppDelegateSwizzler", package: "GoogleUtilities"),
            ],
            path: "ios/Sources/WhiskerFirebaseAuth",
            plugins: [.plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker")]
        ),
    ]
)
