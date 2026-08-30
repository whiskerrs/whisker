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
//! `docs/hot-reload-internals.md`.
//!
//! hot reload fat-build capture (see [`crate::capture`]) is opt-in via
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

/// Remote SwiftPM source for the `Whisker` package — provides the native
/// module authoring API and `WhiskerModuleCodegenPlugin`. The generated aggregator `Package.swift`
/// and every module manifest reference this single identity (`whisker`,
/// the lowercased last URL path component) so the SwiftPM build graph has
/// one `WhiskerRuntime`. This is what lets iOS apps build outside the
/// monorepo — no `platforms/ios` local path required.
///
/// Keep in lockstep with the `Package.swift` at the repo root and the
/// `v<version>` git tag published for SwiftPM to resolve.
pub const WHISKER_IOS_SPM_URL: &str = "https://github.com/whiskerrs/whisker.git";
pub const WHISKER_IOS_SPM_VERSION: &str = "0.1.11";

use crate::capture::{CaptureShims, capture_env_vars_for_triple};

const FRAMEWORK_NAME: &str = "WhiskerDriver";

/// Retained mobile entry points Swift calls across the framework boundary.
///
/// Leading underscore is the Mach-O C-symbol prefix; `ld64`'s
/// `-exported_symbol` flag expects it.
const BRIDGE_EXPORTS: &[&str] = &[
    "_whisker_view_create",
    "_whisker_view_tick",
    "_whisker_view_destroy",
    "_whisker_view_dispatch_event",
    "_whisker_view_dispatch_pointer",
    "_whisker_view_dispatch_module_event",
    "_whisker_view_dispatch_resource_event",
    "_whisker_aslr_anchor",
];

/// `cargo rustc --release --crate-type {cdylib,dylib} --target <triple>` for one
/// iOS triple. Appends `-Wl,-exported_symbol,<sym>` for every entry in
/// [`BRIDGE_EXPORTS`] so Swift can dlsym them across the framework
/// boundary.
///
/// `--release` is always set regardless of `capture` — iOS dev's
/// hot reload capture wants the same optimised codegen prod ships. The
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
    let hot_reload = features
        .iter()
        .any(|feature| feature.contains("hot-reload"));
    let crate_type = if hot_reload { "dylib" } else { "cdylib" };
    cmd.args([
        "rustc",
        "--release",
        "-p",
        package,
        "--target",
        triple,
        "--crate-type",
        crate_type,
    ]);
    if !hot_reload {
        cmd.env("CARGO_PROFILE_RELEASE_LTO", "fat")
            .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
            .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "z")
            .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
            .env("CARGO_PROFILE_RELEASE_PANIC", "abort");
    }
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
        // Override with this iteration's triple: any slice built
        // without `-Cdebug-assertions=on` silently loses subsecond's
        // JumpTable dispatch and never sees a patch.
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
fn build_framework_dir(parent: &Path, dylib_src: &Path) -> Result<PathBuf> {
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

    // `crates/whisker-driver-sys/build.rs` already passes
    // `-Wl,-install_name,…`; rewriting it here too covers the lipo'd
    // fat binary and any invocation that missed the build-script flag.
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

    let hdr_dir = fw_dir.join("Headers");
    std::fs::create_dir_all(&hdr_dir)?;
    std::fs::write(
        hdr_dir.join("whisker.h"),
        "#pragma once\n\n/*\n * Link-only application framework. The stable typed Host ABI is declared by\n * WhiskerCBridge/whisker_mobile.h so this wrapper cannot drift from it.\n */\n",
    )?;

    // The repo-level modulemap is a plain `module …` declaration; a
    // framework needs the `framework module` keyword for Xcode to
    // `import` it.
    let mod_dir = fw_dir.join("Modules");
    std::fs::create_dir_all(&mod_dir)?;
    std::fs::write(
        mod_dir.join("module.modulemap"),
        format!(
            "framework module {FRAMEWORK_NAME} {{\n    \
             header \"whisker.h\"\n    \
             export *\n\
             }}\n"
        ),
    )?;

    // Info.plist — Apple's mandatory bundle metadata. Without this,
    // codesign on the embedded framework fails with "bundle format
    // unrecognized, invalid, or unsuitable".
    std::fs::write(
        fw_dir.join("Info.plist"),
        framework_info_plist(&min_os_version()),
    )?;

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
pub fn build_framework_for_xcode_run_script(
    inputs: &XcodeRunScriptInputs<'_>,
    built_products_dir: &Path,
) -> Result<PathBuf> {
    if inputs.archs.is_empty() {
        return Err(anyhow!("--archs is empty; Xcode passed no ARCHS"));
    }

    let lib_stem = inputs.package.replace('-', "_");
    let cargo_dylib_name = format!("lib{lib_stem}.dylib");

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

    // Scratch area for lipo + wrap, under `target/` so `cargo clean`
    // reaps it.
    let out_dir = inputs
        .workspace_root
        .join("target/whisker-driver/run-script");
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir)
            .with_context(|| format!("rm -rf {}", out_dir.display()))?;
    }
    std::fs::create_dir_all(&out_dir).with_context(|| format!("mkdir -p {}", out_dir.display()))?;

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

    let staged_fw = build_framework_dir(&out_dir, &combined_dylib)?;

    // Xcode's embed-frameworks phase scans `Frameworks/` at link time.
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

