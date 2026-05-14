//! Drop-in replacement for `cargo ndk … build`.
//!
//! Sets the NDK clang as Rust's linker for the target triple and
//! seeds the matching CC / CXX / AR env vars for `cc-rs`-driven
//! `build.rs` scripts. Then invokes plain `cargo build`. Everything
//! after `--` is forwarded to cargo.
//!
//! Notably we do NOT:
//!
//! - inject extra `-L` search paths (this is what tripped up cargo-ndk
//!   in our setup: it prepended the non-API-versioned sysroot dir
//!   ahead of clang's auto-added API dir, and `-lc` ended up resolving
//!   to the static `libc.a` instead of `libc.so` — silent static
//!   bionic embedding into the dylib),
//! - bundle `libc++_shared.so` (that's the caller's job; for our
//!   hello-world example it lives in `build-android-example.sh`).
//!
//! The intent is "set the bare minimum env so cargo + cc-rs + clang
//! agree on the target, then get out of the way."

use anyhow::{Context, Result};
use std::process::Command;

use super::ndk;
use crate::paths;

#[derive(clap::Args)]
pub struct CargoBuildArgs {
    /// Crate to build (`cargo build -p <package>`).
    #[arg(short = 'p', long)]
    pub package: String,

    /// Android ABI. Maps internally to a Rust target triple.
    #[arg(long, default_value = "arm64-v8a")]
    pub abi: String,

    /// Android API level baked into `--target=<triple><api>` when
    /// clang is invoked.
    #[arg(long, default_value_t = 24)]
    pub api: u32,

    /// Build in release mode (default). Pass `--profile dev` for debug.
    #[arg(long, default_value = "release")]
    pub profile: String,

    /// Cargo features to pass through, repeatable. Example:
    ///   `cargo xtask android cargo -p hello-world --features whisker/hot-reload`
    /// Multiple values supported either as repeated `--features` or
    /// comma-separated; both go straight to cargo's own `--features`.
    #[arg(long)]
    pub features: Vec<String>,

    /// Extra args forwarded to `cargo build` after a literal `--`.
    /// Example: `cargo xtask android cargo -p hello-world -- --verbose`
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub cargo_args: Vec<String>,
}

pub fn run(args: CargoBuildArgs) -> Result<()> {
    let triple = ndk::abi_to_triple(&args.abi)?;
    let tc = ndk::toolchain(&args.abi, args.api)?;

    // Env vars are spelled with underscores in the target triple, and
    // uppercased for the linker var. Both forms are what cargo /
    // cc-rs look for at build time.
    let triple_env = triple.replace('-', "_");
    let triple_upper = triple_env.to_uppercase();

    // `cargo rustc --crate-type dylib` overrides whatever the user
    // crate's manifest declares (plain `rlib` for hello-world, so
    // host `cargo build` doesn't drown in unresolved bridge symbols).
    // This is the symmetric counterpart of
    // `cargo rustc --crate-type staticlib` for iOS.
    //
    // **Why `dylib` and not `cdylib`?** rustc unconditionally injects
    // `-Wl,--exclude-libs,ALL` for `cdylib`, stripping every
    // mangled Rust symbol from `.dynsym`. subsecond hot patches
    // resolve std/alloc/core symbols against the host at `dlopen`
    // time, so an excluded `.dynsym` makes the apply_patch dlopen
    // fail with `cannot locate symbol _ZN4core3fmt…`. rustc does
    // NOT add `--exclude-libs,ALL` to `dylib`, so std/core/alloc and
    // every `pub fn` in the user crate stay visible. The resulting
    // file is still a regular `.so` and `System.loadLibrary` (which
    // doesn't care about ABI flavour) loads it identically. See
    // docs/hot-reload-plan.md "Second Pivot" for the full analysis.
    let mut cmd = Command::new("cargo");
    cmd.arg("rustc")
        .args(["--target", triple])
        .args(["-p", &args.package])
        .args(["--crate-type", "dylib"]);
    match args.profile.as_str() {
        "release" => {
            cmd.arg("--release");
        }
        "dev" => {
            // cargo's default profile — no flag needed.
        }
        other => anyhow::bail!("unsupported profile: {other} (use release or dev)"),
    }
    for feat in &args.features {
        cmd.args(["--features", feat]);
    }
    cmd.args(&args.cargo_args);

    // Append rustc-level link args. We need `Java_*` and `JNI_OnLoad`
    // exported in `.dynsym` so `System.loadLibrary` + the Android
    // JNI runtime can resolve them by `dlsym`. The C++ bridge defines
    // them with default visibility, but rustc auto-generates a
    // version-script for `dylib` that lists Rust-mangled symbols
    // in `global:` and ends with `local: *;` — which demotes the
    // JNI exports from the linked-in static archive to LOCAL.
    //
    // We supply an additional version-script (passed AFTER rustc's
    // own) listing JNI symbols in `global:` only. lld merges multiple
    // anonymous version-scripts additively — a symbol matching any
    // script's `global:` is exported, even if another script's
    // `local: *;` would otherwise hide it. The merge handles JNI
    // exports without touching rustc's Rust-symbol list. The
    // version-script file lives under `target/.whisker/` so it's a
    // discoverable build artifact, not a hidden temp.
    let vs_dir = paths::workspace_root().join("target/.whisker");
    std::fs::create_dir_all(&vs_dir)
        .with_context(|| format!("create version-script dir {}", vs_dir.display()))?;
    let vs_path = vs_dir.join("android-jni-exports.ver");
    std::fs::write(
        &vs_path,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n};\n",
    )
    .with_context(|| format!("write {}", vs_path.display()))?;
    cmd.arg("--").args([
        "-C".to_string(),
        format!("link-arg=-Wl,--version-script={}", vs_path.display()),
    ]);

    // cc-rs honours these for cross compilation.
    cmd.env(format!("CC_{triple_env}"), &tc.clang);
    cmd.env(format!("CXX_{triple_env}"), &tc.clang_cpp);
    cmd.env(format!("AR_{triple_env}"), &tc.ar);
    // cargo uses this to drive the final link. Honour any
    // pre-existing value so callers can interpose a linker shim
    // (Whisker's Tier 1 dev loop does this with whisker-linker-shim,
    // which then forwards to the NDK clang via WHISKER_REAL_LINKER).
    let linker_env = format!("CARGO_TARGET_{triple_upper}_LINKER");
    if std::env::var_os(&linker_env).is_none() {
        cmd.env(&linker_env, &tc.clang);
    }
    // Exposed so build.rs scripts that want to poke at the NDK
    // (e.g. for sysroot paths) can find it without re-implementing
    // version detection.
    cmd.env("ANDROID_NDK_HOME", &tc.ndk);

    println!(
        "==> cargo rustc --crate-type dylib --target {triple} -p {pkg}  (NDK: {ndk})",
        triple = triple,
        pkg = args.package,
        ndk = tc.ndk.display()
    );

    let status = cmd
        .status()
        .with_context(|| format!("failed to spawn `cargo` for target {triple}"))?;
    if !status.success() {
        anyhow::bail!("cargo build failed (exit {status})");
    }
    Ok(())
}
