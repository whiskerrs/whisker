//! `cargo xtask ios …` subtree — Whisker-internal iOS build steps.
//! Currently just Lynx framework wrangling.
//!
//! User-app iOS builds (cargo per-triple + xcframework wrap +
//! xcodebuild) live in `whisker-cli` / `whisker-build` — run
//! `whisker run --target ios` or `whisker build --target ios-sim`.

use clap::{Args, Subcommand};

mod build_lynx_frameworks;

#[derive(Args)]
pub struct IosArgs {
    #[command(subcommand)]
    command: IosCommand,
}

#[derive(Subcommand)]
enum IosCommand {
    /// Build Lynx + PrimJS + LynxBase + LynxServiceAPI as xcframeworks
    /// from the upstream CocoaPods source pods. Used to feed the SPM
    /// binaryTarget chain.
    BuildLynxFrameworks(build_lynx_frameworks::Args),
}

pub fn run(args: IosArgs) -> anyhow::Result<()> {
    match args.command {
        IosCommand::BuildLynxFrameworks(a) => build_lynx_frameworks::run(a),
    }
}
