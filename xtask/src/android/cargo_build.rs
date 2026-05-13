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
//!   bionic embedding into the cdylib),
//! - bundle `libc++_shared.so` (that's the caller's job; for our
//!   hello-world example it lives in `build-android-example.sh`).
//!
//! The intent is "set the bare minimum env so cargo + cc-rs + clang
//! agree on the target, then get out of the way."

use anyhow::{Context, Result};
use std::process::Command;

use super::ndk;

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

    // `cargo rustc --crate-type cdylib` overrides whatever the
    // user crate's manifest declares (plain `rlib` for hello-world,
    // so host `cargo build` doesn't drown in unresolved bridge
    // symbols). This is the symmetric counterpart of
    // `cargo rustc --crate-type staticlib` for iOS.
    let mut cmd = Command::new("cargo");
    cmd.arg("rustc")
        .args(["--target", triple])
        .args(["-p", &args.package])
        .args(["--crate-type", "cdylib"]);
    match args.profile.as_str() {
        "release" => {
            cmd.arg("--release");
        }
        "dev" => {
            // cargo's default profile — no flag needed.
        }
        other => anyhow::bail!("unsupported profile: {other} (use release or dev)"),
    }
    cmd.args(&args.cargo_args);

    // cc-rs honours these for cross compilation.
    cmd.env(format!("CC_{triple_env}"), &tc.clang);
    cmd.env(format!("CXX_{triple_env}"), &tc.clang_cpp);
    cmd.env(format!("AR_{triple_env}"), &tc.ar);
    // cargo uses this to drive the final link.
    cmd.env(format!("CARGO_TARGET_{triple_upper}_LINKER"), &tc.clang);
    // Exposed so build.rs scripts that want to poke at the NDK
    // (e.g. for sysroot paths) can find it without re-implementing
    // version detection.
    cmd.env("ANDROID_NDK_HOME", &tc.ndk);

    println!(
        "==> cargo rustc --crate-type cdylib --target {triple} -p {pkg}  (NDK: {ndk})",
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
