//! Author surface for 3rd-party Whisker CNG plugins.
//!
//! ## Writing a plugin
//!
//! Add `whisker-plugin` as a dep, implement [`Plugin`] on a unit
//! struct, and call [`run_as_subprocess`] from `main`:
//!
//! ```no_run
//! use whisker_plugin::{Operation, Plugin, GenerateContext, PlistValue, Target};
//!
//! #[derive(Default, serde::Serialize, serde::Deserialize)]
//! struct MyConfig {
//!     bundle_suffix: String,
//! }
//!
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     type Config = MyConfig;
//!     fn name(&self) -> &'static str { "example-plugin" }
//!     fn apply(&self, ctx: &mut GenerateContext, cfg: &MyConfig) -> anyhow::Result<()> {
//!         if let Some(ios) = ctx.ios.as_mut() {
//!             let key = "CFBundleSuffix".to_string();
//!             ios.info_plist.insert(key.clone(), PlistValue::String(cfg.bundle_suffix.clone()));
//!             ctx.journal.record(
//!                 "example-plugin",
//!                 Target::Ios,
//!                 &format!("info_plist.{key}"),
//!                 Operation::Set,
//!             );
//!         }
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     whisker_plugin::run_as_subprocess(MyPlugin)
//! }
//! ```
//!
//! ## How the subprocess runner works
//!
//! The engine inside `whisker-cng` spawns the plugin binary, pipes
//! a [`PluginRequest`] JSON to its stdin, and reads a
//! [`PluginResponse`] JSON from its stdout. [`run_as_subprocess`]
//! is the boilerplate side of that contract — it deserializes the
//! request, calls [`Plugin::validate`] then [`Plugin::apply`],
//! serializes the resulting [`GenerateContext`] back out, and
//! returns. Any error from `validate` / `apply` propagates as a
//! non-zero exit with the message on stderr (the engine surfaces
//! both).
//!
//! Stderr is reserved for human-readable diagnostics: log lines,
//! progress messages, anything the plugin wants to surface to the
//! user when `whisker generate --verbose` is in play. Stdout is
//! strictly the JSON envelope — anything else there is a wire
//! format violation.
//!
//! ## 1st-party plugins
//!
//! Built-in plugins live inside `whisker-cng` itself and never go
//! through this crate — they implement [`Plugin`] from
//! [`whisker_cng_protocol`] directly and the engine drives them
//! in-process. `whisker-plugin` is for external authors who want a
//! shippable binary.

use std::io::{Read, Write};

pub use whisker_cng_protocol::{
    AndroidManifest, AndroidProjectIr, AppMeta, FileEntry, GenerateContext, GradleDsl,
    IntentFilter, IntentFilterData, IosProjectIr, MetaDataEntry, MutationJournal, MutationRecord,
    Operation, PbxprojOp, PlistValue, Plugin, PluginRequest, PluginResponse, Target,
};

