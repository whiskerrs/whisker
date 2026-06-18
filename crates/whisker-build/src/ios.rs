//! iOS cargo + xcframework + xcodebuild orchestration. Shared by
//! the `whisker-build` binary (xcodebuild Build Phase) and `whisker-dev-server`'s install
//! step.
//!
//! Two entry points:
//!
//! 1. [`build_framework_for_xcode_run_script`] — the path xcodebuild's
//!    "Whisker Prebuild" Build Phase invokes via the `whisker-build
//!    ios` CLI. Cross-compiles the user crate as a Mach-O `.dylib`
//!    for each requested arch (`$ARCHS` from Xcode), lipo-fuses sim
//!    slices when both are requested, wraps the result into a
//!    `WhiskerDriver.framework/` and drops it at
//!    `$BUILT_PRODUCTS_DIR/Frameworks/`. xcodebuild's link step picks
//!    it up via `OTHER_LDFLAGS += -framework WhiskerDriver` and the
//!    "Whisker Embed Framework" phase copies it into the `.app`
//!    bundle.
//!
//! 2. [`run_xcodebuild_app`] — invoke `xcodebuild` against the
//!    cng-generated `<scheme>.xcodeproj` under `gen/ios/`, returning
//!    the produced `.app`. Trigger #1 above runs from inside this
//!    xcodebuild invocation via the Build Phase.
//!
//! Why `dylib` (not `staticlib`)? subsecond's hot-patch model needs
//! the dylib's `.dynsym` available to read mangled Rust symbols
//! against at runtime. Matches the Android side's choice. See
//! `docs/hot-reload-plan.md` "Second Pivot".
//!
//! Tier 1 fat-build capture (see [`crate::capture`]) is opt-in via
//! the `capture` parameter on the per-arch cargo helper. The
//! dev-server wires it up by setting `RUSTC_WORKSPACE_WRAPPER` /
//! `CARGO_TARGET_*_LINKER` / `CARGO_TARGET_*_RUSTFLAGS` env vars on
//! the xcodebuild Command (see `whisker-dev-server::installer`); the
//! variables propagate through to the Build Phase shell, the
//! `whisker build-ios` subprocess, and finally cargo. Direct `xcodebuild`
//! sets no env so the build runs without capture.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Remote SwiftPM source for the `Whisker` package — provides
/// `WhiskerRuntime`, the `Lynx*` binary frameworks, and the
/// `WhiskerModuleCodegenPlugin`. The generated aggregator `Package.swift`
/// and every module manifest reference this single identity (`whisker`,
/// the lowercased last URL path component) so the SwiftPM build graph has
/// one `WhiskerRuntime`. This is what lets iOS apps build outside the
/// monorepo — no `platforms/ios` local path required.
///
/// Keep in lockstep with the `Package.swift` at the repo root and the
/// `v<version>` git tag published for SwiftPM to resolve.
pub const WHISKER_IOS_SPM_URL: &str = "https://github.com/whiskerrs/whisker.git";
pub const WHISKER_IOS_SPM_VERSION: &str = "0.1.0";

use crate::capture::{CaptureShims, capture_env_vars_for_triple};

const FRAMEWORK_NAME: &str = "WhiskerDriver";

