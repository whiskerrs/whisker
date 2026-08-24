// swift-tools-version:5.9
//
// Local development mirror of the repository-root Whisker Swift package.

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
        .systemLibrary(name: "WhiskerCBridge", path: "Sources/WhiskerCBridge/include"),
        .macro(
            name: "WhiskerModuleMacros",
            dependencies: [
                .product(name: "SwiftCompilerPlugin", package: "swift-syntax"),
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftSyntaxMacros", package: "swift-syntax"),
            ],
            path: "macros/Sources/WhiskerModuleMacros"
        ),
        .target(
            name: "WhiskerModule",
            dependencies: ["WhiskerCBridge", "WhiskerModuleMacros"],
            path: "Sources/WhiskerModule"
        ),
        .target(
            name: "WhiskerRuntime",
            dependencies: ["WhiskerModule"],
            path: "Sources/WhiskerRuntime"
        ),
        .executableTarget(
            name: "WhiskerModuleCodegen",
            dependencies: [
                .product(name: "SwiftSyntax", package: "swift-syntax"),
                .product(name: "SwiftParser", package: "swift-syntax"),
            ],
            path: "macros/Sources/WhiskerModuleCodegen"
        ),
        .plugin(
            name: "WhiskerModuleCodegenPlugin",
            capability: .buildTool(),
            dependencies: ["WhiskerModuleCodegen"],
            path: "macros/Plugins/WhiskerModuleCodegenPlugin"
        ),
    ]
)
