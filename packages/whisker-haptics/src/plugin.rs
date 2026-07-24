//! Whisker plugin for the haptics module.
//!
//! Haptic feedback is meaningless without `android.permission.VIBRATE`
//! (a "normal" permission — auto-granted, no runtime dialog), so
//! unlike `whisker-audio`'s config plugin, there's no opt-in flag to
//! spell: opting into the plugin at all means opting into the
//! permission.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! use whisker_haptics::WhiskerHaptics;
//!
//! app.plugin::<WhiskerHaptics>(|c| c);
//! ```

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, Plugin, PluginConfig, Target};

/// No fields — see this module's doc comment for why.
#[derive(Default, Serialize, Deserialize)]
pub struct WhiskerHapticsConfig;

impl PluginConfig for WhiskerHapticsConfig {
    const NAME: &'static str = "whisker-haptics";
}

/// The plugin the Whisker engine drives — either in-process or as a
/// subprocess via the bundled `whisker-haptics-plugin` binary,
/// depending on the engine's plugin pipeline. Also the namespace for
/// the runtime API (`impact`/`selection`/`notification`, defined in
/// `runtime.rs`) — one unit struct serves both roles, unlike
/// `whisker-audio`'s split (`WhiskerAudio` the plugin vs. `Player`
/// the runtime handle), since haptics has no handle/identity to
/// carry, just static methods (Shape 5).
pub struct WhiskerHaptics;

impl Plugin for WhiskerHaptics {
    type Config = WhiskerHapticsConfig;
    fn apply(&self, ctx: &mut GenerateContext, _cfg: &WhiskerHapticsConfig) -> anyhow::Result<()> {
        if let Some(android) = ctx.android.as_mut() {
            android
                .manifest
                .permissions
                .push("android.permission.VIBRATE".into());
            ctx.journal.record(
                WhiskerHapticsConfig::NAME,
                Target::Android,
                "manifest.permissions",
                Operation::ArrayPush { count: 1 },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::AndroidProjectIr;

    #[test]
    fn appends_vibrate_permission() {
        let mut ctx = GenerateContext {
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        };
        WhiskerHaptics
            .apply(&mut ctx, &WhiskerHapticsConfig)
            .unwrap();
        assert_eq!(
            ctx.android.unwrap().manifest.permissions,
            vec!["android.permission.VIBRATE".to_string()],
        );
    }

    #[test]
    fn no_android_target_is_a_no_op() {
        let mut ctx = GenerateContext::default();
        WhiskerHaptics
            .apply(&mut ctx, &WhiskerHapticsConfig)
            .unwrap();
        assert!(ctx.android.is_none());
    }
}
