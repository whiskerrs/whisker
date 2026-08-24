// swift-tools-version:5.9
//
// Remote-consumable iOS module API and Host runtime. Generated applications
// only compose WhiskerView; renderer implementation stays in this package.

import PackageDescription
import CompilerPluginSupport

let package = Package(
    name: "Whisker",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerModule", targets: ["WhiskerModule"]),
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            targets: ["WhiskerModuleCodegenPlugin"]
        ),
    ],
    dependencies: [
        .package(url: "https://github.com/swiftlang/swift-syntax.git", from: "510.0.0"),
    ],
    targets: [
        .systemLibrary(
            name: "WhiskerCBridge",
            path: "platforms/ios/Sources/WhiskerCBridge/include"
        ),
        .macro(
            name: "WhiskerModuleMacros",
            dependencies: [
                .product(name: "SwiftCompilerPlugin", package: "swift-syntax"),
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftSyntaxMacros", package: "swift-syntax"),
            ],
            path: "platforms/ios/macros/Sources/WhiskerModuleMacros"
        ),
        .target(
            name: "WhiskerModule",
            dependencies: ["WhiskerCBridge", "WhiskerModuleMacros"],
            path: "platforms/ios/Sources/WhiskerModule"
        ),
        .target(
            name: "WhiskerRuntime",
            dependencies: ["WhiskerModule"],
            path: "platforms/ios/Sources/WhiskerRuntime",
            swiftSettings: [.define("WHISKER_HOST_CONFORMANCE")]
        ),
        .target(
            name: "WhiskerHostConformanceStubs",
            path: "tests/host-conformance/runners/ios/Stubs"
        ),
        .testTarget(
            name: "WhiskerIOSHostConformanceTests",
            dependencies: [
                "WhiskerRuntime",
                "WhiskerModule",
                "WhiskerCBridge",
                "WhiskerHostConformanceStubs",
            ],
            path: "tests/host-conformance/runners/ios/Tests"
        ),
        .executableTarget(
            name: "WhiskerModuleCodegen",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftParser", package: "swift-syntax"),
            ],
            path: "platforms/ios/macros/Sources/WhiskerModuleCodegen"
        ),
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            capability: .buildTool(),
            dependencies: ["WhiskerModuleCodegen"],
            path: "platforms/ios/macros/Plugins/WhiskerModuleCodegenPlugin"
        ),
    ]
)
