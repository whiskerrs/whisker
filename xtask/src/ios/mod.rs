//! `cargo xtask ios …` subtree.

use clap::{Args, Subcommand};

mod build_lynx_frameworks;
mod build_xcframework;

#[derive(Args)]
pub struct IosArgs {
    #[command(subcommand)]
    command: IosCommand,
}

#[derive(Subcommand)]
enum IosCommand {
    /// Build the user crate's static libs for iOS device + Simulator
    /// (arm64, arm64-sim, x86_64-sim), lipo the simulator slices, and
    /// wrap into `WhiskerDriver.xcframework`.
    BuildXcframework(build_xcframework::Args),
    /// Build Lynx + PrimJS + LynxBase + LynxServiceAPI as xcframeworks
    /// from the upstream CocoaPods source pods. Used to feed the SPM
    /// binaryTarget chain.
    BuildLynxFrameworks(build_lynx_frameworks::Args),
}

pub fn run(args: IosArgs) -> anyhow::Result<()> {
    match args.command {
        IosCommand::BuildXcframework(a) => build_xcframework::run(a),
        IosCommand::BuildLynxFrameworks(a) => build_lynx_frameworks::run(a),
    }
}