/// The app's iOS deployment target, straight from the environment
/// xcodebuild runs the Build Phase with — the same value the app
/// target itself compiles against, and the value cargo hands the Rust
/// dylib. Falls back to whisker's own floor outside xcodebuild.
fn min_os_version() -> String {
    std::env::var("IPHONEOS_DEPLOYMENT_TARGET").unwrap_or_else(|_| DEFAULT_MIN_OS.to_string())
}

const DEFAULT_MIN_OS: &str = "13.0";

/// Minimal Info.plist that satisfies codesign + dyld for an embedded
/// iOS framework. CFBundleExecutable must match the binary filename
/// (= `FRAMEWORK_NAME`).
///
/// `MinimumOSVersion` tracks the app's deployment target rather than a
/// constant: App Store validation rejects the upload when an embedded
/// framework disagrees with the app about its minimum
/// ("The bundle …/WhiskerDriver.framework does not support the minimum
/// OS Version specified in the Info.plist", 90208), and the dylib
/// inside really is built for whatever xcodebuild passed down.
fn framework_info_plist(min_os: &str) -> String {
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
    <string>{min_os}</string>
</dict>
</plist>
"#,
        lower = FRAMEWORK_NAME.to_lowercase(),
    )
}

// ----- xcodebuild -----------------------------------------------------------

/// Configuration for an `xcodebuild` invocation against the
/// CNG-generated `gen/ios/<scheme>.xcodeproj`.
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

// ============================================================================
// Release (ipa) pipeline — `whisker build ipa`
// ============================================================================

/// ExportOptions.plist `method` values. Spelled exactly like Apple's
/// plist values (and Flutter's `--export-method`), so the string the
/// user typed matches what xcodebuild's own errors echo back —
/// deliberately NOT fastlane's hyphen-less `adhoc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMethod {
    /// App Store / TestFlight distribution.
    AppStoreConnect,
    /// Device-limited distribution (Firebase App Distribution etc.);
    /// requires tester UDIDs registered on the developer portal.
    AdHoc,
}

impl ExportMethod {
    pub fn plist_value(self) -> &'static str {
        match self {
            ExportMethod::AppStoreConnect => "app-store-connect",
            ExportMethod::AdHoc => "ad-hoc",
        }
    }
}

/// Signing inputs for the release pipeline. All auth flows through
/// the App Store Connect API key (whisker's one true path — no
/// Xcode-session dependence, identical local and CI), with the
/// cloud-managed distribution certificate doing the actual signing
/// at export time.
pub struct ReleaseSigning<'a> {
    pub team_id: &'a str,
    /// Absolute path to `AuthKey_<key_id>.p8` — points into the
    /// caller's credential staging dir.
    pub key_path: &'a Path,
    pub key_id: &'a str,
    pub issuer_id: &'a str,
}

pub struct IosReleaseInputs<'a> {
    /// `gen/ios/` — synced by cng before this runs.
    pub gen_dir: &'a Path,
    pub scheme: &'a str,
    pub workspace_root: &'a Path,
    /// User app crate name — namespaces the archive/export output dir.
    pub package: &'a str,
    pub method: ExportMethod,
    pub signing: ReleaseSigning<'a>,
}

