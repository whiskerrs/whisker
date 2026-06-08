//! In-process plugin engine.
//!
//! Takes a registered set of [`Plugin`]s plus a user `AppConfig`,
//! topologically orders the plugins via their `after()` / `before()`
//! constraints, runs each one with its user-supplied config (or the
//! Config's default when the user didn't declare it), and returns
//! the post-pipeline [`GenerateContext`]. The CNG renderer pass
//! (Phase 3) consumes the context to write `gen/{android,ios}/`.
//!
//! ## Scope (Phase 1 PR 3a)
//!
//! Only the in-process case. 3rd-party plugin subprocesses and
//! Cargo-metadata-driven discovery come in follow-up PRs; this
//! module wires the typed-Plugin → erased-trait dispatch path and
//! the ordering / conflict-detection skeleton everything else
//! sits on top of.
//!
//! ## Type erasure
//!
//! [`Plugin`] has an associated `Config` type. Storing different
//! plugins in one collection means erasing it. [`DynPlugin`] is the
//! internal erased trait: `name` / `after` / `before` forward
//! verbatim, while `run` consumes a JSON-encoded config (or `None`
//! for "use the Config's `Default`"), deserializes it into the
//! plugin's typed Config, then drives `validate` + `apply`.
//!
//! `DynPlugin` is `pub(crate)` because callers should always go
//! through [`Engine::register`] — handing them the erased trait
//! invites trying to instantiate a plugin without registering it
//! with the engine, which loses the topo-sort / conflict checks.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use whisker_app_config::AppConfig;
use whisker_plugin::{
    AndroidProjectIr, AppMeta, GenerateContext, IosProjectIr, MutationJournal, MutationRecord,
    Operation, Plugin, PluginRequest, PluginResponse, Target,
};

// ============================================================================
// Public surface
// ============================================================================

/// Which platform targets the current `compose` invocation should
/// produce IRs for. Plugins see `ctx.ios.is_some()` /
/// `ctx.android.is_some()` matching these flags.
///
/// No `Default` impl — "neither target enabled" is almost always a
/// misconfiguration, so callers spell their intent via
/// [`ios_only`](Self::ios_only) / [`android_only`](Self::android_only) /
/// [`both`](Self::both). Construct the literal yourself if you
/// genuinely want a no-op pipeline (e.g. validate-without-build).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnabledTargets {
    pub ios: bool,
    pub android: bool,
}

impl EnabledTargets {
    pub fn ios_only() -> Self {
        Self {
            ios: true,
            android: false,
        }
    }
    pub fn android_only() -> Self {
        Self {
            ios: false,
            android: true,
        }
    }
    pub fn both() -> Self {
        Self {
            ios: true,
            android: true,
        }
    }
}

/// Registry of plugins the engine runs against an [`AppConfig`].
///
/// Holds a homogeneous list of erased plugins regardless of their
/// concrete `Config` type. Construct via [`Engine::new`], add
/// plugins via [`Engine::register`], run via [`Engine::compose`].
#[derive(Default)]
pub struct Engine {
    plugins: Vec<Box<dyn DynPlugin>>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a typed [`Plugin`] with the engine. The engine
    /// retains ownership for the rest of its lifetime; plugins are
    /// run in topologically-sorted order on every `compose` call,
    /// not in registration order.
    pub fn register<P: Plugin + 'static>(&mut self, plugin: P) -> &mut Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// Number of plugins currently registered. Mostly useful for
    /// tests / debug output.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Run the plugin pipeline against `app_config` and return the
    /// resulting [`GenerateContext`]. Steps:
    ///
    /// 1. Build [`AppMeta`] + IR shells (or `None` per
    ///    [`EnabledTargets`]).
    /// 2. Reject any `app_config.plugins` entry whose key doesn't
    ///    match a registered plugin's [`Plugin::name`] — a user
    ///    declared a plugin that isn't installed.
    /// 3. Topologically sort registered plugins by their `after()`
    ///    / `before()` constraints; reject cycles.
    /// 4. For each plugin: deserialize its user config (or use
    ///    `Default` if absent), call `validate`, then `apply`.
    /// 5. Walk the [`MutationJournal`] for `Set`/`Set` collisions
    ///    on the same `(target, path)` and reject them. `Override`
    ///    is the escape hatch.
    pub fn compose(
        &self,
        app_config: &AppConfig,
        enabled: EnabledTargets,
    ) -> Result<GenerateContext> {
        let mut ctx = build_initial_context(app_config, enabled);

        check_no_unregistered_plugin_configs(app_config, &self.plugins)
            .context("validate AppConfig.plugins against registered plugins")?;

        let order = topo_sort(&self.plugins).context("topologically sort plugins")?;

        for idx in order {
            let plugin = &self.plugins[idx];
            let name = plugin.name();
            let user_cfg = app_config.plugins.get(name);
            plugin
                .run(&mut ctx, user_cfg)
                .with_context(|| format!("plugin `{name}` failed"))?;
        }

        detect_conflicts(&ctx.journal).context("post-pipeline conflict check")?;

        Ok(ctx)
    }
}

