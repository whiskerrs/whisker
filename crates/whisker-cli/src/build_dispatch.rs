//! Internal build-tool dispatch — the entry points the **generated
//! native projects** invoke to cross-compile the user's Rust crate
//! into the platform artifact (iOS `WhiskerDriver.framework` /
//! Android `lib*.so`) and to discover Whisker modules.
//!
//! These used to be a separate `whisker-build` binary. Folding them
//! into the `whisker` CLI (which already links `whisker-build` as a
//! library) means **`cargo install whisker-cli` is the only install
//! needed** — there is no second binary to put on `PATH`.
//!
//! Invocation shape (hidden subcommands, not for humans):
//!
//! ```sh
//! # Xcode Run Script Phase (gen/ios/.../project.pbxproj):
//! whisker build-ios \
//!     --workspace="$WORKSPACE" --package="$PKG" \
//!     --configuration="$CONFIGURATION" --platform="$PLATFORM_NAME" \
//!     --archs="$ARCHS" --built-products-dir="$BUILT_PRODUCTS_DIR"
//!
//! # Gradle cargoBuild task (whisker-gradle-plugin):
//! whisker build-android \
//!     --workspace="$WS" --package="$PKG" --profile=debug \
//!     --abi=arm64-v8a --jni-libs-dir="$DIR" --min-sdk=24
//!
//! # Gradle Settings plugin module discovery → JSON on stdout:
//! whisker modules --workspace="$WS" --package="$PKG"
//! ```

use anyhow::{Context, Result, anyhow};
use clap::Args;
use std::path::PathBuf;
use whisker_build::Profile;

/// Inputs the Xcode Run Script Phase passes through. Mirrors the
/// Xcode environment variables verbatim so the script glue stays one
/// shell line.
#[derive(Args, Debug)]
pub struct IosArgs {
    /// Workspace root containing the user app's top-level `Cargo.toml`.
    #[arg(long)]
    workspace: PathBuf,

    /// Cargo package name (the user app crate). Passed rather than
    /// re-discovered to stay deterministic with multiple workspace
    /// members.
    #[arg(long)]
    package: String,

    /// Xcode `CONFIGURATION` (`Debug` or `Release`).
    #[arg(long)]
    configuration: String,

    /// Xcode `PLATFORM_NAME` (`iphoneos` or `iphonesimulator`).
    #[arg(long)]
    platform: String,

    /// Xcode `ARCHS` — one or more space-separated architectures.
    #[arg(long)]
    archs: String,

    /// Xcode `BUILT_PRODUCTS_DIR`. The framework lands under
    /// `<dir>/Frameworks/` so Xcode's embed phase picks it up.
    #[arg(long)]
    built_products_dir: PathBuf,

    /// Cargo `--features` to forward to the cross-compile. Repeatable.
    /// `whisker run` passes `whisker/hot-reload` here so the user
    /// dylib carries the dev-runtime WebSocket client.
    #[arg(long)]
    features: Vec<String>,
}

/// Inputs the Gradle `cargoBuild*` task passes through.
#[derive(Args, Debug)]
pub struct AndroidArgs {
    /// Workspace root.
    #[arg(long)]
    workspace: PathBuf,

    /// Cargo package name (the user app crate).
    #[arg(long)]
    package: String,

    /// Gradle build type (`debug` or `release`).
    #[arg(long)]
    profile: String,

    /// Target ABI (`arm64-v8a` / `armeabi-v7a` / `x86_64` / `x86`).
    #[arg(long)]
    abi: String,

    /// Where to place the resulting `.so` (`<...>/jniLibs/<abi>/`).
    #[arg(long)]
    jni_libs_dir: PathBuf,

    /// Android `minSdkVersion` — selects the NDK sysroot.
    #[arg(long, default_value = "24")]
    min_sdk: u32,

    /// Cargo `--features` to forward to the cross-compile. Repeatable.
    #[arg(long)]
    features: Vec<String>,
}

/// Inputs for the `modules` discovery subcommand. Workspace + app
/// crate name are enough — the JSON carries per-platform availability
/// flags inline.
#[derive(Args, Debug)]
pub struct ModulesArgs {
    /// Workspace root containing the user app's top-level `Cargo.toml`.
    #[arg(long)]
    workspace: PathBuf,

    /// User app crate name. Discovery walks the cargo dep graph rooted
    /// at this package.
    #[arg(long)]
    package: String,
}

