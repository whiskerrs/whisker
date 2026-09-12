//! Compatibility adapter from development targets to CNG generation.

use anyhow::Result;
use std::path::Path;
use whisker_config::Config;
use whisker_dev_server::Target;

pub use whisker_cng::PlatformSync;

pub fn generation_target(target: Target) -> whisker_cng::GenerationTarget {
    match target {
        Target::Android => whisker_cng::GenerationTarget::Android,
        Target::IosSimulator => whisker_cng::GenerationTarget::Ios,
        Target::Macos => whisker_cng::GenerationTarget::Macos,
        Target::Web => whisker_cng::GenerationTarget::Web,
    }
}

pub fn sync_for_target(
    target: Target,
    config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    whisker_cng::sync_for_target(
        generation_target(target),
        config,
        crate_dir,
        workspace_root,
        package,
    )
}