// ============================================================================
// Internal — type erasure
// ============================================================================

/// Erased [`Plugin`] surface. One blanket impl on every `P: Plugin`
/// for in-process plugins; an explicit impl on [`SubprocessPlugin`]
/// for 3rd-party binaries.
///
/// Return shapes are owned-string-ish (`&str`, `Vec<&str>`) rather
/// than `&'static`-pinned: subprocess plugins read their name and
/// ordering hints at runtime (from Cargo metadata in PR 3c), so the
/// trait has to accept dynamic strings as well as the
/// `&'static`-clean shape `Plugin` exposes.
pub(crate) trait DynPlugin {
    fn name(&self) -> &str;
    fn after(&self) -> Vec<&str>;
    fn before(&self) -> Vec<&str>;
    /// Run validate + apply with `user_config` (or the Config's
    /// `Default` when `None`).
    fn run(&self, ctx: &mut GenerateContext, user_config: Option<&Value>) -> Result<()>;
}

impl<P: Plugin> DynPlugin for P {
    fn name(&self) -> &str {
        Plugin::name(self)
    }
    fn after(&self) -> Vec<&str> {
        Plugin::after(self).to_vec()
    }
    fn before(&self) -> Vec<&str> {
        Plugin::before(self).to_vec()
    }
    fn run(&self, ctx: &mut GenerateContext, user_config: Option<&Value>) -> Result<()> {
        let cfg: P::Config = match user_config {
            Some(v) => serde_json::from_value(v.clone()).with_context(|| {
                format!("decode user config for plugin `{}`", Plugin::name(self))
            })?,
            None => Default::default(),
        };
        Plugin::validate(self, &cfg)
            .with_context(|| format!("`{}`::validate", Plugin::name(self)))?;
        Plugin::apply(self, ctx, &cfg)
            .with_context(|| format!("`{}`::apply", Plugin::name(self)))?;
        Ok(())
    }
}

// ============================================================================
// Internal — pipeline steps
// ============================================================================

fn build_initial_context(app_config: &AppConfig, enabled: EnabledTargets) -> GenerateContext {
    let app_meta = AppMeta {
        name: app_config.name.clone().unwrap_or_default(),
        version: app_config.version.clone().unwrap_or_default(),
        build_number: app_config.build_number.unwrap_or(1),
        ios_bundle_id: if enabled.ios {
            app_config
                .ios
                .bundle_id
                .clone()
                .or_else(|| app_config.bundle_id.clone())
        } else {
            None
        },
        android_application_id: if enabled.android {
            app_config
                .android
                .application_id
                .clone()
                .or_else(|| app_config.bundle_id.clone())
        } else {
            None
        },
    };

    GenerateContext {
        app_meta,
        ios: enabled.ios.then(IosProjectIr::default),
        android: enabled.android.then(AndroidProjectIr::default),
        journal: MutationJournal::default(),
    }
}

