//! `cargo xtask android …` subtree — cross-compile + packaging pipeline.

use clap::{Args, Subcommand};

mod build_example;
mod build_lynx_aar;
mod cargo_build;
mod ndk;
mod unpack_lynx;

#[derive(Args)]
pub struct AndroidArgs {
    #[command(subcommand)]
    command: AndroidCommand,
}

#[derive(Subcommand)]
enum AndroidCommand {
    /// Build a Rust crate for an Android ABI (drop-in replacement for
    /// `cargo ndk … build`). Sets up the NDK clang as linker and CC/CXX
    /// for cc-rs, then invokes plain `cargo build`. Pass extra args to
    /// cargo after a literal `--`.
    Cargo(cargo_build::CargoBuildArgs),
    /// Extract `jni/<abi>/*.so` from `target/lynx-android/*.aar` into
    /// `target/lynx-android-unpacked/` so the C++ bridge link step and
    /// example apps can pick them up.
    UnpackLynx(unpack_lynx::UnpackArgs),
    /// End-to-end build of an example app: cargo cdylib + jniLibs
    /// staging + AAR unpack + gradle assembleDebug.
    BuildExample(build_example::BuildExampleArgs),
    /// Patch + gradle-assemble Lynx Android AARs from a local Lynx
    /// source checkout. Output: `target/lynx-android/*.aar`.
    BuildLynxAar(build_lynx_aar::BuildLynxAarArgs),
}

pub fn run(args: AndroidArgs) -> anyhow::Result<()> {
    match args.command {
        AndroidCommand::Cargo(a) => cargo_build::run(a),
        AndroidCommand::UnpackLynx(a) => unpack_lynx::run(a),
        AndroidCommand::BuildExample(a) => build_example::run(a),
        AndroidCommand::BuildLynxAar(a) => build_lynx_aar::run(a),
    }
}
