// swift-tools-version:5.9
//
// SwiftPM manifest for the `whisker-safe-area` module package.
//
// Mirrors `whisker-image` / `whisker-video`'s shape: one library
// target with sources under `ios/Sources/WhiskerSafeArea`, the
// WhiskerModuleCodegenPlugin wired so the Module subclass
// registration lands in `<Target>+Generated.swift` at build time.
//
// `whisker-build` injects the absolute location of Whisker's iOS
// runtime + macros packages via these env vars, so this module
// resolves them no matter where the crate lives — in the monorepo,
// in a user's whisker project, or unpacked from the cargo registry.

import PackageDescription

// Resolve WhiskerRuntime + macros paths. Order of preference:
//   1. WHISKER_IOS_RUNTIME / WHISKER_IOS_MACROS env vars. The whisker
//      CLI's orchestrated flow (`whisker build` / `whisker run`) sets
//      these. Xcode-driven builds where the scheme sets them via env
//      vars also land here.
//   2. Monorepo fallback: this package lives at `packages/<crate>/`,
//      so `../../platforms/ios` resolves to WhiskerRuntime and
//      `../../platforms/ios/macros` to the macros package. This is
//      what makes Xcode-driven `xcodebuild` (Step 7) succeed without
//      a wrapping CLI invocation.
//
// Step-7 note: the cng-rendered `gen/ios/whisker_modules/Package.swift`
// reaches its module deps by absolute path (the cng renderer baked it
// in at sync time), so by the time SPM resolves THIS file, env vars
// from the CLI may or may not be in the inherited environment. The
// monorepo fallback lets the resolve succeed in either case.

let whiskerRuntimePath: String
let whiskerMacrosPath: String
if let r = Context.environment["WHISKER_IOS_RUNTIME"],
   let m = Context.environment["WHISKER_IOS_MACROS"] {
    whiskerRuntimePath = r
    whiskerMacrosPath = m
} else {
    whiskerRuntimePath = Context.packageDirectory + "/../../platforms/ios"
    whiskerMacrosPath = whiskerRuntimePath + "/macros"
}

let package = Package(
    name: "whisker-safe-area",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "WhiskerSafeArea", targets: ["WhiskerSafeArea"]),
    ],
    dependencies: [
        .package(name: "macros", path: whiskerMacrosPath),
        .package(name: "WhiskerRuntime", path: whiskerRuntimePath),
    ],
    targets: [
        .target(
            name: "WhiskerSafeArea",
            dependencies: [
                // WhiskerRuntime re-exports both WhiskerModule (the
                // Module base + DSL) and WhiskerDriver (the
                // NotificationCenter name constant the safeAreaInsetsDidChange
                // hook posts under).
                .product(name: "WhiskerModule", package: "WhiskerRuntime"),
                .product(name: "WhiskerRuntime", package: "WhiskerRuntime"),
            ],
            path: "ios/Sources/WhiskerSafeArea",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
