//! `whisker-build` binary — the Xcode Run Script Phase / Gradle
//! plugin entry point.
//!
//! The library half of this crate (re-exported via `src/lib.rs`)
//! stays the canonical Rust API for `whisker-cli` and
//! `whisker-dev-server`. This binary is a thin arg-parse shim that
//! routes Xcode / Gradle environment values into the same lib
//! functions, so the same orchestration logic powers both the
//! external CLI ("whisker run", "whisker build") and the IDE
//! standalone path ("Cmd+B in Xcode" / "Sync now in Android
//! Studio").
//!
//! ## Invocation shape
//!
//! From an Xcode Run Script Phase:
//!
//! ```sh
//! whisker-build ios \
//!     --workspace="$SRCROOT/.." \
//!     --configuration="$CONFIGURATION" \
//!     --platform="$PLATFORM_NAME" \
//!     --archs="$ARCHS" \
//!     --built-products-dir="$BUILT_PRODUCTS_DIR" \
//!     --package="$WHISKER_PACKAGE"
//! ```
//!
//! From the Gradle plugin's `cargoBuildDebug` / `cargoBuildRelease`
//! task:
//!
//! ```sh
//! whisker-build android \
//!     --workspace="$rootDir/.." \
//!     --profile=debug \
//!     --abi=arm64-v8a \
//!     --jni-libs-dir="$projectDir/src/main/jniLibs" \
//!     --package="$WHISKER_PACKAGE"
//! ```
//!
//! ## Responsibilities
//!
//! 1. Resolve Xcode / Gradle env to whisker-build lib inputs
//!    (`Profile`, `AndroidToolchain`, target triples).
//! 2. Discover whisker modules through the lib's
//!    [`whisker_build::modules::discover`].
//! 3. Drive cargo cross-compile + per-platform autolinking aux
//!    generation through the lib's existing
//!    `android::cargo_build_dylib` /
//!    `ios::build_framework_for_xcode_run_script` /
//!    `*::stage_module_*_sources` helpers.
//! 4. Place the resulting binary in the location Xcode / Gradle
//!    expects (`$BUILT_PRODUCTS_DIR/Frameworks/...` /
//!    `jniLibs/<abi>/lib*.so`).
//!
//! Step 2 of the build-system migration only wires up the CLI
//! surface + module discovery; Steps 4–5 fill in the actual cargo
//! cross-compile + artefact placement once the cng templates start
//! invoking this binary.

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use whisker_build::Profile;

#[derive(Parser)]
#[command(
    name = "whisker-build",
    version,
    about = "Cargo cross-compile + module autolinking shim invoked by Xcode Run Script Phase / Gradle plugin",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// iOS dispatch — called from a Run Script Build Phase the
    /// whisker-cng-rendered `project.pbxproj` carries.
    Ios(IosArgs),

    /// Android dispatch — called from the whisker-gradle-plugin's
    /// `cargoBuildDebug` / `cargoBuildRelease` task.
    Android(AndroidArgs),

    /// Discovery dispatch — emits a JSON manifest of every Whisker
    /// module the user app depends on. Consumed by the Gradle
    /// Settings Plugin (`rs.whisker`) at Initialization phase to
    /// `include(":...")` each module's Android subproject, and by
    /// the SwiftPM Build Tool Plugin to generate Swift wrappers.
    ///
    /// Pure read-only — no cargo build, no NDK touch. Fast enough
    /// (~100ms cold cargo metadata) that the plugin can call it on
    /// every Sync; consumers still cache by `Cargo.lock` hash for
    /// the warm path.
    Modules(ModulesArgs),
}

/// Inputs the Xcode Run Script Phase passes to the binary. Mirrors
/// the Xcode environment variables verbatim so the script glue
/// stays one shell line.
#[derive(Args)]
struct IosArgs {
    /// Workspace root containing the user app's top-level `Cargo.toml`
    /// (the one with `[workspace]`). Typically `"$SRCROOT/.."` when
    /// called from the gen/ios Xcode project.
    #[arg(long)]
    workspace: PathBuf,

    /// Cargo package name (the user app crate). Matches what
    /// whisker-cng renders into the pbxproj template — passed
    /// rather than re-discovered to keep the binary deterministic
    /// when multiple workspace members exist.
    #[arg(long)]
    package: String,

