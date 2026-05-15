//! Build the user crate as an iOS xcframework that the WhiskerRuntime
//! SPM target consumes.
//!
//! Slices produced (each as a Mach-O `.framework` containing a dylib):
//! - `ios-arm64` (real device — Tier 1 hot-reload does NOT work here
//!   because amfid blocks `dlopen` of unsigned dylibs, but cold rebuild
//!   and release distribution still need this slice)
//! - `ios-arm64_x86_64-simulator` (lipo'd arm64-sim + x86_64-sim — this
//!   is where `whisker run --target ios --hot-patch` aims)
//!
//! Output shape (each slice):
//! ```text
//! WhiskerDriver.xcframework/<slice>/WhiskerDriver.framework/
//!   ├── WhiskerDriver        (Mach-O dylib, install_name=@rpath/WhiskerDriver.framework/WhiskerDriver)
//!   ├── Headers/{whisker.h, whisker_bridge.h}
//!   ├── Modules/module.modulemap   (framework module form)
//!   └── Info.plist                  (minimal CFBundle* required by codesign + dyld)
//! ```
//!
//! Why dylib (not staticlib): subsecond's hot-patch model needs a
//! separate dynamic library it can read symbols from at build time and
//! `dlopen` patches against at runtime. Matches the Android `dylib`
//! convention. See `docs/hot-reload-plan.md` "Second Pivot".

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

const FRAMEWORK_NAME: &str = "WhiskerDriver";

#[derive(clap::Args)]
pub struct Args {
    /// User crate (the one with `#[whisker::main]`). Its dylib is
    /// `lib<package_underscored>.dylib`. Default: hello-world.
    #[arg(short = 'p', long, default_value = "hello-world")]
    pub package: String,

    /// Output directory. Default: `target/whisker-driver/`.
    #[arg(long)]
    pub out_dir: Option<PathBuf>,

    /// Cargo features forwarded to the dylib build for every iOS
    /// triple. `whisker run` uses this to pass `whisker/hot-reload`.
    #[arg(long)]
    pub features: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    let root = paths::workspace_root();
    let out = args.out_dir.unwrap_or_else(paths::whisker_driver_out);
    let lib_stem = args.package.replace('-', "_");
    let cargo_dylib_name = format!("lib{lib_stem}.dylib");

    let headers_src = root.join("crates/whisker-driver/include");
    let bridge_headers_src = paths::bridge_include();
    for required in ["whisker.h", "module.modulemap"] {
        if !headers_src.join(required).is_file() {
            anyhow::bail!(
                "missing header {} (expected at {})",
                required,
                headers_src.display()
            );
        }
    }
    if !bridge_headers_src.join("whisker_bridge.h").is_file() {
        anyhow::bail!(
            "missing whisker_bridge.h (expected at {})",
            bridge_headers_src.display()
        );
    }

    println!("==> Cleaning {}", out.display());
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    std::fs::create_dir_all(&out)?;

    let triples = [
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
    ];
    println!("==> Building Rust dylibs (user crate: {})", args.package);
    for triple in triples {
        println!("    -- {triple}");
        cargo_build(&args.package, triple, &root, &args.features)?;
    }

    let target_dir = paths::target_dir();
    let device_dylib = target_dir
        .join("aarch64-apple-ios/release")
        .join(&cargo_dylib_name);
    let sim_arm64_dylib = target_dir
        .join("aarch64-apple-ios-sim/release")
        .join(&cargo_dylib_name);
    let sim_x86_dylib = target_dir
        .join("x86_64-apple-ios/release")
        .join(&cargo_dylib_name);
    for p in [&device_dylib, &sim_arm64_dylib, &sim_x86_dylib] {
        if !p.is_file() {
            anyhow::bail!("expected dylib not built: {}", p.display());
        }
    }

    // Stage the device-slice framework.
    let device_fw_parent = out.join("device");
    let device_fw = build_framework_dir(
        &device_fw_parent,
        &device_dylib,
        &headers_src,
        &bridge_headers_src,
    )?;