/// `extern "C"` bridge entry points that need to land in the dylib's
/// `.dynsym` so Swift can call them across the framework boundary.
/// Keep in sync with the `WHISKER_BRIDGE_EXPORT`-tagged declarations
/// in `crates/whisker-driver-sys/bridge/include/whisker_bridge.h`. If
/// you add a new bridge function there without listing it here, Swift
/// linking will fail with `Undefined symbols: _<name>`.
///
/// Leading underscore is the Mach-O C-symbol prefix; `ld64`'s
/// `-exported_symbol` flag expects it.
const BRIDGE_EXPORTS: &[&str] = &[
    "_whisker_bridge_engine_attach",
    "_whisker_bridge_engine_release",
    "_whisker_bridge_dispatch",
    "_whisker_bridge_create_element",
    "_whisker_bridge_create_element_by_name",
    "_whisker_bridge_release_element",
    "_whisker_bridge_set_attribute",
    "_whisker_bridge_set_inline_styles",
    "_whisker_bridge_append_child",
    "_whisker_bridge_remove_child",
    "_whisker_bridge_set_event_listener",
    "_whisker_bridge_set_event_listener_with_value",
    // Phase 5: Rust-side event propagation. The driver registers a
    // dispatcher the reporter hook forwards to, and queries element
    // signs to key its tree + listener maps.
    "_whisker_bridge_register_event_dispatcher",
    "_whisker_bridge_element_sign",
    "_whisker_bridge_set_native_event_handler",
    "_whisker_bridge_set_root",
    "_whisker_bridge_flush",
    "_whisker_bridge_invoke_module",
    "_whisker_bridge_invoke_module_async",
    "_whisker_bridge_value_release",
    // Phase 7-Φ.F: Swift Macro `@WhiskerModule` emits an `@_cdecl`
    // dispatch shim per module + the generated
    // `WhiskerModuleBehaviors.swift` calls this to register the
    // shim against the C-side dispatch table. Replaces the
    // previous `_OBJC_CLASS_$_WhiskerModuleRegistry` export (the
    // Obj-C class is gone — pure C function pointer table now).
    "_whisker_bridge_register_module_dispatch",
    // whisker-module event system (Phase L-2c). `add/remove_event_listener`
    // is consumed by Rust subscribers (e.g. AndroidPredictiveBack);
    // `send_event` is the native module → Rust fan-out;
    // `register_observer_hooks` drives OnStart/StopObserving. Without
    // these in the whitelist iOS link drops them and
    // `WhiskerModuleEventCenter.swift` fails to resolve at link time.
    "_whisker_bridge_module_add_event_listener",
    "_whisker_bridge_module_remove_event_listener",
    "_whisker_bridge_module_send_event",
    "_whisker_bridge_module_register_observer_hooks",
    "_whisker_bridge_log_hello",
    "_whisker_bridge_log_info",
];

/// `cargo rustc --release --crate-type dylib --target <triple>` for one
/// iOS triple. Appends `-Wl,-exported_symbol,<sym>` for every entry in
/// [`BRIDGE_EXPORTS`] so Swift can dlsym them across the framework
/// boundary.
///
/// `--release` is always set regardless of `capture` — iOS dev's
/// Tier 1 capture wants the same optimised codegen prod ships. The
/// only thing that changes when `capture` is `Some` is the env-var
/// envelope (RUSTC_WORKSPACE_WRAPPER, the linker shim, save-temps,
/// debug-assertions, export-dynamic) — see [`crate::capture_env_vars`].
fn cargo_build_ios_dylib(
    workspace_root: &Path,
    package: &str,
    triple: &str,
    features: &[String],
    capture: Option<&CaptureShims>,
    step: &crate::ui::Step,
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "rustc",
        "--release",
        "-p",
        package,
        "--target",
        triple,
        "--crate-type",
        "dylib",
    ]);
    for feat in features {
        cmd.args(["--features", feat]);
    }
    cmd.arg("--");
    for sym in BRIDGE_EXPORTS {
        cmd.arg(format!("-Clink-arg=-Wl,-exported_symbol,{sym}"));
    }
    if let Some(c) = capture {
        std::fs::create_dir_all(&c.rustc_cache_dir)
            .with_context(|| format!("create rustc cache dir {}", c.rustc_cache_dir.display()))?;
        std::fs::create_dir_all(&c.linker_cache_dir)
            .with_context(|| format!("create linker cache dir {}", c.linker_cache_dir.display()))?;
        // Use the *current iteration's* triple, not whatever was
        // baked into `c.target_triple`. Without this override every
        // slice except the matching one would build without
        // `-Cdebug-assertions=on`, which silently disables
        // subsecond's JumpTable dispatch — `subsecond::call` then
        // inlines to its `if !cfg!(debug_assertions) { return f() }`
        // early return and hot reload patches never reach user code.
        for (k, v) in capture_env_vars_for_triple(c, Some(triple)) {
            cmd.env(k, v);
        }
    }
    cmd.current_dir(workspace_root);
    let status = step.pipe(&mut cmd).context("spawn cargo")?;
    if !status.success() {
        return Err(anyhow!("cargo rustc failed for {triple} ({status})"));
    }
    Ok(())
}