fn check_no_unregistered_plugin_configs(
    app_config: &AppConfig,
    plugins: &[Box<dyn DynPlugin>],
) -> Result<()> {
    let registered: std::collections::HashSet<&str> = plugins.iter().map(|p| p.name()).collect();
    let mut unknown: Vec<&String> = app_config
        .plugins
        .keys()
        .filter(|k| !registered.contains(k.as_str()))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        bail!(
            "AppConfig declares plugin(s) not registered with the engine: {}. \
             Either install the plugin crate or remove the `app.plugin::<{{Cfg}}>(…)` call.",
            unknown
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

/// Kahn's algorithm with deterministic ordering: ties between
/// candidates are broken alphabetically by plugin name so the same
/// `(plugins, AppConfig)` pair always produces the same execution
/// order. The fingerprint path downstream depends on this.
fn topo_sort(plugins: &[Box<dyn DynPlugin>]) -> Result<Vec<usize>> {
    // name → index. Only used for `after()` / `before()` lookups;
    // determinism of the final order comes from sort_by_key on the
    // ready-queue below, not from iteration of this map.
    //
    // Keys are borrowed-from-plugin (`&str`), since subprocess
    // plugins' names live in a `String` field rather than a
    // `&'static str` constant.
    let mut name_to_idx: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, p) in plugins.iter().enumerate() {
        if name_to_idx.insert(p.name(), i).is_some() {
            bail!("two plugins registered with the same name `{}`", p.name());
        }
    }

    // Edges: predecessor → list of successors. `X.after(Y)` and
    // `Y.before(X)` both produce the edge `Y → X`.
    let mut succ: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut in_degree: Vec<usize> = vec![0; plugins.len()];

    let resolve = |this_name: &str, target_name: &str, kind: &str| -> Result<usize> {
        name_to_idx.get(target_name).copied().ok_or_else(|| {
            anyhow!(
                "plugin `{this_name}` declares {kind}(`{target_name}`), \
                 but no plugin with that name is registered"
            )
        })
    };

    for (i, p) in plugins.iter().enumerate() {
        for after_name in p.after() {
            let j = resolve(p.name(), after_name, "after")?;
            if j == i {
                bail!("plugin `{}` lists itself in after()", p.name());
            }
            succ.entry(j).or_default().push(i);
            in_degree[i] += 1;
        }
        for before_name in p.before() {
            let j = resolve(p.name(), before_name, "before")?;
            if j == i {
                bail!("plugin `{}` lists itself in before()", p.name());
            }
            succ.entry(i).or_default().push(j);
            in_degree[j] += 1;
        }
    }

    // Seed queue from index order so registration order breaks
    // ties — combined with the alphabetical name_to_idx walk above
    // this is deterministic.
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut candidates: Vec<usize> = (0..plugins.len()).filter(|&i| in_degree[i] == 0).collect();
    candidates.sort_by_key(|&i| plugins[i].name());
    queue.extend(candidates);

    let mut order = Vec::with_capacity(plugins.len());
    while let Some(i) = queue.pop_front() {
        order.push(i);
        if let Some(succs) = succ.get(&i) {
            let mut newly_ready: Vec<usize> = Vec::new();
            for &j in succs {
                in_degree[j] -= 1;
                if in_degree[j] == 0 {
                    newly_ready.push(j);
                }
            }
            newly_ready.sort_by_key(|&j| plugins[j].name());
            queue.extend(newly_ready);
        }
    }

    if order.len() != plugins.len() {
        let unfinished: Vec<&str> = (0..plugins.len())
            .filter(|i| !order.contains(i))
            .map(|i| plugins[i].name())
            .collect();
        bail!("plugin ordering cycle involving: {}", unfinished.join(", "));
    }

    Ok(order)
}

fn detect_conflicts(journal: &MutationJournal) -> Result<()> {
    let mut last_writer: HashMap<(Target, &str), &MutationRecord> = HashMap::new();
    for r in &journal.records {
        match r.operation {
            Operation::Set => {
                let key = (r.target, r.path.as_str());
                if let Some(prior) = last_writer.get(&key) {
                    bail!(
                        "plugin `{}` set `{:?}.{}` at sequence {}, but plugin `{}` \
                         had already written it at sequence {}. \
                         Order the plugins with `after()` / `before()` and have the \
                         second writer use `Operation::Override` to acknowledge it \
                         intends to replace the earlier value.",
                        r.plugin,
                        r.target,
                        r.path,
                        r.sequence_index,
                        prior.plugin,
                        prior.sequence_index,
                    );
                }
                last_writer.insert(key, r);
            }
            Operation::Override => {
                // Explicitly acknowledges the prior writer — no
                // conflict either way. Still record so a subsequent
                // `Set` against the same path errors.
                last_writer.insert((r.target, r.path.as_str()), r);
            }
            Operation::ArrayPush { .. } => {
                // Array fields are append-only; multiple plugins
                // contributing entries is the whole point.
            }
        }
    }
    Ok(())
}

// ============================================================================
// Subprocess plugins
// ============================================================================

/// 3rd-party plugin shipped as a standalone binary, driven by JSON
/// over stdin/stdout. The corresponding author-side helper is
/// `whisker_plugin::run_as_subprocess`.
///
/// From [`Engine`]'s perspective a subprocess plugin behaves
/// exactly like an in-process one: same `name` / `after` / `before`
/// surface, same dispatch into [`DynPlugin::run`]. The difference
/// is what `run` does — spawn a child process, write a
/// [`PluginRequest`] to its stdin, parse a [`PluginResponse`] back
/// from its stdout, swap the response's context into the engine's
/// running context.
///
/// ## Journal continuity
///
/// The engine hands the subprocess the full running
/// [`GenerateContext`], including the [`MutationJournal`] entries
/// previous plugins already wrote. The subprocess's
/// `whisker_plugin::run_as_subprocess` helper preserves those
/// records and appends new ones via `MutationJournal::record`,
/// which keeps sequence indices monotonic across the in-process /
/// subprocess boundary.
///
/// A malicious or buggy subprocess could drop existing journal
/// entries. PR 3c (discovery) is the right place to surface that
/// guarantee as a hard check; for now we trust the response.
pub struct SubprocessPlugin {
    name: String,
    binary: PathBuf,
    after: Vec<String>,
    before: Vec<String>,
}

impl SubprocessPlugin {
    pub fn new(name: impl Into<String>, binary: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            binary: binary.into(),
            after: Vec::new(),
            before: Vec::new(),
        }
    }

    pub fn after(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.after = names.into_iter().map(Into::into).collect();
        self
    }

    pub fn before(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.before = names.into_iter().map(Into::into).collect();
        self
    }
}