    // Lipo the two sim dylibs into one fat binary, then frame it.
    let sim_fat_parent = out.join("sim");
    std::fs::create_dir_all(&sim_fat_parent)?;
    let sim_fat = sim_fat_parent.join(&cargo_dylib_name);
    println!("==> Lipo simulator slices → {}", sim_fat.display());
    let status = Command::new("lipo")
        .args(["-create"])
        .arg(&sim_arm64_dylib)
        .arg(&sim_x86_dylib)
        .args(["-output"])
        .arg(&sim_fat)
        .status()
        .context("failed to spawn lipo")?;
    if !status.success() {
        anyhow::bail!("lipo failed (exit {status})");
    }
    let sim_fw = build_framework_dir(&sim_fat_parent, &sim_fat, &headers_src, &bridge_headers_src)?;

    let xcf = out.join(format!("{FRAMEWORK_NAME}.xcframework"));
    println!("==> Creating xcframework");
    let status = Command::new("xcodebuild")
        .arg("-create-xcframework")
        .args(["-framework"])
        .arg(&device_fw)
        .args(["-framework"])
        .arg(&sim_fw)
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
    println!("==> Staging {}", fw_dir.display());
    if fw_dir.exists() {
        std::fs::remove_dir_all(&fw_dir)?;
    }
    std::fs::create_dir_all(&fw_dir)?;

    // Main binary: copy dylib, rename to `<FRAMEWORK_NAME>` (no extension,
    // no `lib` prefix — Apple's flat-framework convention).
    let binary_dst = fw_dir.join(FRAMEWORK_NAME);
    std::fs::copy(dylib_src, &binary_dst)
        .with_context(|| format!("copy {} → {}", dylib_src.display(), binary_dst.display()))?;

    // Rewrite LC_ID_DYLIB to the @rpath form. The Cargo build sets
    // install_name via `-Wl,-install_name,...` (see
    // `crates/whisker-driver-sys/build.rs`), but we run install_name_tool
    // here as belt-and-suspenders so the lipo'd fat binary and any
    // pre-build-script-flag-less invocation also end up correct.
    let install_name = format!("@rpath/{FRAMEWORK_NAME}.framework/{FRAMEWORK_NAME}");
    let status = Command::new("install_name_tool")
        .args(["-id", &install_name])
        .arg(&binary_dst)
        .status()
        .context("failed to spawn install_name_tool")?;
    if !status.success() {
        anyhow::bail!(
            "install_name_tool failed on {} (exit {status})",
            binary_dst.display()
        );
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

    // Modules/module.modulemap — framework form (`framework module ...`).
    // The repo-level modulemap is a plain `module ...` declaration that
    // suits the old `-library` xcframework shape; the framework xcframework
    // requires the `framework module` keyword so Xcode can `import`-style
    // resolve it.
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

/// `extern "C"` bridge entry points that need to land in the dylib's
/// `.dynsym` so Swift can call them across the framework boundary.
///
/// Keep in sync with the `WHISKER_BRIDGE_EXPORT`-tagged declarations
/// in `crates/whisker-driver-sys/bridge/include/whisker_bridge.h`. If
/// you add a new bridge function there without listing it here, Swift
/// linking will fail with an `Undefined symbols: _<name>` error.
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

fn cargo_build(package: &str, triple: &str, root: &Path, features: &[String]) -> Result<()> {
    // `cargo rustc --crate-type dylib` overrides the manifest's
    // `crate-type` (`["rlib"]`) so the user crate compiles as a Mach-O
    // dynamic library *and* keeps mangled `pub fn` symbols in
    // `.dynsym`. cdylib's default `-Wl,-exported_symbols_list,…`
    // strips those, which would break subsecond patch dispatch — see
    // `docs/hot-reload-plan.md` "Second Pivot" for the Android-side
    // analysis that applies here unchanged.
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
    // Pass-through rustc args go after `--`. We use `-C link-arg=` to
    // append linker flags to the dylib link step. These can't come
    // from `whisker-driver-sys/build.rs` because `cargo:rustc-link-arg`
    // only affects the build of the crate that emits it (an rlib,
    // which has no link step) — the user-crate dylib build is a
    // sibling cargo target and doesn't inherit them.
    cmd.arg("--");
    for sym in BRIDGE_EXPORTS {
        cmd.arg(format!("-Clink-arg=-Wl,-exported_symbol,{sym}"));
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