/// Build a `<FRAMEWORK_NAME>.framework/` directory inside `parent`,
/// copying the dylib at `dylib_src` to the framework's main binary,
/// and assembling Headers/, Modules/, Info.plist. Sets the binary's
/// LC_ID_DYLIB to `@rpath/<FRAMEWORK_NAME>.framework/<FRAMEWORK_NAME>`
/// so the embedding app's `@executable_path/Frameworks` rpath
/// resolves it at runtime.
///
/// Returns the path to the constructed `.framework` directory.
fn build_framework_dir(
    parent: &Path,
    dylib_src: &Path,
    rust_headers_src: &Path,
    bridge_headers_src: &Path,
) -> Result<PathBuf> {
    let fw_dir = parent.join(format!("{FRAMEWORK_NAME}.framework"));
    crate::ui::debug(format!("stage {}", fw_dir.display()));
    if fw_dir.exists() {
        std::fs::remove_dir_all(&fw_dir)?;
    }
    std::fs::create_dir_all(&fw_dir)?;

    // Main binary: copy dylib, rename to `<FRAMEWORK_NAME>` (no
    // extension, no `lib` prefix — Apple's flat-framework convention).
    let binary_dst = fw_dir.join(FRAMEWORK_NAME);
    std::fs::copy(dylib_src, &binary_dst)
        .with_context(|| format!("copy {} → {}", dylib_src.display(), binary_dst.display()))?;

    // Rewrite LC_ID_DYLIB to the @rpath form. The Cargo build sets
    // install_name via `-Wl,-install_name,...` (see
    // `crates/whisker-driver-sys/build.rs`), but we run
    // `install_name_tool` here as belt-and-suspenders so the lipo'd
    // fat binary and any pre-build-script-flag-less invocation also
    // end up correct.
    let install_name = format!("@rpath/{FRAMEWORK_NAME}.framework/{FRAMEWORK_NAME}");
    let status = Command::new("install_name_tool")
        .args(["-id", &install_name])
        .arg(&binary_dst)
        .status()
        .context("spawn install_name_tool")?;
    if !status.success() {
        return Err(anyhow!(
            "install_name_tool failed on {} ({status})",
            binary_dst.display(),
        ));
    }

    // Headers/
    let hdr_dir = fw_dir.join("Headers");
    std::fs::create_dir_all(&hdr_dir)?;
    std::fs::copy(
        rust_headers_src.join("whisker.h"),
        hdr_dir.join("whisker.h"),
    )?;
    std::fs::copy(
        bridge_headers_src.join("whisker_bridge.h"),
        hdr_dir.join("whisker_bridge.h"),
    )?;

    // Modules/module.modulemap — framework form (`framework module …`).
    // The repo-level modulemap is a plain `module …` declaration; the
    // framework xcframework wants the `framework module` keyword so
    // Xcode can `import` it.
    //
    // Phase 7-Φ.F: dropped `whisker_module_registry.h` from the
    // header set — the Obj-C class is gone, replaced by a pure C
    // function-pointer table inside `whisker_bridge_common.cc`. The
    // C ABI in `whisker_bridge.h` (the `whisker_bridge_register_module_dispatch`
    // declaration) is the only surface user-app code needs.
    let mod_dir = fw_dir.join("Modules");
    std::fs::create_dir_all(&mod_dir)?;
    std::fs::write(
        mod_dir.join("module.modulemap"),
        format!(
            "framework module {FRAMEWORK_NAME} {{\n    \
             header \"whisker.h\"\n    \
             header \"whisker_bridge.h\"\n    \
             export *\n\
             }}\n"
        ),
    )?;

    // Info.plist — Apple's mandatory bundle metadata. Without this,
    // codesign on the embedded framework fails with "bundle format
    // unrecognized, invalid, or unsuitable".
    std::fs::write(fw_dir.join("Info.plist"), framework_info_plist())?;

    Ok(fw_dir)
}

// ----- Xcode Run Script Phase entry point -----------------------------------