/// Resolve the workspace path to its canonical form (`..` collapsed,
/// symlinks resolved) before anything downstream consumes it.
///
/// Without this, two invocations with logically-equivalent but
/// textually-different workspace paths bake different absolute paths
/// into the cng-rendered files — and SPM's `.package(path:)` does a
/// byte-for-byte string compare for identity, so a mismatch splits the
/// same package into two identities and breaks the SwiftPM build-tool
/// plugin dependency chain.
fn canonicalize_workspace(p: &PathBuf) -> Result<PathBuf> {
    std::fs::canonicalize(p).with_context(|| format!("canonicalize workspace {}", p.display()))
}

pub fn run_modules(args: ModulesArgs) -> Result<()> {
    let workspace = canonicalize_workspace(&args.workspace)?;
    let report = whisker_build::modules::build_modules_report(&workspace, &args.package)
        .with_context(|| {
            format!(
                "build modules report for `{}` (workspace={})",
                args.package,
                workspace.display(),
            )
        })?;
    // Pretty-print so a human inspecting the cache file can read it;
    // the Gradle plugin parses either form fine.
    let json = serde_json::to_string_pretty(&report).context("serialize modules report")?;
    println!("{json}");
    Ok(())
}

pub fn run_ios(args: IosArgs) -> Result<()> {
    let workspace = canonicalize_workspace(&args.workspace)?;
    // No Lynx pre-fetch here. The bridge cc build no longer touches
    // any Lynx header path, and the host xcodebuild invocation
    // resolves Lynx xcframeworks via SPM's `binaryTarget(url:checksum:)`.
    let archs: Vec<&str> = args.archs.split_whitespace().collect();
    let fw = whisker_build::ios::build_framework_for_xcode_run_script(
        &whisker_build::ios::XcodeRunScriptInputs {
            workspace_root: &workspace,
            package: &args.package,
            platform: &args.platform,
            archs: &archs,
            features: &args.features,
        },
        &args.built_products_dir,
    )
    .with_context(|| {
        format!(
            "build framework for ({}/{}) → {}",
            args.platform,
            args.archs,
            args.built_products_dir.display(),
        )
    })?;

    // `configuration` is currently informational — the iOS cargo build
    // is always release-tier (subsecond's Tier 1 capture wants the same
    // optimised codegen prod ships). Logged so a Debug-mode Xcode build
    // surprised by release optimisation has the mismatch visible.
    eprintln!(
        "[whisker build-ios] published {} (configuration={}, archs=[{}])",
        fw.display(),
        args.configuration,
        args.archs,
    );
    Ok(())
}

pub fn run_android(args: AndroidArgs) -> Result<()> {
    let workspace = canonicalize_workspace(&args.workspace)?;
    let cargo_toml = workspace.join("Cargo.toml");
    let modules = whisker_build::modules::discover(&cargo_toml, &args.package)
        .with_context(|| format!("discover whisker modules in {}", cargo_toml.display()))?;

    let profile = parse_profile(&args.profile)?;

    // No Lynx pre-fetch on Android either — the bridge calls into Lynx
    // via `dlopen("liblynx.so")` + `dlsym` at engine-attach time, and
    // gradle pulls `lynx-android.aar` transitively from Maven.
    let toolchain = whisker_build::android::resolve_toolchain(&args.abi, args.min_sdk)
        .with_context(|| {
            format!(
                "resolve NDK toolchain for {} (api {})",
                args.abi, args.min_sdk
            )
        })?;

    let so_path = whisker_build::android::cargo_build_dylib(&whisker_build::android::CargoBuild {
        workspace_root: &workspace,
        package: &args.package,
        toolchain: &toolchain,
        profile,
        features: &args.features,
        capture: None,
    })
    .context("cargo cross-compile for Android")?;

    whisker_build::android::stage_so_files(&args.jni_libs_dir, &so_path, &toolchain, &args.abi)
        .with_context(|| {
            format!(
                "stage .so + libc++_shared.so into {}",
                args.jni_libs_dir.display()
            )
        })?;

    eprintln!(
        "[whisker build-android] {} module(s) discovered (gradle-subproject wiring is the Gradle plugin's job)",
        modules.len(),
    );
    Ok(())
}

/// Translate the `--profile` string the Gradle plugin passes into the
/// typed [`Profile`] the library API expects.
fn parse_profile(s: &str) -> Result<Profile> {
    match s {
        "debug" => Ok(Profile::Debug),
        "release" => Ok(Profile::Release),
        other => Err(anyhow!(
            "--profile must be 'debug' or 'release' (got `{other}`)"
        )),
    }
}
