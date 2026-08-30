//! In-process plugin engine.
//!
//! Takes a registered set of [`Plugin`]s plus a user `Config`,
//! topologically orders the plugins via their `after()` / `before()`
//! constraints, runs each one with its user-supplied config (or the
//! Config's default when the user didn't declare it), and returns
//! the post-pipeline [`GenerateContext`] that the renderer writes
//! `gen/<platform>/` from.
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
//! `DynPlugin` is `pub(crate)` because a plugin instantiated outside
//! [`Engine::register`] loses the topo-sort and conflict checks.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use whisker_config::Config;
use whisker_plugin::{
    AndroidManifest, AndroidProjectIr, AppMeta, GenerateContext, IosProjectIr, MutationJournal,
    MutationRecord, Operation, PlistValue, Plugin, PluginRequest, PluginResponse, Target,
};

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

/// Registry of plugins the engine runs against an [`Config`].
///
/// Holds a homogeneous list of erased plugins regardless of their
/// concrete `Config` type. Construct via [`Engine::new`], add
/// plugins via [`Engine::register`], run via [`Engine::compose`].
#[derive(Default)]
pub struct Engine {
    plugins: Vec<Box<dyn DynPlugin>>,
    /// Absolute path to the consuming app crate root, forwarded into
    /// the [`GenerateContext`] every `compose` builds so plugins can
    /// resolve user-relative paths (e.g. `whisker-asset`'s
    /// `c.dir("assets")`). `None` for callers that don't run from a
    /// real app crate (most unit tests).
    app_crate_dir: Option<PathBuf>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the consuming app crate root the engine stamps onto each
    /// composed [`GenerateContext`]. Builder-style so it chains off
    /// [`Engine::with_builtins`] / [`Engine::new`].
    pub fn with_app_crate_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.app_crate_dir = Some(dir.into());
        self
    }

    /// Like [`Engine::new`] but pre-registers every built-in plugin
    /// shipped under [`crate::plugins`]. Built-ins are opt-in — their
    /// `Config::default()` is empty — so an app that never calls
    /// `app.plugin::<…>(|c| …)` gets no extra output from them.
    pub fn with_builtins() -> Self {
        let mut e = Self::new();
        e.register(crate::plugins::info_plist_extra::InfoPlistExtra)
            .register(crate::plugins::android_permissions::AndroidPermissions)
            .register(crate::plugins::android_meta_data::AndroidMetaData)
            .register(crate::plugins::android_application_attributes::AndroidApplicationAttributes)
            .register(crate::plugins::android_gradle_plugins::GradlePlugins)
            .register(crate::plugins::android_gradle_dependencies::GradleDependencies)
            .register(crate::plugins::ios_extra_files::IosExtraFiles)
            .register(crate::plugins::android_extra_files::AndroidExtraFiles)
            .register(crate::plugins::ios_pbxproj_ops::IosPbxprojOps)
            .register(crate::plugins::app_icon::AppIcon);
        e
    }

    /// Register a typed [`Plugin`] with the engine. Plugins run in
    /// topologically-sorted order on every `compose` call, not in
    /// registration order.
    pub fn register<P: Plugin + 'static>(&mut self, plugin: P) -> &mut Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// Number of plugins currently registered.
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
    pub fn compose(&self, app_config: &Config, enabled: EnabledTargets) -> Result<GenerateContext> {
        let mut ctx = build_initial_context(app_config, enabled);
        ctx.app_crate_dir = self.app_crate_dir.clone();

        check_no_unregistered_plugin_configs(app_config, &self.plugins)
            .context("validate Config.plugins against registered plugins")?;

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

/// Erased [`Plugin`] surface. One blanket impl on every `P: Plugin`
/// for in-process plugins; an explicit impl on [`SubprocessPlugin`]
/// for 3rd-party binaries.
///
/// Returns `&str` / `Vec<&str>` rather than the `&'static`-pinned
/// shape `Plugin` exposes, because a subprocess plugin's name and
/// ordering hints are read from Cargo metadata at runtime.
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

fn build_initial_context(app_config: &Config, enabled: EnabledTargets) -> GenerateContext {
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

    // The layering is "engine seeds defaults; plugins override via
    // Operation::Override", so every core `Config` field lands in the
    // IR before the first plugin runs.
    let ios = enabled.ios.then(|| IosProjectIr {
        app_name: app_config.name.clone(),
        version: app_config.version.clone(),
        build_number: app_config.build_number,
        bundle_id: app_meta.ios_bundle_id.clone(),
        scheme: app_config.ios.scheme.clone(),
        deployment_target: app_config.ios.deployment_target.clone(),
        info_plist: seed_orientation_plist(&app_config.ios.orientations),
        ..Default::default()
    });
    let android = enabled.android.then(|| AndroidProjectIr {
        app_name: app_config.name.clone(),
        version: app_config.version.clone(),
        build_number: app_config.build_number,
        application_id: app_meta.android_application_id.clone(),
        min_sdk: app_config.android.min_sdk,
        target_sdk: app_config.android.target_sdk,
        manifest: AndroidManifest {
            main_activity_url_schemes: app_config.url_schemes.clone(),
            ..Default::default()
        },
        ..Default::default()
    });

    GenerateContext {
        app_meta,
        ios,
        android,
        journal: MutationJournal::default(),
        // Stamped by `Engine::compose`, which is where the app crate
        // dir is known.
        app_crate_dir: None,
    }
}

fn check_no_unregistered_plugin_configs(
    app_config: &Config,
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
            "Config declares plugin(s) not registered with the engine: {}. \
             Either install the plugin crate or remove the `app.plugin::<{{Plugin}}>(…)` call.",
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
/// `(plugins, Config)` pair always produces the same execution
/// order. The fingerprint path downstream depends on this.
fn topo_sort(plugins: &[Box<dyn DynPlugin>]) -> Result<Vec<usize>> {
    let mut name_to_idx: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, p) in plugins.iter().enumerate() {
        if name_to_idx.insert(p.name(), i).is_some() {
            bail!("two plugins registered with the same name `{}`", p.name());
        }
    }

    // `X.after(Y)` and `Y.before(X)` both produce the edge `Y → X`.
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
                // Acknowledges the prior writer, but still recorded so
                // a later `Set` on the same path errors.
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

/// 3rd-party plugin shipped as a standalone binary, driven by JSON
/// over stdin/stdout. The corresponding author-side helper is
/// `whisker_plugin::run_as_subprocess`.
///
/// From [`Engine`]'s perspective a subprocess plugin behaves exactly
/// like an in-process one: same `name` / `after` / `before` surface,
/// same dispatch into [`DynPlugin::run`]. `run` spawns a child
/// process, writes a [`PluginRequest`] to its stdin, parses a
/// [`PluginResponse`] back, and swaps the response's context into the
/// engine's running context.
///
/// ## Journal continuity
///
/// The subprocess receives the full running [`GenerateContext`],
/// [`MutationJournal`] included, and `run_as_subprocess` appends to
/// those records rather than replacing them — which is what keeps
/// sequence indices monotonic across the process boundary. A buggy
/// subprocess can still drop entries; the response is trusted as-is.
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

/// Spawn the plugin binary, pipe JSON, parse the response. stderr is
/// inherited so plugin diagnostics reach the user's terminal.
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
    // Leave stdin to `wait_with_output`, which closes it (signalling
    // EOF) before draining stdout. Closing it here instead deadlocks
    // whenever the child's response exceeds the pipe buffer: the
    // parent waits on exit while the child waits on a stdout drain.

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

/// Seed `UISupportedInterfaceOrientations` (and its `~ipad` variant).
///
/// App Store validation rejects a bundle that declares no orientations
/// at all, so every generated app gets all four by default. Restricting
/// them is only legal for a bundle that also opts out of iPad
/// multitasking, hence the `UIRequiresFullScreen` companion.
///
/// Seeded into the IR rather than the template so a plugin can still
/// override either key.
fn seed_orientation_plist(
    orientations: &[whisker_config::Orientation],
) -> std::collections::BTreeMap<String, PlistValue> {
    let restricted = !orientations.is_empty();
    let list = if restricted {
        orientations.to_vec()
    } else {
        whisker_config::Orientation::all()
    };
    let value = PlistValue::Array(
        list.iter()
            .map(|o| PlistValue::String(o.plist_value().to_string()))
            .collect(),
    );

    let mut seeded = std::collections::BTreeMap::new();
    seeded.insert(
        "UISupportedInterfaceOrientations".to_string(),
        value.clone(),
    );
    seeded.insert("UISupportedInterfaceOrientations~ipad".to_string(), value);
    if restricted {
        seeded.insert(
            "UIRequiresFullScreen".to_string(),
            PlistValue::Boolean(true),
        );
    }
    seeded
}

#[cfg(test)]
mod tests;
