//! iOS cargo + xcframework + xcodebuild orchestration. Shared by
//! `whisker-cli`'s `whisker build` and `whisker-dev-server`'s Tier 2
//! cold rebuild path.
//!
//! Three phases:
//!
//! 1. [`build_xcframework`] — cross-compile the user crate as a
//!    Mach-O `.dylib` for each iOS triple (`aarch64-apple-ios`,
//!    `aarch64-apple-ios-sim`, `x86_64-apple-ios`), lipo the two
//!    simulator slices into a fat binary, wrap each slice into a
//!    `WhiskerDriver.framework/` directory (with Headers, Modules,
//!    Info.plist), then `xcodebuild -create-xcframework`. Output
//!    lands at `<workspace>/target/whisker-driver/WhiskerDriver.xcframework`,
//!    which the WhiskerRuntime SPM package references.
//!
//! 2. [`run_xcodebuild_app`] — invoke `xcodebuild` against the
//!    XcodeGen-generated `<scheme>.xcodeproj` under `gen/ios/`,
//!    returning the produced `.app`.
//!
//! Why `dylib` (not `staticlib`)? subsecond's hot-patch model needs
//! the dylib's `.dynsym` available to read mangled Rust symbols
//! against at runtime. Matches the Android side's choice. See
//! `docs/hot-reload-plan.md` "Second Pivot".
//!
//! Tier 1 fat-build capture (see [`crate::capture`]) is opt-in via
//! the `capture` parameter on [`build_xcframework`]. Dev-server's
//! Tier 2 cold rebuild passes `Some(&shims)`; `whisker build`
//! passes `None`.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::capture::{capture_env_vars_for_triple, CaptureShims};

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
    "_whisker_bridge_release_element",
    "_whisker_bridge_set_attribute",
    "_whisker_bridge_set_inline_styles",
    "_whisker_bridge_append_child",
    "_whisker_bridge_remove_child",
    "_whisker_bridge_set_event_listener",
    "_whisker_bridge_set_root",
    "_whisker_bridge_flush",
    "_whisker_bridge_log_hello",
];

/// Build the WhiskerDriver.xcframework that wraps `package`'s dylib
/// for every iOS slice the WhiskerRuntime SPM package consumes
/// (`ios-arm64` device + `ios-arm64_x86_64-simulator` fat sim).
///
/// Returns the path to the resulting `.xcframework` directory.
///
/// When `capture` is `Some`, the cargo invocations per triple are
/// elevated into Tier 1 fat builds — they still produce the same
/// dylibs but populate the rustc + linker capture caches so the
/// dev-server's Patcher can construct hot patches. Dev-server's
/// Tier 2 cold rebuild path passes `Some(&shims)`; `whisker build`
/// passes `None` (prod has no Tier 1).
/// Which iOS slice(s) to put in the resulting xcframework.
///
/// `whisker run --target ios-sim` doesn't need the device slice —
/// skipping it cuts the dev-loop's initial build by one cargo
/// triple (~20–60s warm cache). The simulator pair (arm64 + x86_64)
/// stays as a fat slice because `xcodebuild
/// -destination 'generic/platform=iOS Simulator'` insists on
/// linking against a universal binary; pinning `-arch <host>`
/// alongside the destination is rejected by xcodebuild.
///
/// Production `whisker build` keeps the universal shape so the
/// resulting artifact runs on real hardware too.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IosSlices {
    /// arm64 device + arm64 sim + x86_64 sim. Required for shipping.
    Universal,
    /// arm64 sim + x86_64 sim (lipo'd into one fat slice). Right
    /// choice for `whisker run --target ios-sim` — both sim archs
    /// are needed at link time, but the device slice isn't.
    SimulatorFat,
}

pub fn build_xcframework(
    workspace_root: &Path,
    package: &str,
    features: &[String],
    capture: Option<&CaptureShims>,
) -> Result<PathBuf> {
    build_xcframework_with(
        workspace_root,
        package,
        features,
        capture,
        IosSlices::Universal,
    )
}