/// Inputs from an Xcode Run Script Build Phase invocation of the
/// `whisker-build` binary. Mirrors the Xcode environment 1:1 — the
/// caller (binary's `run_ios`) parses argv into one of these and
/// hands it to [`build_framework_for_xcode_run_script`].
pub struct XcodeRunScriptInputs<'a> {
    pub workspace_root: &'a Path,
    pub package: &'a str,
    /// `PLATFORM_NAME` — `"iphoneos"` or `"iphonesimulator"`. Drives
    /// the (arch → rust triple) mapping inside
    /// [`map_arch_to_triple`].
    pub platform: &'a str,
    /// `ARCHS`, split on whitespace by the caller. Each entry is
    /// `"arm64"` or `"x86_64"`. Multi-arch is only meaningful when
    /// `platform == "iphonesimulator"` — the iphoneos slice is always
    /// arm64 today.
    pub archs: &'a [&'a str],
    /// Cargo `--features` to forward to each slice's cross-compile.
    /// `whisker run` populates `["whisker/hot-reload"]` so the user
    /// dylib carries the dev-runtime WebSocket client; direct `xcodebuild` invocations
    /// leaves this empty for prod.
    pub features: &'a [String],
}

/// Cross-compile + framework-wrap path for the Xcode Run Script
/// Phase. Cargo-builds one dylib per requested arch, lipo-fuses sim
/// slices when both archs are requested, wraps the result into a
/// `WhiskerDriver.framework/` and drops it at
/// `<built_products_dir>/Frameworks/<FRAMEWORK_NAME>.framework/`
/// where xcodebuild's link step picks it up via
/// `OTHER_LDFLAGS += -framework WhiskerDriver`.
///
/// Returns the path to the produced `.framework` directory.
/// Resolve the `include` dirs of `whisker-driver` (`whisker.h` +
/// `module.modulemap`) and `whisker-driver-sys` (`whisker_bridge.h`).
///
/// Uses `cargo metadata` against `workspace_root` so the paths point at
/// wherever cargo actually placed each crate: the monorepo
/// `crates/whisker-driver*/…` for in-workspace development, or the
/// registry extraction (`~/.cargo/registry/src/index.crates.io-*/
/// whisker-driver-<v>/…`) for a `cargo install`-only user. Falls back
/// to the legacy in-workspace layout if metadata can't be read (e.g.
/// cargo missing from the Run Script env), preserving the old behaviour.
fn resolve_bridge_header_dirs(workspace_root: &Path) -> (PathBuf, PathBuf) {
    let legacy = || {
        (
            workspace_root.join("crates/whisker-driver/include"),
            workspace_root.join("crates/whisker-driver-sys/bridge/include"),
        )
    };
    let Ok(meta) = cargo_metadata::MetadataCommand::new()
        .current_dir(workspace_root)
        .exec()
    else {
        return legacy();
    };
    let crate_dir = |name: &str| -> Option<PathBuf> {
        meta.packages
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.manifest_path.parent())
            .map(|d| d.as_std_path().to_path_buf())
    };
    match (crate_dir("whisker-driver"), crate_dir("whisker-driver-sys")) {
        (Some(driver), Some(driver_sys)) => {
            (driver.join("include"), driver_sys.join("bridge/include"))
        }
        // Either crate absent from the graph (shouldn't happen — both
        // are transitive deps of `whisker`) → legacy layout.
        _ => legacy(),
    }
}