impl DynPlugin for SubprocessPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    fn after(&self) -> Vec<&str> {
        self.after.iter().map(String::as_str).collect()
    }
    fn before(&self) -> Vec<&str> {
        self.before.iter().map(String::as_str).collect()
    }
    fn run(&self, ctx: &mut GenerateContext, user_config: Option<&Value>) -> Result<()> {
        let request = build_request(self.name.clone(), user_config, ctx);
        let response = spawn_and_exchange(&self.binary, &self.name, &request)
            .with_context(|| format!("subprocess plugin `{}` failed", self.name))?;
        merge_response(ctx, response);
        Ok(())
    }
}

impl Engine {
    /// Register a subprocess plugin. The engine spawns
    /// `plugin.binary` on every [`Engine::compose`] call that
    /// dispatches to it.
    pub fn register_subprocess(&mut self, plugin: SubprocessPlugin) -> &mut Self {
        self.plugins.push(Box::new(plugin));
        self
    }
}

fn build_request(
    name: String,
    user_config: Option<&Value>,
    ctx: &GenerateContext,
) -> PluginRequest {
    PluginRequest {
        name,
        config: user_config.cloned().unwrap_or(Value::Null),
        context: ctx.clone(),
    }
}

fn merge_response(ctx: &mut GenerateContext, response: PluginResponse) {
    *ctx = response.context;
}

/// Spawn the plugin binary, pipe JSON, parse the response. stderr
/// is inherited so plugin diagnostics reach the user during
/// `whisker generate --verbose`.
fn spawn_and_exchange(
    binary: &Path,
    plugin_name: &str,
    request: &PluginRequest,
) -> Result<PluginResponse> {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn plugin `{plugin_name}` binary `{}`", binary.display(),))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("plugin `{plugin_name}` stdin pipe missing"))?;
        let json = serde_json::to_vec(request)
            .with_context(|| format!("encode PluginRequest for plugin `{plugin_name}`"))?;
        stdin
            .write_all(&json)
            .with_context(|| format!("write PluginRequest to plugin `{plugin_name}`"))?;
    }
    // No explicit `child.stdin.take()` here — `wait_with_output`
    // does it internally before reading stdout, which is what
    // signals EOF to the child. If we closed stdin first AND the
    // child wrote a stdout response larger than the pipe buffer,
    // we'd deadlock (parent waiting on child exit, child waiting
    // on parent to drain stdout). The wait_with_output path
    // serialises them safely.

    let output = child
        .wait_with_output()
        .with_context(|| format!("wait for plugin `{plugin_name}`"))?;

    check_exit_status(plugin_name, output.status)?;
    decode_response_bytes(plugin_name, &output.stdout)
}

fn check_exit_status(plugin_name: &str, status: std::process::ExitStatus) -> Result<()> {
    if !status.success() {
        bail!(
            "plugin `{plugin_name}` exited with non-zero status ({status}). \
             Check its stderr for the error message."
        );
    }
    Ok(())
}

