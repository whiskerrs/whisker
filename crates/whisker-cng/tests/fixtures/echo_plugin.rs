//! Fixture binary for `tests/subprocess.rs`.
//!
//! A minimal 3rd-party-shaped Whisker CNG plugin that pushes its
//! `permission` config string into the Android manifest's
//! permissions list. The test crafts a `PluginRequest` envelope and
//! spawns this binary via `Engine::register_subprocess` to verify
//! the spawn / pipe / merge wiring end-to-end.

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
struct EchoCfg {
    #[serde(default)]
    permission: String,
}

impl PluginConfig for EchoCfg {
    const NAME: &'static str = "fixture-echo-plugin";
}

struct EchoPlugin;

impl Plugin for EchoPlugin {
    type Config = EchoCfg;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &EchoCfg) -> anyhow::Result<()> {
        if let Some(android) = ctx.android.as_mut() {
            if !cfg.permission.is_empty() {
                android.manifest.permissions.push(cfg.permission.clone());
                ctx.journal.record(
                    EchoCfg::NAME,
                    Target::Android,
                    "manifest.permissions",
                    Operation::ArrayPush { count: 1 },
                );
            }
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    whisker_plugin::run_as_subprocess(EchoPlugin)
}
