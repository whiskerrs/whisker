//! Generation of the iOS SwiftPM module aggregator.
//!
//! This is part of CNG output: after `gen/ios` has been synchronized, Xcode
//! can resolve and compile every native module without a preceding
//! `whisker run`/`whisker build` staging step.

use std::path::Path;

use anyhow::{Context, Result};

use crate::modules::{ModulePlatform, NativeManifestKind, ResolvedModule};

/// Canonical remote Swift package containing Whisker's iOS Host SDK.
pub const WHISKER_IOS_SPM_URL: &str = "https://github.com/whiskerrs/whisker.git";
/// Exact Swift package version used by generated consumer projects.
pub const WHISKER_IOS_SPM_VERSION: &str = "0.1.13";

/// Generate the iOS module-aggregator SwiftPM package under
/// `gen/ios/whisker_modules/`. Module sources stay in their own
/// package directories — each module ships a hand-written
/// `Package.swift` — and the aggregator depends on them via
/// `.package(path: …)`. The Host SDK itself is the released Whisker
/// package pinned by [`WHISKER_IOS_SPM_VERSION`].
///
/// Mirror of Android's module-aggregator generation for iOS. The Android path
/// generates `settings.gradle.kts` includes;
/// the iOS equivalent produces a tiny SwiftPM package the user
/// app declares as a local Swift Package Dependency.
///
/// Layout produced (within `gen/ios/whisker_modules/`):
///
/// ```text
/// whisker_modules/
/// ├── Package.swift                       ← generated (aggregator)
/// └── Sources/WhiskerModules/
///     └── RegisterAll.swift               ← registers SDK built-ins + modules
/// ```
///
/// `Package.swift` declares one product (`WhiskerModules`) depending
/// on the released `WhiskerRuntime` product + each discovered module's
/// local-path SwiftPM package. The user app's pbxproj references only
/// `gen/ios/whisker_modules` as an `XCLocalSwiftPackageReference`;
/// SwiftPM resolves the SDK and module graph transitively.
///
/// `RegisterAll.swift` imports every module's SwiftPM library and
/// exposes the `@objc WhiskerModuleBehaviors.registerAll()` entry
/// point the AppDelegate calls at launch. The actual registration
/// work happens inside the per-module
/// `_whiskerRegisterModules_<TargetName>()` fns that the
/// `WhiskerModuleCodegenPlugin` emits into each module target.
///
/// Empty / non-Swift-contributing module list still writes a
/// no-op aggregator so the pbxproj reference always resolves
/// and `AppDelegate.swift` compiles.
pub fn stage_module_swift_sources(
    gen_ios: &Path,
    modules: &[ResolvedModule],
    workspace_root: &Path,
) -> Result<()> {
    let root = gen_ios.join("whisker_modules");
    let sources_root = root.join("Sources/WhiskerModules");

    // Wipe the previous tree so a removed-or-renamed module doesn't
    // leave behind a stale Package.swift / RegisterAll.swift entry.
    if root.exists() {
        std::fs::remove_dir_all(&root).with_context(|| format!("rm -rf {}", root.display()))?;
    }
    std::fs::create_dir_all(&sources_root)
        .with_context(|| format!("mkdir -p {}", sources_root.display()))?;

    // The module manifest is authoritative; unsupported and common-only iOS
    // implementations do not enter SwiftPM's graph.
    let ios_modules: Vec<&ResolvedModule> = modules
        .iter()
        .filter(|m| {
            m.native_manifest(ModulePlatform::Ios)
                .is_some_and(|manifest| manifest.kind == NativeManifestKind::SwiftPm)
        })
        .collect();

    // A Whisker source checkout contains the root Swift package that owns
    // WhiskerModule, WhiskerRuntime, and the codegen plugin. Prefer it while
    // developing the framework itself so the generated app tests the current
    // Host sources instead of the last published SDK tag. Consumer workspaces
    // do not have this layout and continue to resolve the released package.
    let local_whisker_package = workspace_root
        .join("platforms/ios/Sources/WhiskerRuntime")
        .is_dir()
        .then_some(workspace_root);

    let package_path = root.join("Package.swift");
    std::fs::write(
        &package_path,
        render_modules_package_swift(local_whisker_package, &ios_modules),
    )
    .with_context(|| format!("write {}", package_path.display()))?;

    let register_all_path = sources_root.join("RegisterAll.swift");
    std::fs::write(&register_all_path, render_register_all_swift(&ios_modules))
        .with_context(|| format!("write {}", register_all_path.display()))?;
    Ok(())
}