    /// Xcode `CONFIGURATION` (`Debug` or `Release`).
    #[arg(long)]
    configuration: String,

    /// Xcode `PLATFORM_NAME` (`iphoneos` or `iphonesimulator`).
    #[arg(long)]
    platform: String,

    /// Xcode `ARCHS` — one or more space-separated architectures
    /// (`arm64`, `x86_64`). The binary cross-compiles each
    /// requested arch then lipo-merges them into a single fat
    /// dylib for the simulator destination, or hands the device
    /// slice through verbatim.
    #[arg(long)]
    archs: String,

    /// Xcode `BUILT_PRODUCTS_DIR`. The dylib lands at
    /// `<dir>/Frameworks/Whisker.framework/Whisker` so Xcode's
    /// embed-frameworks build phase picks it up automatically.
    #[arg(long)]
    built_products_dir: PathBuf,
}

#[derive(Args)]
struct AndroidArgs {
    /// Workspace root.
    #[arg(long)]
    workspace: PathBuf,

    /// Cargo package name (the user app crate).
    #[arg(long)]
    package: String,

    /// Gradle build type (`debug` or `release`).
    #[arg(long)]
    profile: String,

    /// Target ABI — gradle passes one of `arm64-v8a` / `armeabi-v7a`
    /// / `x86_64` / `x86` per `splits.abi` config. The binary
    /// resolves the matching Rust target triple via
    /// [`whisker_build::android::abi_to_triple`].
    #[arg(long)]
    abi: String,

    /// Where to place the resulting `.so`. AGP's default layout is
    /// `<project>/src/main/jniLibs/<abi>/lib<package>.so`; the
    /// gradle plugin computes that path and passes it.
    #[arg(long)]
    jni_libs_dir: PathBuf,

    /// Android `minSdkVersion`. The NDK toolchain lookup
    /// ([`whisker_build::android::resolve_toolchain`]) needs the
    /// API level to pick the right sysroot binaries.
    #[arg(long, default_value = "24")]
    min_sdk: u32,
}

/// Inputs for the `modules` discovery subcommand. Workspace + app
/// crate name are enough — there's no platform context here because
/// the JSON carries per-platform availability flags inline.
#[derive(Args)]
struct ModulesArgs {
    /// Workspace root containing the user app's top-level `Cargo.toml`.
    #[arg(long)]
    workspace: PathBuf,

    /// User app crate name. Discovery walks the cargo dep graph
    /// rooted at this package.
    #[arg(long)]
    package: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Ios(args) => run_ios(canonicalize_ios_args(args)?),
        Cmd::Android(args) => run_android(canonicalize_android_args(args)?),
        Cmd::Modules(args) => run_modules(canonicalize_modules_args(args)?),
    }
}

/// Resolve the workspace path to its canonical form (`..` collapsed, symlinks
/// resolved) before anything downstream consumes it. Without this, two
/// invocations with logically-equivalent but textually-different workspace
/// paths bake different absolute paths into the cng-rendered files
/// (`gen/ios/whisker_modules/Package.swift`, the pbxproj's Run Script
/// substitution) — and SPM's `.package(path:)` does a byte-for-byte string
/// compare for identity, so a mismatch against the module-side fallback
/// `URL.standardized.path` splits the same package into two identities and
/// breaks the SwiftPM build-tool-plugin dependency chain (Step 7's late
/// blocker).
///
/// `whisker-cli` already canonicalizes via its `find_workspace_root`, but
/// nothing forces a caller (a hand-edited Run Script, a CI YAML, a future
/// IDE driver) to come through that path. Belt-and-braces.
fn canonicalize_workspace(p: PathBuf) -> Result<PathBuf> {
    std::fs::canonicalize(&p).with_context(|| format!("canonicalize workspace {}", p.display()))
}

fn canonicalize_ios_args(mut args: IosArgs) -> Result<IosArgs> {
    args.workspace = canonicalize_workspace(args.workspace)?;
    Ok(args)
}

fn canonicalize_android_args(mut args: AndroidArgs) -> Result<AndroidArgs> {
    args.workspace = canonicalize_workspace(args.workspace)?;
    Ok(args)
}

