//! Build Lynx + PrimJS + LynxBase + LynxServiceAPI xcframeworks from
//! the upstream CocoaPods source pods.
//!
//! Lynx ships only as source pods on CocoaPods (there are no prebuilt
//! iOS binaries on GitHub Releases), so we set up a tiny "carrier"
//! Xcode project, `pod install` the source pods into it, build for
//! iOS device + Simulator in static-framework form, and lift the
//! resulting frameworks into xcframeworks.
//!
//! Output (under `target/lynx-ios/`):
//!   - `Lynx.xcframework`
//!   - `PrimJS.xcframework`
//!   - `LynxBase.xcframework`
//!   - `LynxServiceAPI.xcframework`
//!   - `sources/<pod>/` — staged C++ headers for the bridge build

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

const LYNX_VERSION: &str = "3.7.0";
const PRIMJS_VERSION: &str = "3.7.0";
const PODS: &[&str] = &["Lynx", "PrimJS", "LynxBase", "LynxServiceAPI"];

#[derive(clap::Args)]
pub struct Args {
    /// Carrier-project build dir. Default: `target/lynx-build`.
    #[arg(long)]
    pub build_dir: Option<PathBuf>,

    /// xcframework output dir. Default: `target/lynx-ios`.
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let build = args
        .build_dir
        .unwrap_or_else(|| paths::target_dir().join("lynx-build"));
    let out = args.out_dir.unwrap_or_else(paths::lynx_ios_root);

    println!("==> Clean");
    for p in [&build, &out] {
        if p.exists() {
            std::fs::remove_dir_all(p)?;
        }
        std::fs::create_dir_all(p)?;
    }

    println!("==> Generate carrier Xcode project");
    write_carrier_sources(&build)?;
    run_in(&build, "xcodegen", &["generate"])?;

    println!("==> pod install");
    write_podfile(&build)?;
    run_in(&build, "pod", &["install", "--repo-update"])?;

    println!("==> Patch upstream podspec bug (HEADER_SEARCH_PATHS / CI-only path)");
    patch_pods_xcconfig(&build)?;

    let common: &[&str] = &[
        "-workspace",
        "LynxCarrier.xcworkspace",
        "-scheme",
        "LynxCarrier",
        "-configuration",
        "Release",
        "SKIP_INSTALL=NO",
        "ONLY_ACTIVE_ARCH=NO",
        "CODE_SIGNING_ALLOWED=NO",
        "CODE_SIGNING_REQUIRED=NO",
        "CODE_SIGN_IDENTITY=",
    ];

    println!("==> Build for iOS device");
    let mut device_args: Vec<&str> = vec!["build"];
    device_args.extend_from_slice(common);
    device_args.extend_from_slice(&[
        "-destination",
        "generic/platform=iOS",
        "-derivedDataPath",
        "build/device",
    ]);
    run_in(&build, "xcodebuild", &device_args)?;

    println!("==> Build for iOS Simulator");
    let mut sim_args: Vec<&str> = vec!["build"];
    sim_args.extend_from_slice(common);
    sim_args.extend_from_slice(&[
        "-destination",
        "generic/platform=iOS Simulator",
        "-derivedDataPath",
        "build/sim",
    ]);
    run_in(&build, "xcodebuild", &sim_args)?;

    let device_dir = build.join("build/device/Build/Products/Release-iphoneos");
    let sim_dir = build.join("build/sim/Build/Products/Release-iphonesimulator");

    println!("==> Create xcframeworks");
    for fw in PODS {
        let dev_fw = device_dir.join(fw).join(format!("{fw}.framework"));
        let sim_fw = sim_dir.join(fw).join(format!("{fw}.framework"));
        if !dev_fw.is_dir() || !sim_fw.is_dir() {
            println!("⚠️  Missing {fw} framework");
            println!("    expected device: {}", dev_fw.display());
            println!("    expected sim:    {}", sim_fw.display());
            continue;
        }
        let xcf_out = out.join(format!("{fw}.xcframework"));
        if xcf_out.exists() {
            std::fs::remove_dir_all(&xcf_out)?;
        }
        let status = Command::new("xcodebuild")
            .args(["-create-xcframework"])
            .args(["-framework"])
            .arg(&dev_fw)
            .args(["-framework"])
            .arg(&sim_fw)
            .args(["-output"])
            .arg(&xcf_out)
            .status()
            .context("xcodebuild -create-xcframework")?;
        if !status.success() {
            anyhow::bail!("xcodebuild failed for {fw}");
        }
        println!("✅ {}", xcf_out.display());
    }

