// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "whisker-firebase-firestore",
    platforms: [.iOS(.v15), .macOS(.v13)],
    products: [.library(name: "WhiskerFirebaseFirestore", targets: ["WhiskerFirebaseFirestore"])],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.17"),
        .package(url: "https://github.com/firebase/firebase-ios-sdk.git", exact: "12.19.2"),
    ],
    targets: [
        .target(
            name: "WhiskerFirebaseFirestore",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "FirebaseFirestore", package: "firebase-ios-sdk"),
            ],
            path: "ios/Sources/WhiskerFirebaseFirestore",
            plugins: [.plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker")]
        ),
    ]
)