fn canonicalize_modules_args(mut args: ModulesArgs) -> Result<ModulesArgs> {
    args.workspace = canonicalize_workspace(args.workspace)?;
    Ok(args)
}

fn run_modules(args: ModulesArgs) -> Result<()> {
    let report = whisker_build::modules::build_modules_report(&args.workspace, &args.package)
        .with_context(|| {
            format!(
                "build modules report for `{}` (workspace={})",
                args.package,
                args.workspace.display(),
            )
        })?;
    // Pretty-print so a human inspecting the cache file can read it;
    // the Gradle plugin parses either form fine.
    let json = serde_json::to_string_pretty(&report).context("serialize modules report")?;
    println!("{json}");
    Ok(())
}

fn run_ios(args: IosArgs) -> Result<()> {
    // Lynx iOS xcframework needs to be on disk before the bridge cc
    // build resolves `<staged>/Lynx/...` includes. Mirrors the
    // Android path's `ensure_lynx_android()`.
    let _cache = whisker_build::ensure_lynx_ios().context("fetch pinned Lynx iOS artifacts")?;
    whisker_build::link_lynx_into_workspace(&args.workspace, whisker_build::LynxPlatform::Ios)
        .context("symlink target/lynx-ios into workspace")?;

    let archs: Vec<&str> = args.archs.split_whitespace().collect();
    let fw = whisker_build::ios::build_framework_for_xcode_run_script(
        &whisker_build::ios::XcodeRunScriptInputs {
            workspace_root: &args.workspace,
            package: &args.package,
            platform: &args.platform,
            archs: &archs,
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

    // `configuration` is currently informational — the iOS cargo
    // build is always `--release` (matches what `cargo_build_ios_dylib`
    // already pins for the xcframework path; subsecond's Tier 1
    // capture wants the same optimised codegen prod ships). Logged
    // here so a Debug-mode Xcode build that's surprised by
    // release-tier optimisation has the mismatch visible.
    eprintln!(
        "[whisker-build ios] published {} (configuration={}, archs=[{}])",
        fw.display(),
        args.configuration,
        args.archs,
    );
    Ok(())
}

fn run_android(args: AndroidArgs) -> Result<()> {
    let cargo_toml = args.workspace.join("Cargo.toml");
    let modules = whisker_build::modules::discover(&cargo_toml, &args.package)
        .with_context(|| format!("discover whisker modules in {}", cargo_toml.display()))?;

    let profile = parse_profile(&args.profile)?;

    // `whisker-driver-sys`'s build.rs reads Lynx Android headers +
    // .so from `<workspace>/target/lynx-android*`. Fetch the pinned
    // tarball + create the symlinks before cargo runs so the cc-rs
    // include search finds Lynx without a pre-existing whisker CLI
    // bootstrap.
    let _cache =
        whisker_build::ensure_lynx_android().context("fetch pinned Lynx Android artifacts")?;
    whisker_build::link_lynx_into_workspace(&args.workspace, whisker_build::LynxPlatform::Android)
        .context("symlink target/lynx-android* into workspace")?;

    let toolchain = whisker_build::android::resolve_toolchain(&args.abi, args.min_sdk)
        .with_context(|| {
            format!(
                "resolve NDK toolchain for {} (api {})",
                args.abi, args.min_sdk
            )
        })?;

    let so_path = whisker_build::android::cargo_build_dylib(&whisker_build::android::CargoBuild {
        workspace_root: &args.workspace,
        package: &args.package,
        toolchain: &toolchain,
        profile,
        features: &[],
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
        "[whisker-build android] {} module(s) discovered (gradle-subproject wiring is the Gradle plugin's job)",
        modules.len(),
    );
    Ok(())
}

/// Translate the `--profile` string the Gradle plugin (or CLI caller)
/// passes into the typed [`Profile`] the library API expects. The
/// plugin currently emits exactly `"debug"` / `"release"` so the
/// match is closed; any other value is a wiring bug worth surfacing.
fn parse_profile(s: &str) -> Result<Profile> {
    match s {
        "debug" => Ok(Profile::Debug),
        "release" => Ok(Profile::Release),
        other => Err(anyhow!(
            "--profile must be 'debug' or 'release' (got `{other}`)"
        )),
    }
}
