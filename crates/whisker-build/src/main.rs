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
//!    `android::cargo_build_dylib` / `ios::build_xcframework`
//!    / `*::stage_module_*_sources` helpers.
//! 4. Place the resulting binary in the location Xcode / Gradle
//!    expects (`$BUILT_PRODUCTS_DIR/Frameworks/...` /
//!    `jniLibs/<abi>/lib*.so`).
//!
//! Step 2 of the build-system migration only wires up the CLI
//! surface + module discovery; Steps 4–5 fill in the actual cargo
//! cross-compile + artefact placement once the cng templates start
//! invoking this binary.

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Ios(args) => run_ios(args),
        Cmd::Android(args) => run_android(args),
    }
}

fn run_ios(args: IosArgs) -> Result<()> {
    let cargo_toml = args.workspace.join("Cargo.toml");
    let modules = whisker_build::modules::discover(&cargo_toml, &args.package)
        .with_context(|| format!("discover whisker modules in {}", cargo_toml.display()))?;

    eprintln!(
        "[whisker-build ios] workspace={} package={} config={} platform={} archs=[{}] modules={}",
        args.workspace.display(),
        args.package,
        args.configuration,
        args.platform,
        args.archs,
        modules.len(),
    );
    eprintln!(
        "[whisker-build ios] built-products-dir={}",
        args.built_products_dir.display(),
    );

    // Step 4 will fill in:
    //   - cargo rustc --target=<triple> --crate-type=cdylib per
    //     requested arch
    //   - lipo of the simulator slices when ARCHS contains both
    //     arm64 + x86_64
    //   - generate whisker_modules/Package.swift +
    //     RegisterAll.swift via ios::stage_module_swift_sources
    //   - copy the dylib into
    //     $BUILT_PRODUCTS_DIR/Frameworks/Whisker.framework/
    // Today the binary just validates the arg surface and exercises
    // module discovery so the cng-rendered pbxproj has something
    // safe to invoke during a build.
    eprintln!("[whisker-build ios] cargo cross-compile + module aux placement wired in Step 4",);

    Ok(())
}

fn run_android(args: AndroidArgs) -> Result<()> {
    let cargo_toml = args.workspace.join("Cargo.toml");
    let modules = whisker_build::modules::discover(&cargo_toml, &args.package)
        .with_context(|| format!("discover whisker modules in {}", cargo_toml.display()))?;

    let triple = whisker_build::android::abi_to_triple(&args.abi)
        .with_context(|| format!("unrecognised ABI `{}`", args.abi))?;

    eprintln!(
        "[whisker-build android] workspace={} package={} profile={} abi={} triple={} modules={}",
        args.workspace.display(),
        args.package,
        args.profile,
        args.abi,
        triple,
        modules.len(),
    );
    eprintln!(
        "[whisker-build android] jni-libs-dir={} min-sdk={}",
        args.jni_libs_dir.display(),
        args.min_sdk,
    );

    // Step 4 will fill in:
    //   - android::resolve_toolchain(abi, min_sdk) → NDK paths
    //   - android::cargo_build_dylib(&CargoBuild { ... })
    //   - android::stage_jni_libs(jni_libs_dir, abi, so_path, &toolchain)
    //   - whisker_modules.settings.gradle.kts +
    //     whisker_module_deps.gradle.kts emission so the gradle
    //     plugin's `apply(from = ...)` references resolve
    // Today the binary just validates the arg surface + ABI mapping
    // + module discovery.
    eprintln!("[whisker-build android] cargo cross-compile + jniLibs staging wired in Step 4",);

    Ok(())
}