/// Convention: SwiftPM library product / target name is the
/// `PascalCase`-ised cargo crate name. So `whisker-local-store` →
/// `WhiskerLocalStore`. Module authors MUST follow this convention
/// in their hand-written `Package.swift` for the aggregator's
/// `.product(name:, package:)` lookups to resolve.
///
/// Deterministic + reversible — same input always yields same
/// output, no separator chars beyond `-` are touched.
fn crate_to_spm_target(crate_name: &str) -> String {
    let mut out = String::new();
    let mut next_upper = true;
    for ch in crate_name.chars() {
        if ch == '-' || ch == '_' {
            next_upper = true;
            continue;
        }
        if next_upper {
            out.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// iOS floor for the aggregator when no module asks for more.
const DEFAULT_IOS_PLATFORM_MAJOR: u32 = 13;

/// `.iOS(.v15)` → `15`, read from the `platforms:` list. `None` for a
/// manifest that declares no iOS floor or spells it some other way
/// (e.g. the `.iOS("16.4")` string form) — such a module just doesn't
/// raise the aggregator's floor.
#[doc(hidden)]
pub fn parse_ios_platform_major(manifest: &str) -> Option<u32> {
    const NEEDLE: &str = ".iOS(.v";
    let rest = &manifest[manifest.find(NEEDLE)? + NEEDLE.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The aggregator's iOS floor: the highest any module package declares.
///
/// SwiftPM refuses to resolve a dependency whose minimum platform is
/// higher than its consumer's, so pinning one version here caps what a
/// module may require — `whisker-revenuecat` needs iOS 15 (RevenueCatUI's
/// Customer Center) and could never resolve against a hardcoded `.v13`.
/// Reading it back off the modules keeps that a module-local decision.
fn ios_platform_major(modules: &[&ResolvedModule]) -> u32 {
    modules
        .iter()
        .filter_map(|m| m.native_manifest(ModulePlatform::Ios))
        .filter_map(|manifest| std::fs::read_to_string(&manifest.path).ok())
        .filter_map(|manifest| parse_ios_platform_major(&manifest))
        .chain(std::iter::once(DEFAULT_IOS_PLATFORM_MAJOR))
        .max()
        .unwrap_or(DEFAULT_IOS_PLATFORM_MAJOR)
}

/// Render `Package.swift` for the generated `WhiskerModules`
/// aggregator. Depends on the current checkout's `WhiskerRuntime` package
/// while developing Whisker, or the released package from consumer
/// workspaces, plus each discovered module package via local-path SwiftPM
/// dependency.
#[doc(hidden)]
pub fn render_modules_package_swift(
    local_whisker_package: Option<&Path>,
    modules: &[&ResolvedModule],
) -> String {
    let mut out = String::new();
    out.push_str(
        "// swift-tools-version:5.9\n\
         //\n\
         // AUTO-GENERATED by whisker-cng. Do NOT edit — re-run\n\
         // `whisker run` to refresh.\n\
         //\n\
         // Phase 7-Φ.G aggregator. The Whisker Host SDK is pinned to\n\
         // its released source package; each Whisker module ships its\n\
         // own SwiftPM package and is a local-path dependency.\n\
         // SwiftPM resolves the transitive build graph; the user\n\
         // app's pbxproj only references THIS aggregator package\n\
         // via `XCLocalSwiftPackageReference`.\n\
         //\n\
         // RegisterAll.swift (next to this file) imports each\n\
         // module and calls its per-target register fn from a\n\
         // top-level `WhiskerModuleBehaviors.registerAll()`.\n\n",
    );
    out.push_str("import PackageDescription\n\n");
    out.push_str("let package = Package(\n");
    out.push_str("    name: \"WhiskerModules\",\n");
    out.push_str(&format!(
        "    platforms: [.iOS(.v{})],\n",
        ios_platform_major(modules)
    ));
    out.push_str("    products: [\n");
    out.push_str("        .library(name: \"WhiskerModules\", targets: [\"WhiskerModules\"]),\n");
    out.push_str("    ],\n");
    out.push_str("    dependencies: [\n");
    if let Some(path) = local_whisker_package {
        out.push_str(&format!(
            "        .package(name: \"whisker\", path: {path:?}),\n",
            path = path.display().to_string(),
        ));
    } else {
        out.push_str(&format!(
            "        .package(url: {WHISKER_IOS_SPM_URL:?}, exact: {WHISKER_IOS_SPM_VERSION:?}),\n",
        ));
    }
    for m in modules {
        // The module's SwiftPM package is rooted at the package
        // directory (Package.swift lives there, identity = the
        // crate's dir name — unique). Its target sources live under
        // the package's `ios/` subdir (Expo-style layout).
        let path = m
            .native_manifest(ModulePlatform::Ios)
            .and_then(|manifest| manifest.path.parent())
            .expect("iOS modules were filtered to SwiftPM manifests")
            .display()
            .to_string();
        out.push_str(&format!(
            "        .package(name: {pkg:?}, path: {path:?}),\n",
            pkg = m.package
        ));
    }
    out.push_str("    ],\n");
    out.push_str("    targets: [\n");
    out.push_str("        .target(\n");
    out.push_str("            name: \"WhiskerModules\",\n");
    out.push_str("            dependencies: [\n");
    out.push_str("                .product(name: \"WhiskerModule\", package: \"whisker\"),\n");
    out.push_str("                .product(name: \"WhiskerRuntime\", package: \"whisker\"),\n");
    for m in modules {
        let target = crate_to_spm_target(&m.package);
        out.push_str(&format!(
            "                .product(name: {target:?}, package: {pkg:?}),\n",
            pkg = m.package
        ));
    }
    out.push_str("            ],\n");
    out.push_str("            path: \"Sources/WhiskerModules\"\n");
    out.push_str("        ),\n");
    out.push_str("    ]\n");
    out.push_str(")\n");
    out
}

/// Render `RegisterAll.swift` for the aggregator. Imports every
/// module's SwiftPM library and exposes the top-level
/// `WhiskerModuleBehaviors.registerAll()` entry point the
/// AppDelegate calls. Per-target work happens inside each
/// module's plugin-emitted `_whiskerRegisterModules_<TargetName>()`.
#[doc(hidden)]
pub fn render_register_all_swift(modules: &[&ResolvedModule]) -> String {
    let mut out = String::new();
    out.push_str(
        "// AUTO-GENERATED by whisker-cng. Do NOT edit — re-run\n\
         // `whisker run` to refresh.\n\
         //\n\
         // Aggregates every Whisker module's per-target register fn\n\
         // (emitted by the `WhiskerModuleCodegenPlugin` SwiftPM\n\
         // build-tool plugin into each module's compilation) into a\n\
         // single `WhiskerModuleBehaviors.registerAll()` entry point.\n\
         // The user app's AppDelegate calls this once at launch —\n\
         // the actual per-module registration work runs inside each\n\
         // `_whiskerRegisterModules_<TargetName>()`.\n\n",
    );
    out.push_str("import Foundation\n");
    out.push_str("@_exported import WhiskerModule\n");
    out.push_str("@_exported import WhiskerRuntime\n");
    for m in modules {
        let target = crate_to_spm_target(&m.package);
        out.push_str(&format!("import {target}\n"));
    }
    out.push('\n');
    out.push_str("@objc public final class WhiskerModuleBehaviors: NSObject {\n");
    out.push_str("    private static var registered = false\n");
    out.push_str("    private static let lock = NSLock()\n");
    out.push('\n');
    out.push_str("    @objc public static func registerAll() {\n");
    out.push_str("        lock.lock()\n");
    out.push_str("        defer { lock.unlock() }\n");
    out.push_str("        if registered { return }\n");
    out.push_str("        registered = true\n");
    out.push_str("        let builtInModule = BuiltInElementModule()\n");
    out.push_str("        builtInModule.qualifiedName = builtInModule.definitionLazy.name\n");
    out.push_str("        WhiskerModuleKernel.install(builtInModule)\n");
    if modules.is_empty() {
        out.push_str("        // (no Whisker module dependencies)\n");
    }
    for m in modules {
        let target = crate_to_spm_target(&m.package);
        out.push_str(&format!("        _whiskerRegisterModules_{target}()\n"));
    }
    out.push_str("    }\n}\n");
    out
}