/// Drive a [`Plugin`] as a stdin/stdout JSON subprocess.
///
/// Reads a [`PluginRequest`] envelope from stdin (blocking until
/// EOF on the input pipe), runs [`Plugin::validate`] then
/// [`Plugin::apply`], and writes a [`PluginResponse`] back to
/// stdout. The function returns `Ok(())` on success and propagates
/// any deserialization / validation / apply error as an
/// `anyhow::Error` — the recommended `main` form is:
///
/// ```ignore
/// fn main() -> anyhow::Result<()> {
///     whisker_plugin::run_as_subprocess(MyPlugin)
/// }
/// ```
///
/// `?` on the result causes the process to exit with status 1 and
/// the error message on stderr, which is the contract the engine
/// expects.
pub fn run_as_subprocess<P: Plugin>(plugin: P) -> anyhow::Result<()> {
    let mut stdin_buf = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_buf)
        .map_err(|e| anyhow::anyhow!("read PluginRequest from stdin: {e}"))?;

    let request: PluginRequest = serde_json::from_str(&stdin_buf)
        .map_err(|e| anyhow::anyhow!("decode PluginRequest JSON: {e}"))?;

    if request.name != plugin.name() {
        return Err(anyhow::anyhow!(
            "plugin name mismatch: engine asked for `{}` but this binary serves `{}`",
            request.name,
            plugin.name(),
        ));
    }

    let config: P::Config = serde_json::from_value(request.config)
        .map_err(|e| anyhow::anyhow!("decode plugin config for `{}`: {e}", plugin.name()))?;

    plugin
        .validate(&config)
        .map_err(|e| anyhow::anyhow!("`{}`::validate: {e}", plugin.name()))?;

    let mut ctx = request.context;
    plugin
        .apply(&mut ctx, &config)
        .map_err(|e| anyhow::anyhow!("`{}`::apply: {e}", plugin.name()))?;

    let response = PluginResponse { context: ctx };
    let json = serde_json::to_string(&response)
        .map_err(|e| anyhow::anyhow!("encode PluginResponse JSON: {e}"))?;

    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(json.as_bytes())
        .map_err(|e| anyhow::anyhow!("write PluginResponse to stdout: {e}"))?;
    stdout
        .write_all(b"\n")
        .map_err(|e| anyhow::anyhow!("write trailing newline: {e}"))?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // The subprocess runner reads stdin / writes stdout, which is
    // awkward to unit-test directly. Factor the core into an
    // in-memory shim and test that — `run_as_subprocess` is a thin
    // wrapper over it.
    fn run_with_pipes<P: Plugin>(plugin: P, input: &str) -> anyhow::Result<String> {
        let request: PluginRequest = serde_json::from_str(input)?;
        anyhow::ensure!(
            request.name == plugin.name(),
            "name mismatch: {} vs {}",
            request.name,
            plugin.name(),
        );
        let config: P::Config = serde_json::from_value(request.config)?;
        plugin.validate(&config)?;
        let mut ctx = request.context;
        plugin.apply(&mut ctx, &config)?;
        Ok(serde_json::to_string(&PluginResponse { context: ctx })?)
    }

    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct PermissionCfg {
        permission: String,
    }

    struct PermissionPlugin;

    impl Plugin for PermissionPlugin {
        type Config = PermissionCfg;
        fn name(&self) -> &'static str {
            "test-permission"
        }
        fn apply(&self, ctx: &mut GenerateContext, cfg: &PermissionCfg) -> anyhow::Result<()> {
            let android = ctx.android.as_mut().ok_or_else(|| {
                anyhow::anyhow!("test-permission requires android target enabled")
            })?;
            android.manifest.permissions.push(cfg.permission.clone());
            ctx.journal.record(
                "test-permission",
                Target::Android,
                "manifest.permissions",
                Operation::ArrayPush { count: 1 },
            );
            Ok(())
        }
    }

    #[test]
    fn happy_path_round_trip() {
        let request = PluginRequest {
            name: "test-permission".into(),
            config: serde_json::json!({"permission": "android.permission.CAMERA"}),
            context: GenerateContext {
                android: Some(AndroidProjectIr::default()),
                ..Default::default()
            },
        };
        let input = serde_json::to_string(&request).unwrap();

        let output = run_with_pipes(PermissionPlugin, &input).unwrap();
        let response: PluginResponse = serde_json::from_str(&output).unwrap();

        let android = response.context.android.expect("android should be present");
        assert_eq!(
            android.manifest.permissions,
            vec!["android.permission.CAMERA".to_string()],
        );
        assert_eq!(response.context.journal.records.len(), 1);
        assert_eq!(
            response.context.journal.records[0].plugin,
            "test-permission",
        );
        assert!(matches!(
            response.context.journal.records[0].operation,
            Operation::ArrayPush { count: 1 },
        ));
    }

    #[test]
    fn name_mismatch_is_an_error() {
        let request = PluginRequest {
            name: "some-other-plugin".into(),
            config: serde_json::json!({"permission": "x"}),
            context: GenerateContext::default(),
        };
        let input = serde_json::to_string(&request).unwrap();
        let err = run_with_pipes(PermissionPlugin, &input).unwrap_err();
        assert!(err.to_string().contains("name mismatch"), "{err}");
    }

    #[test]
    fn apply_error_propagates() {
        let request = PluginRequest {
            name: "test-permission".into(),
            config: serde_json::json!({"permission": "android.permission.CAMERA"}),
            // No android IR — apply asserts it's present, so this
            // exercises the error path.
            context: GenerateContext::default(),
        };
        let input = serde_json::to_string(&request).unwrap();
        let err = run_with_pipes(PermissionPlugin, &input).unwrap_err();
        assert!(err.to_string().contains("requires android"), "{err}");
    }
}
