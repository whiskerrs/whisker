//! Build the user crate as an iOS xcframework that the TuftRuntime
//! SPM target consumes.
//!
//! Slices produced:
//! - `ios-arm64` (real device)
//! - `ios-arm64_x86_64-simulator` (lipo'd arm64-sim + x86_64-sim)

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::paths;

#[derive(clap::Args)]
pub struct Args {
    /// User crate (the one with `#[tuft::main]`). Its static library
    /// is `lib<package_underscored>.a`. Default: hello-world.
    #[arg(short = 'p', long, default_value = "hello-world")]
    pub package: String,

    /// Output directory. Default: `target/tuft-driver/`.
    #[arg(long)]
    pub out_dir: Option<PathBuf>,

    /// Cargo features forwarded to the static-lib build for every
    /// iOS triple. `tuft run` uses this to pass `tuft/hot-reload`.
    #[arg(long)]
    pub features: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    let root = paths::workspace_root();
    let out = args.out_dir.unwrap_or_else(paths::tuft_driver_out);
    let lib_stem = args.package.replace('-', "_");
    let lib_name = format!("lib{lib_stem}.a");

    let headers_src = root.join("crates/tuft-driver/include");
    let bridge_headers_src = paths::bridge_include();
    for required in ["tuft.h", "module.modulemap"] {
        if !headers_src.join(required).is_file() {
            anyhow::bail!(
                "missing header {} (expected at {})",
                required,
                headers_src.display()
            );
        }
    }
    if !bridge_headers_src.join("tuft_bridge.h").is_file() {
        anyhow::bail!(
            "missing tuft_bridge.h (expected at {})",
            bridge_headers_src.display()
        );
    }

    println!("==> Cleaning {}", out.display());
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    std::fs::create_dir_all(&out)?;

    let triples = ["aarch64-apple-ios", "aarch64-apple-ios-sim", "x86_64-apple-ios"];
    println!("==> Building Rust static libs (user crate: {})", args.package);
    for triple in triples {
        println!("    -- {triple}");
        cargo_build(&args.package, triple, &root, &args.features)?;
    }

    let target_dir = paths::target_dir();
    let device_lib = target_dir.join("aarch64-apple-ios/release").join(&lib_name);
    let sim_arm64_lib = target_dir
        .join("aarch64-apple-ios-sim/release")
        .join(&lib_name);
    let sim_x86_lib = target_dir
        .join("x86_64-apple-ios/release")
        .join(&lib_name);
    for p in [&device_lib, &sim_arm64_lib, &sim_x86_lib] {
        if !p.is_file() {
            anyhow::bail!("expected static lib not built: {}", p.display());
        }
    }

    let sim_dir = out.join("sim");
    std::fs::create_dir_all(&sim_dir)?;
    let sim_fat = sim_dir.join(&lib_name);
    println!("==> Lipo simulator slices");
    let status = Command::new("lipo")
        .args(["-create"])
        .arg(&sim_arm64_lib)
        .arg(&sim_x86_lib)
        .args(["-output"])
        .arg(&sim_fat)
        .status()
        .context("failed to spawn lipo")?;
    if !status.success() {
        anyhow::bail!("lipo failed (exit {status})");
    }

    println!("==> Staging headers");
    let hdr_dir = out.join("Headers");
    std::fs::create_dir_all(&hdr_dir)?;
    std::fs::copy(headers_src.join("tuft.h"), hdr_dir.join("tuft.h"))?;
    std::fs::copy(
        bridge_headers_src.join("tuft_bridge.h"),
        hdr_dir.join("tuft_bridge.h"),
    )?;
    std::fs::copy(
        headers_src.join("module.modulemap"),
        hdr_dir.join("module.modulemap"),
    )?;

    let xcf = out.join("TuftDriver.xcframework");
    println!("==> Creating xcframework");
    let status = Command::new("xcodebuild")
        .arg("-create-xcframework")
        .args(["-library"])
        .arg(&device_lib)
        .args(["-headers"])
        .arg(&hdr_dir)
        .args(["-library"])
        .arg(&sim_fat)
        .args(["-headers"])
        .arg(&hdr_dir)
        .args(["-output"])
        .arg(&xcf)
        .status()
        .context("failed to spawn xcodebuild")?;
    if !status.success() {
        anyhow::bail!("xcodebuild -create-xcframework failed (exit {status})");
    }

    println!("\n✅ Created {}", xcf.display());
    Ok(())
}

fn cargo_build(
    package: &str,
    triple: &str,
    root: &std::path::Path,
    features: &[String],
) -> Result<()> {
    // `cargo rustc --crate-type staticlib` overrides the manifest's
    // `crate-type` to build *only* the static library. We need this on
    // iOS because the manifest also declares `cdylib` (for Android),
    // and a cdylib build for iOS would try to fully link — failing
    // because the bridge symbols (compiled into staticlib via
    // build.rs's cc::Build) are not yet wired into a final image.
    let mut cmd = Command::new("cargo");
    cmd.args([
        "rustc",
        "--release",
        "-p",
        package,
        "--target",
        triple,
        "--crate-type",
        "staticlib",
    ]);
    for feat in features {
        cmd.args(["--features", feat]);
    }
    let status = cmd
        .current_dir(root)
        .status()
        .context("failed to spawn cargo")?;
    if !status.success() {
        anyhow::bail!("cargo rustc failed for target {triple} (exit {status})");
    }
    Ok(())
}