pub fn build_xcframework_with(
    workspace_root: &Path,
    package: &str,
    features: &[String],
    capture: Option<&CaptureShims>,
    slices: IosSlices,
) -> Result<PathBuf> {
    let out = workspace_root.join("target/whisker-driver");
    let lib_stem = package.replace('-', "_");
    let cargo_dylib_name = format!("lib{lib_stem}.dylib");

    let rust_headers_src = workspace_root.join("crates/whisker-driver/include");
    let bridge_headers_src = workspace_root.join("crates/whisker-driver-sys/bridge/include");
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

    // Clean isn't a step the user needs to watch — it's a sub-100ms
    // rm-rf of our own artifact dir. Just do it silently; the debug
    // line below records the path for verbose-mode triage.
    crate::ui::debug(format!("clean {}", out.display()));
    if out.exists() {
        std::fs::remove_dir_all(&out).with_context(|| format!("rm -rf {}", out.display()))?;
    }
    std::fs::create_dir_all(&out).with_context(|| format!("mkdir -p {}", out.display()))?;

    // Triple set + order driven by `slices`. arm64-sim lands last in
    // both variants so its rustc + linker capture wins the
    // dev-server's last-write-wins cache (Apple-Silicon dev machines
    // run the arm64 simulator natively).
    let triples: &[&str] = match slices {
        IosSlices::Universal => &[
            "aarch64-apple-ios",
            "x86_64-apple-ios",
            "aarch64-apple-ios-sim",
        ],
        IosSlices::SimulatorFat => &["x86_64-apple-ios", "aarch64-apple-ios-sim"],
    };
    for triple in triples {
        let s = crate::ui::step("compile", format!("{package} ({triple})"));
        cargo_build_ios_dylib(workspace_root, package, triple, features, capture, &s)?;
        s.done("");
    }

    let target_dir = workspace_root.join("target");
    // For `SimulatorHost`, this is the lone slice we just built. For
    // `Universal`, this is the arm64 simulator slice that gets
    // lipo'd together with the x86 one further down.
    let sim_arm64_dylib = target_dir
        .join("aarch64-apple-ios-sim/release")
        .join(&cargo_dylib_name);

    let xcf = out.join(format!("{FRAMEWORK_NAME}.xcframework"));
    let xcf_step = crate::ui::step("xcframework", FRAMEWORK_NAME.to_string());

    let mut xcframework_cmd = std::process::Command::new("xcodebuild");
    xcframework_cmd.arg("-create-xcframework");

    // Common: every variant wraps each slice in its own .framework
    // tree before handing the lot to xcodebuild.
    let mut staged_fws: Vec<PathBuf> = Vec::new();

    let sim_x86_dylib = target_dir
        .join("x86_64-apple-ios/release")
        .join(&cargo_dylib_name);
    for p in [&sim_arm64_dylib, &sim_x86_dylib] {
        if !p.is_file() {
            return Err(anyhow!("expected dylib not built: {}", p.display()));
        }
    }

    // Device slice — only for Universal.
    if matches!(slices, IosSlices::Universal) {
        let device_dylib = target_dir
            .join("aarch64-apple-ios/release")
            .join(&cargo_dylib_name);
        if !device_dylib.is_file() {
            return Err(anyhow!(
                "expected dylib not built: {}",
                device_dylib.display()
            ));
        }
        let device_fw_parent = out.join("device");
        let device_fw = build_framework_dir(
            &device_fw_parent,
            &device_dylib,
            &rust_headers_src,
            &bridge_headers_src,
        )?;
        staged_fws.push(device_fw);
    }

    // Lipo arm64 + x86 sim dylibs into a fat sim binary. Both
    // `Universal` and `SimulatorFat` need this — xcodebuild's
    // `-destination 'generic/platform=iOS Simulator'` link demands
    // a universal simulator slice.
    let sim_fat_parent = out.join("sim");
    std::fs::create_dir_all(&sim_fat_parent)?;
    let sim_fat = sim_fat_parent.join(&cargo_dylib_name);
    crate::ui::debug(format!("lipo {}", sim_fat.display()));
    let status = Command::new("lipo")
        .args(["-create"])
        .arg(&sim_arm64_dylib)
        .arg(&sim_x86_dylib)
        .args(["-output"])
        .arg(&sim_fat)
        .status()
        .context("spawn lipo")?;
    if !status.success() {
        xcf_step.fail(format!("lipo {status}"));
        return Err(anyhow!("lipo failed ({status})"));
    }
    let sim_fw = build_framework_dir(
        &sim_fat_parent,
        &sim_fat,
        &rust_headers_src,
        &bridge_headers_src,
    )?;
    staged_fws.push(sim_fw);

    for fw in &staged_fws {
        xcframework_cmd.args(["-framework"]).arg(fw);
    }
    xcframework_cmd.args(["-output"]).arg(&xcf);

    // `xcodebuild -create-xcframework` always prints `xcframework
    // successfully written out to: <path>` on stdout, which doubles
    // the visible confirmation that the step itself already gives.
    // Discard it; failures still surface via the exit status + our
    // anyhow message.
    let status = xcframework_cmd
        .stdout(std::process::Stdio::null())
        .status()
        .context("spawn xcodebuild -create-xcframework")?;
    if !status.success() {
        xcf_step.fail(format!("{status}"));
        return Err(anyhow!("xcodebuild -create-xcframework failed ({status})"));
    }
    xcf_step.done("");
    Ok(xcf)
}

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
    /// `Release` for `whisker build`; `Debug` is unused today but
    /// kept generic so Phase 3 can reuse this for `whisker run`'s
    /// initial build.
    pub configuration: &'a str,
    /// Almost always `<scheme>.xcodeproj` (XcodeGen output). Tests
    /// override it to point at fixtures.
    pub xcodeproj_name: &'a str,
    /// `-derivedDataPath` value. Picked by the caller so the gen
    /// tree stays drift-free for the next sync.
    pub derived_data: &'a Path,
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

    let status = Command::new("xcodebuild")
        .arg("-project")
        .arg(&project)
        .args(["-scheme", args.scheme])
        .args(["-configuration", args.configuration])
        .args(["-destination", &destination])
        .arg("-derivedDataPath")
        .arg(args.derived_data)
        .args(["-quiet", "build"])
        .status()
        .context("spawn xcodebuild")?;
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
        };
        let err = run_xcodebuild_app(&args).unwrap_err();
        assert!(err.to_string().contains("unknown SDK"), "got: {err:#}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
