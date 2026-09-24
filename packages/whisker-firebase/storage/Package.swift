// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "whisker-firebase-storage",
    platforms: [.iOS(.v15), .macOS(.v13)],
    products: [.library(name: "WhiskerFirebaseStorage", targets: ["WhiskerFirebaseStorage"])],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "0.1.17"),
        .package(url: "https://github.com/firebase/firebase-ios-sdk.git", exact: "12.19.2"),
    ],
    targets: [
        .target(
            name: "WhiskerFirebaseStorage",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "FirebaseStorage", package: "firebase-ios-sdk"),
            ],
            path: "ios/Sources/WhiskerFirebaseStorage",
            plugins: [.plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker")]
        ),
    ]
)