fn decode_response_bytes(plugin_name: &str, bytes: &[u8]) -> Result<PluginResponse> {
    if bytes.is_empty() {
        bail!(
            "plugin `{plugin_name}` produced empty stdout. \
             A 3rd-party plugin binary should write exactly one \
             PluginResponse JSON envelope and exit 0."
        );
    }
    serde_json::from_slice(bytes)
        .with_context(|| format!("decode PluginResponse JSON from plugin `{plugin_name}`'s stdout"))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use whisker_plugin::{PlistValue, PluginConfig};

    // ----- Test plugins ----------------------------------------------------

    #[derive(Default, Serialize, Deserialize)]
    struct BundleIdCfg {
        #[serde(default)]
        suffix: String,
    }
    impl PluginConfig for BundleIdCfg {
        const NAME: &'static str = "set-bundle-id";
    }
    struct BundleIdPlugin;
    impl Plugin for BundleIdPlugin {
        type Config = BundleIdCfg;
        fn apply(&self, ctx: &mut GenerateContext, cfg: &BundleIdCfg) -> Result<()> {
            let bundle_id = format!("{}{}", "rs.whisker.demo", cfg.suffix);
            if let Some(ios) = ctx.ios.as_mut() {
                ios.info_plist
                    .insert("CFBundleIdentifier".into(), PlistValue::String(bundle_id));
                ctx.journal.record(
                    Self::Config::NAME,
                    Target::Ios,
                    "info_plist.CFBundleIdentifier",
                    Operation::Set,
                );
            }
            Ok(())
        }
    }

    #[derive(Default, Serialize, Deserialize)]
    struct PermissionsCfg {
        #[serde(default)]
        permissions: Vec<String>,
    }
    impl PluginConfig for PermissionsCfg {
        const NAME: &'static str = "permissions";
    }
    struct PermissionsPlugin;
    impl Plugin for PermissionsPlugin {
        type Config = PermissionsCfg;
        fn apply(&self, ctx: &mut GenerateContext, cfg: &PermissionsCfg) -> Result<()> {
            if let Some(a) = ctx.android.as_mut() {
                for p in &cfg.permissions {
                    a.manifest.permissions.push(p.clone());
                }
                if !cfg.permissions.is_empty() {
                    ctx.journal.record(
                        Self::Config::NAME,
                        Target::Android,
                        "manifest.permissions",
                        Operation::ArrayPush {
                            count: cfg.permissions.len(),
                        },
                    );
                }
            }
            Ok(())
        }
    }

    /// Conflicts with BundleIdPlugin if both `Set` the same key.
    #[derive(Default, Serialize, Deserialize)]
    struct AnotherBundleIdCfg {}
    impl PluginConfig for AnotherBundleIdCfg {
        const NAME: &'static str = "another-bundle-id";
    }
    struct AnotherBundleIdPlugin;
    impl Plugin for AnotherBundleIdPlugin {
        type Config = AnotherBundleIdCfg;
        fn apply(&self, ctx: &mut GenerateContext, _cfg: &AnotherBundleIdCfg) -> Result<()> {
            if let Some(ios) = ctx.ios.as_mut() {
                ios.info_plist.insert(
                    "CFBundleIdentifier".into(),
                    PlistValue::String("rs.other".into()),
                );
                ctx.journal.record(
                    Self::Config::NAME,
                    Target::Ios,
                    "info_plist.CFBundleIdentifier",
                    Operation::Set,
                );
            }
            Ok(())
        }
    }

    /// Like AnotherBundleIdPlugin but uses Override.
    #[derive(Default, Serialize, Deserialize)]
    struct OverrideBundleIdCfg {}
    impl PluginConfig for OverrideBundleIdCfg {
        const NAME: &'static str = "override-bundle-id";
    }
    struct OverrideBundleIdPlugin;
    impl Plugin for OverrideBundleIdPlugin {
        type Config = OverrideBundleIdCfg;
        fn after(&self) -> &'static [&'static str] {
            &["set-bundle-id"]
        }
        fn apply(&self, ctx: &mut GenerateContext, _cfg: &OverrideBundleIdCfg) -> Result<()> {
            if let Some(ios) = ctx.ios.as_mut() {
                ios.info_plist.insert(
                    "CFBundleIdentifier".into(),
                    PlistValue::String("rs.overridden".into()),
                );
                ctx.journal.record(
                    Self::Config::NAME,
                    Target::Ios,
                    "info_plist.CFBundleIdentifier",
                    Operation::Override,
                );
            }
            Ok(())
        }
    }

    fn base_app_config() -> AppConfig {
        let mut a = AppConfig::default();
        a.name("Demo").bundle_id("rs.whisker.demo");
        a
    }

    // ----- Happy paths -----------------------------------------------------

    #[test]
    fn empty_engine_yields_an_empty_context() {
        let engine = Engine::new();
        let ctx = engine
            .compose(&base_app_config(), EnabledTargets::both())
            .unwrap();
        assert!(ctx.ios.is_some());
        assert!(ctx.android.is_some());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn enabled_targets_control_which_ir_is_populated() {
        let engine = Engine::new();
        let ios_only = engine
            .compose(&base_app_config(), EnabledTargets::ios_only())
            .unwrap();
        assert!(ios_only.ios.is_some());
        assert!(ios_only.android.is_none());
        let android_only = engine
            .compose(&base_app_config(), EnabledTargets::android_only())
            .unwrap();
        assert!(android_only.ios.is_none());
        assert!(android_only.android.is_some());
    }

    #[test]
    fn appmeta_is_populated_from_app_config() {
        let engine = Engine::new();
        let ctx = engine
            .compose(&base_app_config(), EnabledTargets::both())
            .unwrap();
        assert_eq!(ctx.app_meta.name, "Demo");
        assert_eq!(
            ctx.app_meta.ios_bundle_id.as_deref(),
            Some("rs.whisker.demo")
        );
        assert_eq!(
            ctx.app_meta.android_application_id.as_deref(),
            Some("rs.whisker.demo"),
        );
    }

    #[test]
    fn plugin_runs_with_user_config_when_declared_in_app_config() {
        let mut engine = Engine::new();
        engine.register(BundleIdPlugin);
        let mut app = base_app_config();
        app.plugin::<BundleIdCfg>(|c| {
            c.suffix = ".staging".into();
        });
        let ctx = engine.compose(&app, EnabledTargets::ios_only()).unwrap();
        let ios = ctx.ios.unwrap();
        assert_eq!(
            ios.info_plist.get("CFBundleIdentifier"),
            Some(&PlistValue::String("rs.whisker.demo.staging".into())),
        );
    }

    #[test]
    fn plugin_falls_back_to_default_config_when_not_declared() {
        // No app.plugin::<BundleIdCfg> call → engine still runs the
        // registered plugin with BundleIdCfg::default().
        let mut engine = Engine::new();
        engine.register(BundleIdPlugin);
        let ctx = engine
            .compose(&base_app_config(), EnabledTargets::ios_only())
            .unwrap();
        let ios = ctx.ios.unwrap();
        // suffix defaults to "" → no suffix appended.
        assert_eq!(
            ios.info_plist.get("CFBundleIdentifier"),
            Some(&PlistValue::String("rs.whisker.demo".into())),
        );
    }

    // ----- Ordering --------------------------------------------------------

    #[test]
    fn after_constraint_orders_dependent_plugin_later() {
        let mut engine = Engine::new();
        engine
            .register(OverrideBundleIdPlugin)
            .register(BundleIdPlugin);
        let ctx = engine
            .compose(&base_app_config(), EnabledTargets::ios_only())
            .unwrap();
        let ios = ctx.ios.unwrap();
        assert_eq!(
            ios.info_plist.get("CFBundleIdentifier"),
            Some(&PlistValue::String("rs.overridden".into())),
        );
        // BundleIdPlugin ran first → set-bundle-id (Set) came before
        // override-bundle-id (Override). Sequence indices reflect that.
        let seqs: Vec<_> = ctx
            .journal
            .records
            .iter()
            .map(|r| r.plugin.as_str())
            .collect();
        assert_eq!(seqs, vec!["set-bundle-id", "override-bundle-id"]);
    }

    #[test]
    fn cycle_in_after_constraints_is_rejected() {
        // A.after(B) and B.after(A)
        struct A;
        struct B;
        #[derive(Default, Serialize, Deserialize)]
        struct CfgA;
        impl PluginConfig for CfgA {
            const NAME: &'static str = "a";
        }
        #[derive(Default, Serialize, Deserialize)]
        struct CfgB;
        impl PluginConfig for CfgB {
            const NAME: &'static str = "b";
        }
        impl Plugin for A {
            type Config = CfgA;
            fn after(&self) -> &'static [&'static str] {
                &["b"]
            }
            fn apply(&self, _: &mut GenerateContext, _: &CfgA) -> Result<()> {
                Ok(())
            }
        }
        impl Plugin for B {
            type Config = CfgB;
            fn after(&self) -> &'static [&'static str] {
                &["a"]
            }
            fn apply(&self, _: &mut GenerateContext, _: &CfgB) -> Result<()> {
                Ok(())
            }
        }
        let mut engine = Engine::new();
        engine.register(A).register(B);
        let err = engine
            .compose(&base_app_config(), EnabledTargets::both())
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("cycle"), "{msg}");
    }

    #[test]
    fn after_referencing_an_unregistered_plugin_is_rejected() {
        struct A;
        #[derive(Default, Serialize, Deserialize)]
        struct CfgA;
        impl PluginConfig for CfgA {
            const NAME: &'static str = "a";
        }
        impl Plugin for A {
            type Config = CfgA;
            fn after(&self) -> &'static [&'static str] {
                &["non-existent"]
            }
            fn apply(&self, _: &mut GenerateContext, _: &CfgA) -> Result<()> {
                Ok(())
            }
        }
        let mut engine = Engine::new();
        engine.register(A);
        let err = engine
            .compose(&base_app_config(), EnabledTargets::both())
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("non-existent"), "{msg}");
    }

    // ----- Validation ------------------------------------------------------

    #[test]
    fn declaring_an_unknown_plugin_in_app_config_is_rejected() {
        // User wrote app.plugin::<X>(…) but no engine has X
        // registered. Caught before any plugin runs.
        let mut app = base_app_config();
        app.plugins
            .insert("ghost-plugin".to_string(), serde_json::json!({}));
        let engine = Engine::new();
        let err = engine.compose(&app, EnabledTargets::both()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("ghost-plugin"), "{msg}");
    }

    #[test]
    fn duplicate_plugin_registration_is_rejected() {
        let mut engine = Engine::new();
        engine.register(BundleIdPlugin).register(BundleIdPlugin);
        let err = engine
            .compose(&base_app_config(), EnabledTargets::both())
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("set-bundle-id"), "{msg}");
    }

    #[test]
    fn validate_failure_aborts_before_apply_runs() {
        struct Picky;
        #[derive(Default, Serialize, Deserialize)]
        struct PickyCfg;
        impl PluginConfig for PickyCfg {
            const NAME: &'static str = "picky";
        }
        impl Plugin for Picky {
            type Config = PickyCfg;
            fn validate(&self, _: &PickyCfg) -> Result<()> {
                bail!("nope")
            }
            fn apply(&self, _: &mut GenerateContext, _: &PickyCfg) -> Result<()> {
                panic!("apply should not run when validate fails")
            }
        }
        let mut engine = Engine::new();
        engine.register(Picky);
        let err = engine
            .compose(&base_app_config(), EnabledTargets::both())
            .unwrap_err();
        assert!(format!("{err:#}").contains("nope"));
    }

    // ----- Conflict detection ----------------------------------------------

    #[test]
    fn two_set_writes_to_same_path_is_a_conflict() {
        let mut engine = Engine::new();
        engine
            .register(BundleIdPlugin)
            .register(AnotherBundleIdPlugin);
        let err = engine
            .compose(&base_app_config(), EnabledTargets::ios_only())
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("CFBundleIdentifier"), "{msg}");
        assert!(msg.contains("set-bundle-id"), "{msg}");
        assert!(msg.contains("another-bundle-id"), "{msg}");
    }

    #[test]
    fn override_resolves_what_would_otherwise_be_a_conflict() {
        let mut engine = Engine::new();
        engine
            .register(BundleIdPlugin)
            .register(OverrideBundleIdPlugin);
        // Override-after-Set is the documented use case for Override.
        engine
            .compose(&base_app_config(), EnabledTargets::ios_only())
            .expect("override should resolve the would-be conflict");
    }

    #[test]
    fn array_push_never_conflicts_even_across_plugins() {
        struct OneCam;
        struct OneLoc;
        #[derive(Default, Serialize, Deserialize)]
        struct C1;
        impl PluginConfig for C1 {
            const NAME: &'static str = "one-cam";
        }
        #[derive(Default, Serialize, Deserialize)]
        struct C2;
        impl PluginConfig for C2 {
            const NAME: &'static str = "one-loc";
        }
        impl Plugin for OneCam {
            type Config = C1;
            fn apply(&self, ctx: &mut GenerateContext, _: &C1) -> Result<()> {
                if let Some(a) = ctx.android.as_mut() {
                    a.manifest
                        .permissions
                        .push("android.permission.CAMERA".into());
                    ctx.journal.record(
                        Self::Config::NAME,
                        Target::Android,
                        "manifest.permissions",
                        Operation::ArrayPush { count: 1 },
                    );
                }
                Ok(())
            }
        }
        impl Plugin for OneLoc {
            type Config = C2;
            fn apply(&self, ctx: &mut GenerateContext, _: &C2) -> Result<()> {
                if let Some(a) = ctx.android.as_mut() {
                    a.manifest
                        .permissions
                        .push("android.permission.LOCATION".into());
                    ctx.journal.record(
                        Self::Config::NAME,
                        Target::Android,
                        "manifest.permissions",
                        Operation::ArrayPush { count: 1 },
                    );
                }
                Ok(())
            }
        }
        let mut engine = Engine::new();
        engine.register(OneCam).register(OneLoc);
        let ctx = engine
            .compose(&base_app_config(), EnabledTargets::android_only())
            .unwrap();
        let perms = ctx.android.unwrap().manifest.permissions;
        assert_eq!(perms.len(), 2);
    }

    // ----- Integration --------------------------------------------------

    #[test]
    fn config_decode_error_is_surfaced_with_plugin_name() {
        // Plugin expects suffix: String but we hand it `{"suffix": 7}`.
        let mut app = base_app_config();
        app.plugins.insert(
            BundleIdCfg::NAME.to_string(),
            serde_json::json!({"suffix": 7}),
        );
        let mut engine = Engine::new();
        engine.register(BundleIdPlugin);
        let err = engine
            .compose(&app, EnabledTargets::ios_only())
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("set-bundle-id"), "{msg}");
        assert!(msg.contains("decode"), "{msg}");
    }

    #[test]
    fn full_pipeline_with_permissions_and_bundle_id_succeeds() {
        let mut app = base_app_config();
        app.plugin::<BundleIdCfg>(|c| {
            c.suffix = ".dev".into();
        });
        app.plugin::<PermissionsCfg>(|c| {
            c.permissions.extend([
                "android.permission.CAMERA".into(),
                "android.permission.LOCATION".into(),
            ]);
        });

        let mut engine = Engine::new();
        engine.register(BundleIdPlugin).register(PermissionsPlugin);
        let ctx = engine.compose(&app, EnabledTargets::both()).unwrap();

        assert_eq!(
            ctx.ios.as_ref().unwrap().info_plist["CFBundleIdentifier"],
            PlistValue::String("rs.whisker.demo.dev".into()),
        );
        assert_eq!(ctx.android.as_ref().unwrap().manifest.permissions.len(), 2);
        // 1 record from bundle id (Set) + 1 from permissions (ArrayPush)
        assert_eq!(ctx.journal.records.len(), 2);
    }

    // ----- Subprocess plumbing (pure helpers) --------------------------

    #[test]
    fn build_request_carries_name_config_and_full_context() {
        let mut ctx = GenerateContext::default();
        ctx.app_meta.name = "Demo".into();
        ctx.journal.record(
            "earlier-plugin",
            Target::Ios,
            "info_plist.X",
            Operation::Set,
        );
        let req = build_request(
            "my-plugin".into(),
            Some(&serde_json::json!({"opt": true})),
            &ctx,
        );
        assert_eq!(req.name, "my-plugin");
        assert_eq!(req.config["opt"], true);
        // The journal entry already in the engine context must be
        // visible to the subprocess so its sequence counter continues
        // monotonically — `next_sequence_index` will be 1 there.
        assert_eq!(req.context.journal.next_sequence_index, 1);
        assert_eq!(req.context.app_meta.name, "Demo");
    }

    #[test]
    fn build_request_uses_null_for_missing_user_config() {
        let ctx = GenerateContext::default();
        let req = build_request("my-plugin".into(), None, &ctx);
        assert!(req.config.is_null());
    }

    #[test]
    fn merge_response_replaces_the_engine_context() {
        let mut ctx = GenerateContext::default();
        ctx.app_meta.name = "Old".into();
        let mut new_ctx = GenerateContext::default();
        new_ctx.app_meta.name = "New".into();
        new_ctx.journal.record(
            "subprocess-plugin",
            Target::Android,
            "manifest.permissions",
            Operation::ArrayPush { count: 1 },
        );
        merge_response(&mut ctx, PluginResponse { context: new_ctx });
        assert_eq!(ctx.app_meta.name, "New");
        assert_eq!(ctx.journal.records.len(), 1);
    }

    #[test]
    fn decode_response_bytes_handles_empty_stdout() {
        let err = decode_response_bytes("p", b"").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("empty"), "{msg}");
        assert!(msg.contains("`p`"), "{msg}");
    }

    #[test]
    fn decode_response_bytes_surfaces_invalid_json_with_plugin_name() {
        let err = decode_response_bytes("p", b"not json").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("`p`"), "{msg}");
        assert!(msg.contains("decode"), "{msg}");
    }

    #[test]
    fn decode_response_bytes_accepts_a_valid_envelope() {
        let envelope = serde_json::to_vec(&PluginResponse {
            context: GenerateContext::default(),
        })
        .unwrap();
        let resp = decode_response_bytes("p", &envelope).unwrap();
        assert_eq!(resp.context.journal.records.len(), 0);
    }
}