pub fn build_framework_for_xcode_run_script(
    inputs: &XcodeRunScriptInputs<'_>,
    built_products_dir: &Path,
) -> Result<PathBuf> {
    if inputs.archs.is_empty() {
        return Err(anyhow!("--archs is empty; Xcode passed no ARCHS"));
    }

    // Header trees the framework's `Headers/` dir copies from. Resolve
    // the owning crates' on-disk locations via `cargo metadata` so this
    // works both in-workspace (monorepo `crates/…`) and for a crates.io
    // user (the registry extraction, `~/.cargo/registry/src/…`). The
    // headers ship in both `whisker-driver` and `whisker-driver-sys`
    // (no `exclude`, so every git-tracked file is published).
    let (rust_headers_src, bridge_headers_src) = resolve_bridge_header_dirs(inputs.workspace_root);
    for required in ["whisker.h", "module.modulemap"] {
        if !rust_headers_src.join(required).is_file() {
            return Err(anyhow!(
                "missing header {} (expected at {})",
                required,
                rust_headers_src.display(),
            ));
        }
    }
    if !bridge_headers_src.join("whisker_bridge.h").is_file() {
        return Err(anyhow!(
            "missing whisker_bridge.h (expected at {})",
            bridge_headers_src.display(),
        ));
    }

    let lib_stem = inputs.package.replace('-', "_");
    let cargo_dylib_name = format!("lib{lib_stem}.dylib");

    // Build one dylib per requested arch.
    let mut slice_paths: Vec<PathBuf> = Vec::with_capacity(inputs.archs.len());
    for arch in inputs.archs {
        let triple = map_arch_to_triple(inputs.platform, arch)?;
        let s = crate::ui::step("compile", format!("{} ({triple})", inputs.package));
        cargo_build_ios_dylib(
            inputs.workspace_root,
            inputs.package,
            triple,
            inputs.features,
            None,
            &s,
        )?;
        s.done("");
        slice_paths.push(
            inputs
                .workspace_root
                .join("target")
                .join(triple)
                .join("release")
                .join(&cargo_dylib_name),
        );
    }

    // Workspace-local scratch area: lipo + wrap happens here, then
    // the final framework dir is copied into `built_products_dir`.
    // Under `target/` so `cargo clean` reaps it.
    let out_dir = inputs
        .workspace_root
        .join("target/whisker-driver/run-script");
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir)
            .with_context(|| format!("rm -rf {}", out_dir.display()))?;
    }
    std::fs::create_dir_all(&out_dir).with_context(|| format!("mkdir -p {}", out_dir.display()))?;

    // Single-arch → use the slice directly. Multi-arch → lipo into a
    // fat binary in `out_dir`.
    let combined_dylib: PathBuf = if slice_paths.len() == 1 {
        slice_paths.into_iter().next().expect("checked len == 1")
    } else {
        let fat = out_dir.join(&cargo_dylib_name);
        crate::ui::debug(format!("lipo {}", fat.display()));
        let mut cmd = Command::new("lipo");
        cmd.arg("-create");
        for p in &slice_paths {
            if !p.is_file() {
                return Err(anyhow!("expected dylib not built: {}", p.display()));
            }
            cmd.arg(p);
        }
        cmd.args(["-output"]).arg(&fat);
        let status = cmd.status().context("spawn lipo")?;
        if !status.success() {
            return Err(anyhow!("lipo failed ({status})"));
        }
        fat
    };

    let staged_fw = build_framework_dir(
        &out_dir,
        &combined_dylib,
        &rust_headers_src,
        &bridge_headers_src,
    )?;

    // Publish into `<built_products_dir>/Frameworks/`. Xcode's
    // embed-frameworks build phase scans that directory at link time.
    let frameworks_dst = built_products_dir.join("Frameworks");
    std::fs::create_dir_all(&frameworks_dst)
        .with_context(|| format!("mkdir -p {}", frameworks_dst.display()))?;
    let published_fw = frameworks_dst.join(format!("{FRAMEWORK_NAME}.framework"));
    if published_fw.exists() {
        std::fs::remove_dir_all(&published_fw)
            .with_context(|| format!("rm -rf {}", published_fw.display()))?;
    }
    copy_dir_recursive(&staged_fw, &published_fw)?;
    crate::ui::info(format!(
        "publish {}.framework → {}",
        FRAMEWORK_NAME,
        published_fw.display(),
    ));
    Ok(published_fw)
}

/// Translate Xcode's `(PLATFORM_NAME, ARCH)` pair into the matching
/// Rust target triple. Pairs that can't appear in a real Xcode
/// build (`iphoneos` + `x86_64`, the long-deprecated armv7 device
/// slice) hit the catch-all so the binary surfaces a clear error
/// before cargo even starts.
fn map_arch_to_triple(platform: &str, arch: &str) -> Result<&'static str> {
    match (platform, arch) {
        ("iphoneos", "arm64") => Ok("aarch64-apple-ios"),
        ("iphonesimulator", "arm64") => Ok("aarch64-apple-ios-sim"),
        ("iphonesimulator", "x86_64") => Ok("x86_64-apple-ios"),
        (p, a) => Err(anyhow!(
            "unsupported (PLATFORM_NAME, ARCH) pair: ({p}, {a})"
        )),
    }
}