/// `xcodebuild archive` → `xcodebuild -exportArchive` → `.ipa`.
///
/// Signing setup is passed as command-line build settings
/// (`DEVELOPMENT_TEAM` / `CODE_SIGN_STYLE=Automatic`), NOT baked
/// into the generated pbxproj — the gen tree stays team-agnostic and
/// `whisker run` (simulator) never sees signing at all.
/// `-allowProvisioningUpdates` + the API-key flags let xcodebuild
/// mint dev certificates, register the bundle id, and (re)generate
/// profiles headlessly on both dev machines and CI.
///
/// The archive is kept on disk between runs: adding ad-hoc test
/// devices only needs the export step re-run, and xcodebuild archive
/// itself overwrites stale archives.
pub fn archive_and_export(inputs: &IosReleaseInputs<'_>) -> Result<PathBuf> {
    let IosReleaseInputs {
        gen_dir,
        scheme,
        workspace_root,
        package,
        method,
        signing,
    } = inputs;
    let xcode_project = gen_dir.join(format!("{scheme}.xcodeproj"));
    if !xcode_project.is_dir() {
        return Err(anyhow!(
            "Xcode project missing at {} — has the gen tree been synced?",
            xcode_project.display(),
        ));
    }
    let out_root = workspace_root
        .join("target/whisker/ios-release")
        .join(package);
    std::fs::create_dir_all(&out_root).with_context(|| format!("mkdir {}", out_root.display()))?;
    let archive_path = out_root.join(format!("{scheme}.xcarchive"));

    // ---- archive ---------------------------------------------------
    let archive_step = crate::ui::step("xcodebuild", format!("archive {scheme}"));
    let mut cmd = Command::new("xcodebuild");
    cmd.arg("-project")
        .arg(&xcode_project)
        .args(["-scheme", scheme])
        .args(["-configuration", "Release"])
        .args(["-destination", "generic/platform=iOS"])
        .arg("-archivePath")
        .arg(&archive_path)
        // SwiftPM build-tool plugins sit behind an interactive trust
        // prompt headless builds can't answer.
        .arg("-skipPackagePluginValidation")
        .arg("-allowProvisioningUpdates")
        .arg("-authenticationKeyPath")
        .arg(signing.key_path)
        .args(["-authenticationKeyID", signing.key_id])
        .args(["-authenticationKeyIssuerID", signing.issuer_id])
        .arg("archive")
        // Command-line build settings override the pbxproj: the
        // template's dev-loop identity stays untouched, release
        // builds get automatic signing under the credential's team.
        .arg(format!("DEVELOPMENT_TEAM={}", signing.team_id))
        .arg("CODE_SIGN_STYLE=Automatic")
        .arg("CODE_SIGN_IDENTITY=Apple Development")
        .current_dir(gen_dir);
    let status = archive_step
        .pipe(&mut cmd)
        .context("spawn xcodebuild archive")?;
    if !status.success() {
        archive_step.fail(format!("{status}"));
        return Err(anyhow!("xcodebuild archive failed ({status})"));
    }
    archive_step.done("");

    // ---- export ----------------------------------------------------
    let options_path = out_root.join("ExportOptions.plist");
    std::fs::write(
        &options_path,
        export_options_plist(*method, signing.team_id),
    )
    .with_context(|| format!("write {}", options_path.display()))?;
    let export_dir = out_root.join("export");

    let export_step = crate::ui::step("xcodebuild", format!("export {}", method.plist_value()));
    let mut cmd = Command::new("xcodebuild");
    cmd.arg("-exportArchive")
        .arg("-archivePath")
        .arg(&archive_path)
        .arg("-exportOptionsPlist")
        .arg(&options_path)
        .arg("-exportPath")
        .arg(&export_dir)
        .arg("-allowProvisioningUpdates")
        .arg("-authenticationKeyPath")
        .arg(signing.key_path)
        .args(["-authenticationKeyID", signing.key_id])
        .args(["-authenticationKeyIssuerID", signing.issuer_id]);
    let status = export_step
        .pipe(&mut cmd)
        .context("spawn xcodebuild -exportArchive")?;
    if !status.success() {
        export_step.fail(format!("{status}"));
        return Err(anyhow!(
            "xcodebuild -exportArchive failed ({status}) — if this is a signing error, \
             re-check the stored key with `whisker credential ios`",
        ));
    }
    export_step.done("");

    // Export names the ipa after the app's product name; scan rather
    // than guess.
    let ipa = std::fs::read_dir(&export_dir)
        .with_context(|| format!("read {}", export_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "ipa"))
        .ok_or_else(|| {
            anyhow!(
                "export succeeded but no .ipa found under {}",
                export_dir.display(),
            )
        })?;
    Ok(ipa)
}

/// The ExportOptions.plist for one export. Nothing secret in here —
/// it lands under `target/whisker/ios-release/` and is regenerated
/// every run.
fn export_options_plist(method: ExportMethod, team_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>method</key>
	<string>{}</string>
	<key>signingStyle</key>
	<string>automatic</string>
	<key>teamID</key>
	<string>{}</string>
	<key>destination</key>
	<string>export</string>
</dict>
</plist>
"#,
        method.plist_value(),
        team_id,
    )
}

#[cfg(test)]
mod tests;

mod swift_modules;

pub use swift_modules::{XcodebuildArgs, stage_module_swift_sources};
#[cfg(test)]
use swift_modules::{
    parse_ios_platform_major, render_modules_package_swift, render_register_all_swift,
};
