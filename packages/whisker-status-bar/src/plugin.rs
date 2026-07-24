//! Whisker plugin for the status-bar module.
//!
//! Currently a no-op: the module is **Android-only** (see `runtime.rs`),
//! and Android needs no manifest/config entry to drive the status bar.
//! It previously injected `UIViewControllerBasedStatusBarAppearance =
//! false` for the deprecated app-level iOS API — that API is no longer
//! used (it broke router transitions), so the injection is gone. When
//! iOS is implemented via `WhiskerViewController.prefersStatusBarHidden`,
//! this plugin should instead ensure that key is `true` (its default).
//! Kept as a registered no-op so that future work has a home and
//! `app.plugin::<WhiskerStatusBar>(|c| c)` stays valid.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! use whisker_status_bar::WhiskerStatusBar;
//!
//! app.plugin::<WhiskerStatusBar>(|c| c);
//! ```

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Plugin, PluginConfig};

/// No fields — see this module's doc comment for why.
#[derive(Default, Serialize, Deserialize)]
pub struct WhiskerStatusBarConfig;

impl PluginConfig for WhiskerStatusBarConfig {
    const NAME: &'static str = "whisker-status-bar";
}

/// The plugin the Whisker engine drives, and the namespace for the
/// runtime API (`set_hidden`/`set_style`, defined in `runtime.rs`) —
/// one unit struct serves both roles (Shape 5: no handle to carry).
pub struct WhiskerStatusBar;

impl Plugin for WhiskerStatusBar {
    type Config = WhiskerStatusBarConfig;
    fn apply(
        &self,
        _ctx: &mut GenerateContext,
        _cfg: &WhiskerStatusBarConfig,
    ) -> anyhow::Result<()> {
        // No-op — Android-only module, no config to inject. See module docs.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::IosProjectIr;

    /// The plugin injects nothing today (Android-only, iOS is a TODO).
    #[test]
    fn apply_is_a_no_op() {
        let mut ctx = GenerateContext {
            ios: Some(IosProjectIr::default()),
            ..Default::default()
        };
        WhiskerStatusBar
            .apply(&mut ctx, &WhiskerStatusBarConfig)
            .unwrap();
        assert!(ctx.ios.unwrap().info_plist.is_empty());
    }
}