/// `cp -R src dst` — file by file so we don't drag in a `fs_extra`
/// dep just for one call site. The framework dir is shallow enough
/// (Headers/, Modules/, plus the binary + Info.plist) that an
/// inline walk is cheaper than vendoring a crate.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("mkdir -p {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("readdir {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} → {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Minimal Info.plist that satisfies codesign + dyld for an embedded
/// iOS framework. CFBundleExecutable must match the binary filename
/// (= `FRAMEWORK_NAME`).
fn framework_info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>{FRAMEWORK_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>rs.whisker.{lower}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{FRAMEWORK_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>FMWK</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>MinimumOSVersion</key>
    <string>13.0</string>
</dict>
</plist>
"#,
        lower = FRAMEWORK_NAME.to_lowercase(),
    )
}

// ----- xcodebuild -----------------------------------------------------------

/// Configuration for an `xcodebuild` invocation against the
/// CNG-generated `gen/ios/<scheme>.xcodeproj`.
pub struct XcodebuildArgs<'a> {
    pub gen_ios: &'a Path,
    pub scheme: &'a str,
    /// `iphonesimulator` (Simulator) or `iphoneos` (device).
    pub sdk: &'a str,
    /// `Release` for direct `xcodebuild` invocations; `Debug` is unused today but
    /// kept generic so Phase 3 can reuse this for `whisker run`'s
    /// initial build.
    pub configuration: &'a str,
    /// Almost always `<scheme>.xcodeproj` (XcodeGen output). Tests
    /// override it to point at fixtures.
    pub xcodeproj_name: &'a str,
    /// `-derivedDataPath` value. Picked by the caller so the gen
    /// tree stays drift-free for the next sync.
    pub derived_data: &'a Path,
    /// Absolute path to Whisker's `platforms/ios` SwiftPM package.
    /// Exported to `xcodebuild` as `WHISKER_IOS_RUNTIME` so each
    /// module's `Package.swift` resolves its `WhiskerRuntime`
    /// dependency by env (with a relative fallback) instead of a
    /// hardcoded `../../platforms/ios` — which only works when the
    /// module sits at a fixed depth in the monorepo. Mirrors how the
    /// Android side injects each module's `projectDir`. `None` skips
    /// the export (the module then relies on its relative fallback).
    pub whisker_runtime_path: Option<&'a Path>,
    /// Absolute path to Whisker's `platforms/ios/macros` SwiftPM
    /// package. Exported as `WHISKER_IOS_MACROS`. Sibling of
    /// [`Self::whisker_runtime_path`].
    pub whisker_ios_macros_path: Option<&'a Path>,
}