    println!("==> Stage Lynx C++ headers for the bridge target");
    stage_headers(&build)?;

    println!("\n==> Final outputs:");
    if let Ok(entries) = std::fs::read_dir(&out) {
        for entry in entries.flatten() {
            println!("  {}", entry.file_name().to_string_lossy());
        }
    }
    Ok(())
}

fn write_carrier_sources(build: &Path) -> Result<()> {
    let sources = build.join("Sources");
    std::fs::create_dir_all(&sources)?;
    std::fs::write(
        sources.join("AppDelegate.swift"),
        r#"import UIKit
@UIApplicationMain
class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
}
"#,
    )?;
    std::fs::write(
        build.join("project.yml"),
        r#"name: LynxCarrier
options:
  bundleIdPrefix: rs.whisker.carrier
  deploymentTarget:
    iOS: '13.0'
targets:
  LynxCarrier:
    type: application
    platform: iOS
    sources: [Sources]
    info:
      path: Info.plist
      properties:
        UILaunchScreen: {}
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: rs.whisker.carrier.LynxCarrier
"#,
    )?;
    Ok(())
}

fn write_podfile(build: &Path) -> Result<()> {
    let podfile = format!(
        "platform :ios, '13.0'\n\
         use_frameworks! :linkage => :static\n\
         target 'LynxCarrier' do\n\
         \x20\x20pod 'Lynx', '{lynx}'\n\
         \x20\x20pod 'PrimJS', '{primjs}', :subspecs => ['quickjs', 'napi']\n\
         end\n",
        lynx = LYNX_VERSION,
        primjs = PRIMJS_VERSION,
    );
    std::fs::write(build.join("Podfile"), podfile)?;
    Ok(())
}

/// Lynx 3.7.0's xcconfigs ship with HEADER_SEARCH_PATHS pointing at
/// `/Users/runner/work/lynx/lynx/lynx` (the GitHub Actions runner
/// path used during release). For LynxServiceAPI it's the *only*
/// search path, so the build fails outright. Rewrite to
/// `${PODS_TARGET_SRCROOT}` so headers resolve against locally
/// extracted pod sources.
fn patch_pods_xcconfig(build: &Path) -> Result<()> {
    let pods = build.join("Pods");
    if !pods.is_dir() {
        anyhow::bail!("Pods/ not generated — did `pod install` succeed?");
    }
    const NEEDLE: &str = "/Users/runner/work/lynx/lynx/lynx";
    const REPLACEMENT: &str = "${PODS_TARGET_SRCROOT}";
    for entry in walkdir::WalkDir::new(&pods)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("xcconfig") {
            continue;
        }
        let body = std::fs::read_to_string(p)
            .with_context(|| format!("read {}", p.display()))?;
        if body.contains(NEEDLE) {
            let patched = body.replace(NEEDLE, REPLACEMENT);
            std::fs::write(p, patched)
                .with_context(|| format!("rewrite {}", p.display()))?;
        }
    }
    Ok(())
}

/// Lynx's framework PrivateHeaders are flattened, but the headers
/// themselves include each other via tree-relative paths like
/// `core/...`, `base/...`. Copy every pod's source tree (headers
/// only) into `target/lynx-headers/<pod>/` with directory structure
/// preserved so the bridge target's header search paths can use
/// them directly. This output is OS-neutral — Android cc::Build
/// reads from the same tree.
fn stage_headers(build: &Path) -> Result<()> {
    let sources_root = paths::lynx_staged_headers();
    if sources_root.exists() {
        std::fs::remove_dir_all(&sources_root)?;
    }
    std::fs::create_dir_all(&sources_root)?;
    for pod in PODS {
        let pod_src = build.join("Pods").join(pod);
        if !pod_src.is_dir() {
            continue;
        }
        let pod_dst = sources_root.join(pod);
        let mut count = 0_usize;
        for entry in walkdir::WalkDir::new(&pod_src)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            if p.extension().and_then(|s| s.to_str()) != Some("h") {
                continue;
            }
            let rel = p.strip_prefix(&pod_src).unwrap();
            let dst = pod_dst.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(p, &dst)?;
            count += 1;
        }
        println!("    {pod}: {count} header(s)");
    }
    Ok(())
}

fn run_in(dir: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to spawn `{cmd}`"))?;
    if !status.success() {
        anyhow::bail!("{cmd} failed (exit {status})");
    }
    Ok(())
}
