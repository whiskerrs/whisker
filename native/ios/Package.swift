// swift-tools-version:5.9
import PackageDescription

// Phase 2: pulls in Lynx + PrimJS + LynxBase + LynxServiceAPI on top of
// LyraMobile (Rust). Generate the binary frameworks first:
//
//   scripts/build-lynx-xcframeworks.sh
//   scripts/build-ios-xcframework.sh
//
// All four Lynx frameworks must be present at target/lynx-ios/ before
// the package will resolve.

let package = Package(
    name: "LyraRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "LyraRuntime", targets: ["LyraRuntime"]),
    ],
    targets: [
        // Rust runtime (C ABI).
        .binaryTarget(
            name: "LyraMobile",
            path: "../../target/lyra-mobile/LyraMobile.xcframework"
        ),

        // Lynx engine + dependencies. Built from upstream source pods via
        // scripts/build-lynx-xcframeworks.sh.
        .binaryTarget(
            name: "Lynx",
            path: "../../target/lynx-ios/Lynx.xcframework"
        ),
        .binaryTarget(
            name: "LynxBase",
            path: "../../target/lynx-ios/LynxBase.xcframework"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            path: "../../target/lynx-ios/LynxServiceAPI.xcframework"
        ),
        .binaryTarget(
            name: "PrimJS",
            path: "../../target/lynx-ios/PrimJS.xcframework"
        ),

        .target(
            name: "LyraRuntime",
            dependencies: [
                "LyraMobile",
                "Lynx",
                "LynxBase",
                "LynxServiceAPI",
                "PrimJS",
            ],
            path: "Sources/LyraRuntime",
            linkerSettings: [
                // System frameworks the Lynx pods declare as dependencies.
                .linkedFramework("JavaScriptCore"),
                .linkedFramework("NaturalLanguage"),
                .linkedLibrary("c++"),
                // -ObjC: Lynx ships many Obj-C categories (e.g.
                // `LynxTemplateRender (SetUp)`) whose methods are stripped
                // from a static framework unless the linker is told to
                // load every Obj-C class.
                .unsafeFlags(["-ObjC"]),
            ]
        ),
    ]
)