/// Generate the iOS module-aggregator SwiftPM package under
/// `gen/ios/whisker_modules/`. Phase 7-Φ.G: replaces the previous
/// file-copy flow (`stage_module_swift_sources`) — module source
/// files now stay in their own package directories, and each module
/// ships its own hand-written `Package.swift`. The aggregator simply
/// depends on each module package via `.package(path: …)` and
/// imports each one's per-target register fn.
///
/// Mirror of [`crate::android::generate_module_aggregator`] for
/// iOS. The Android path generates `settings.gradle.kts` includes;
/// the iOS equivalent produces a tiny SwiftPM package the user
/// app declares as a local Swift Package Dependency.
///
/// Layout produced (within `gen/ios/whisker_modules/`):
///
/// ```text
/// whisker_modules/
/// ├── Package.swift                       ← generated (aggregator)
/// └── Sources/WhiskerModules/
///     └── RegisterAll.swift               ← generated
/// ```
///
/// `Package.swift` declares one product (`WhiskerModules`) depending
/// on `WhiskerRuntime` + each discovered module's local-path
/// SwiftPM package. The user app's pbxproj template references
/// both `native/ios` (WhiskerRuntime) and `gen/ios/whisker_modules`
/// (the aggregator) as `XCLocalSwiftPackageReference` entries —
/// SwiftPM resolves the transitive deps to each module package.
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
    // Retained for call-site compatibility; the aggregator now
    // references WhiskerRuntime + macros via the remote `whisker` SPM
    // package instead of these local paths.
    _whisker_runtime_path: &Path,
    _whisker_ios_macros_path: &Path,
    modules: &[crate::modules::ResolvedModule],
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

    // Each module package contributes via its own Package.swift in
    // its manifest dir. Discovery signal: presence of `Package.swift`
    // next to the crate's `Cargo.toml` (Phase G dropped the
    // `swift_sources` field as the staging trigger). Modules that
    // are Android-only naturally don't have a Package.swift, so
    // they're skipped here without further filtering.
    let ios_modules: Vec<&crate::modules::ResolvedModule> = modules
        .iter()
        .filter(|m| m.manifest_dir.join("Package.swift").is_file())
        .collect();

    let package_path = root.join("Package.swift");
    std::fs::write(&package_path, render_modules_package_swift(&ios_modules))
        .with_context(|| format!("write {}", package_path.display()))?;

    let register_all_path = sources_root.join("RegisterAll.swift");
    std::fs::write(&register_all_path, render_register_all_swift(&ios_modules))
        .with_context(|| format!("write {}", register_all_path.display()))?;

    if !ios_modules.is_empty() {
        crate::ui::info(format!(
            "stage {n} module SPM package(s) under whisker_modules/",
            n = ios_modules.len()
        ));
    }
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

