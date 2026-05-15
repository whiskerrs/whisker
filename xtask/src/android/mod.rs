//! `cargo xtask android …` subtree — Whisker-internal Android build
//! steps. Currently just Lynx AAR wrangling.
//!
//! User-app Android builds (cargo cross-compile + jniLibs staging +
//! gradle assemble) live in `whisker-cli` / `whisker-build` — run
//! `whisker run --target android` or `whisker build --target android`.

use clap::{Args, Subcommand};

mod build_lynx_aar;
mod ndk;
mod unpack_lynx;

#[derive(Args)]
pub struct AndroidArgs {
    #[command(subcommand)]
    command: AndroidCommand,
}

#[derive(Subcommand)]
enum AndroidCommand {
    /// Extract `jni/<abi>/*.so` from `target/lynx-android/*.aar` into
    /// `target/lynx-android-unpacked/` so the C++ bridge link step
    /// can pick them up at build time.
    UnpackLynx(unpack_lynx::UnpackArgs),
    /// Patch + gradle-assemble Lynx Android AARs from a local Lynx
    /// source checkout. Output: `target/lynx-android/*.aar`.
    BuildLynxAar(build_lynx_aar::BuildLynxAarArgs),
}

pub fn run(args: AndroidArgs) -> anyhow::Result<()> {
    match args.command {
        AndroidCommand::UnpackLynx(a) => unpack_lynx::run(a),
        AndroidCommand::BuildLynxAar(a) => build_lynx_aar::run(a),
    }
}