/// Render `Package.swift` for the generated `WhiskerModules`
/// aggregator. Depends on `WhiskerRuntime` + each discovered
/// module package via local-path SwiftPM dependency.
fn render_modules_package_swift(modules: &[&crate::modules::ResolvedModule]) -> String {
    let mut out = String::new();
    out.push_str(
        "// swift-tools-version:5.9\n\
         //\n\
         // AUTO-GENERATED by whisker-build. Do NOT edit — re-run\n\
         // `whisker run` to refresh.\n\
         //\n\
         // Phase 7-Φ.G aggregator. Each Whisker module ships its\n\
         // own SwiftPM package (with hand-written Package.swift),\n\
         // and this file just lists them as local-path dependencies.\n\
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
    out.push_str("    platforms: [.iOS(.v13)],\n");
    out.push_str("    products: [\n");
    out.push_str("        .library(name: \"WhiskerModules\", targets: [\"WhiskerModules\"]),\n");
    out.push_str("    ],\n");
    out.push_str("    dependencies: [\n");
    // WhiskerRuntime + Lynx + the codegen plugin all come from the one
    // remote `whisker` package (no monorepo `platforms/ios` path).
    out.push_str(&format!(
        "        .package(url: {url:?}, exact: {ver:?}),\n",
        url = WHISKER_IOS_SPM_URL,
        ver = WHISKER_IOS_SPM_VERSION,
    ));
    for m in modules {
        // The module's SwiftPM package is rooted at the package
        // directory (Package.swift lives there, identity = the
        // crate's dir name — unique). Its target sources live under
        // the package's `ios/` subdir (Expo-style layout).
        let path = m.manifest_dir.display().to_string();
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
    out.push_str("                .product(name: \"WhiskerRuntime\", package: \"whisker\"),\n");
    out.push_str("                .product(name: \"Lynx\", package: \"whisker\"),\n");
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
fn render_register_all_swift(modules: &[&crate::modules::ResolvedModule]) -> String {
    let mut out = String::new();
    out.push_str(
        "// AUTO-GENERATED by whisker-build. Do NOT edit — re-run\n\
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

/// Run `xcodebuild -configuration <configuration>` and return the
/// produced `.app` directory.
pub fn run_xcodebuild_app(args: &XcodebuildArgs<'_>) -> Result<PathBuf> {
    let project = args
        .gen_ios
        .join(format!("{}.xcodeproj", args.xcodeproj_name));
    if !project.is_dir() {
        return Err(anyhow!(
            "Xcode project missing at {} — did `xcodegen generate` run?",
            project.display(),
        ));
    }

    let _xc_step = crate::ui::step("xcodebuild", args.xcodeproj_name.to_string());
    let destination = match args.sdk {
        "iphonesimulator" => "generic/platform=iOS Simulator".to_string(),
        "iphoneos" => "generic/platform=iOS".to_string(),
        other => return Err(anyhow!("unknown SDK: {other}")),
    };

    let mut cmd = Command::new("xcodebuild");
    cmd.arg("-project")
        .arg(&project)
        .args(["-scheme", args.scheme])
        .args(["-configuration", args.configuration])
        .args(["-destination", &destination])
        .arg("-derivedDataPath")
        .arg(args.derived_data)
        // The WhiskerModuleCodegenPlugin is a SwiftPM build-tool plugin;
        // Xcode gates plugins behind an interactive trust prompt that a
        // headless build can't answer, so skip validation (the plugin
        // ships from Whisker's own `whisker` SPM package).
        .arg("-skipPackagePluginValidation")
        .args(["-quiet", "build"]);
    if let Some(p) = args.whisker_runtime_path {
        cmd.env("WHISKER_IOS_RUNTIME", p);
    }
    if let Some(p) = args.whisker_ios_macros_path {
        cmd.env("WHISKER_IOS_MACROS", p);
    }
    let status = cmd.status().context("spawn xcodebuild")?;
    if !status.success() {
        return Err(anyhow!("xcodebuild failed ({status})"));
    }

    let product_subdir = match args.sdk {
        "iphonesimulator" => format!("{}-iphonesimulator", args.configuration),
        "iphoneos" => format!("{}-iphoneos", args.configuration),
        _ => unreachable!("checked above"),
    };
    let app = args
        .derived_data
        .join("Build/Products")
        .join(product_subdir)
        .join(format!("{}.app", args.scheme));
    if !app.is_dir() {
        return Err(anyhow!(
            "xcodebuild succeeded but {} is missing",
            app.display(),
        ));
    }
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_exports_have_leading_underscore() {
        // ld64's `-exported_symbol` expects the Mach-O C symbol form.
        // Dropping the underscore would silently leave the symbol
        // out of `.dynsym` and Swift would fail to link the bridge.
        for sym in BRIDGE_EXPORTS {
            assert!(
                sym.starts_with('_'),
                "BRIDGE_EXPORTS entry missing leading underscore: {sym}",
            );
        }
    }

    #[test]
    fn framework_info_plist_contains_executable_name() {
        let plist = framework_info_plist();
        assert!(plist.contains("<string>WhiskerDriver</string>"));
        assert!(plist.contains("FMWK"));
    }

    #[test]
    fn missing_xcodeproj_errors() {
        let tmp = std::env::temp_dir().join("whisker-cli-build_ios-test");
        let _ = std::fs::create_dir_all(&tmp);
        let dd = tmp.join("derived");
        let args = XcodebuildArgs {
            gen_ios: &tmp,
            scheme: "X",
            sdk: "iphonesimulator",
            configuration: "Release",
            xcodeproj_name: "X",
            derived_data: &dd,
            whisker_runtime_path: None,
            whisker_ios_macros_path: None,
        };
        let err = run_xcodebuild_app(&args).unwrap_err();
        assert!(
            err.to_string().contains("Xcode project missing"),
            "got: {err:#}",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unknown_sdk_errors() {
        let tmp = std::env::temp_dir().join("whisker-cli-build_ios-sdk-test");
        let _ = std::fs::create_dir_all(&tmp);
        let proj = tmp.join("X.xcodeproj");
        let _ = std::fs::create_dir_all(&proj);
        let dd = tmp.join("derived");
        let args = XcodebuildArgs {
            gen_ios: &tmp,
            scheme: "X",
            sdk: "bogus",
            configuration: "Release",
            xcodeproj_name: "X",
            derived_data: &dd,
            whisker_runtime_path: None,
            whisker_ios_macros_path: None,
        };
        let err = run_xcodebuild_app(&args).unwrap_err();
        assert!(err.to_string().contains("unknown SDK"), "got: {err:#}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
