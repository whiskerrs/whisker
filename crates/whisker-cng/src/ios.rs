//! Render the iOS host project under `gen/ios/` from an
//! [`AppConfig`].
//!
//! The output is a complete Xcode project — `.pbxproj` is rendered
//! directly from a template, no `xcodegen` step:
//!
//! ```text
//! gen/ios/
//! ├── <scheme>.xcodeproj/
//! │   ├── project.pbxproj
//! │   ├── project.xcworkspace/
//! │   │   └── contents.xcworkspacedata
//! │   └── xcshareddata/xcschemes/
//! │       └── <scheme>.xcscheme
//! ├── Info.plist
//! └── Sources/AppDelegate.swift
//! ```
//!
//! Why direct pbxproj rendering: avoids the `xcodegen` runtime
//! dependency. xcodegen was useful to spit out a baseline pbxproj
//! once, after which we templatize and check it into the crate.
//! Subsequent renders are pure string substitution. Same pattern
//! Expo uses for its prebuild bare workflow.
//!
//! Trade-off: we own the pbxproj's compatibility with future Xcode
//! versions. `objectVersion = 77` is the current Xcode 15+ format;
//! if Xcode N+1 demands a new objectVersion, regenerate the
//! template via xcodegen once and re-templatize.

use anyhow::{anyhow, Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use whisker_app_config::AppConfig;
use whisker_plugin::{FileEntry, GenerateContext, PbxprojOp, PlistValue};

use crate::compose::{EnabledTargets, Engine};
use crate::fingerprint;
use crate::render::{escape_xml, render};

const PBXPROJ: &str = include_str!("templates/ios/Project.xcodeproj/project.pbxproj");
const XCWORKSPACEDATA: &str =
    include_str!("templates/ios/Project.xcodeproj/project.xcworkspace/contents.xcworkspacedata");
const XCSCHEME: &str =
    include_str!("templates/ios/Project.xcodeproj/xcshareddata/xcschemes/scheme.xcscheme");
const INFO_PLIST: &str = include_str!("templates/ios/Info.plist");
const APP_DELEGATE_SWIFT: &str = include_str!("templates/ios/Sources/AppDelegate.swift");

#[derive(Debug, Clone, serde::Serialize)]
pub struct IosInputs {
    pub app_name: String,
    pub version: String,
    pub build_number: u32,
    pub scheme: String,
    pub bundle_id: String,
    pub deployment_target: String,
    /// Path the generated pbxproj's `XCLocalSwiftPackageReference`
    /// for `WhiskerRuntime` points at — typically
    /// `<workspace>/platforms/ios` in the monorepo. The published
    /// `XCRemoteSwiftPackageReference` flow that root `Package.swift`
    /// supports is gated on every Whisker module's `Package.swift`
    /// moving off the env-var path-based `.package(path:)`
    /// resolution onto the same remote URL — until that lands,
    /// the cng output stays local-path to avoid product-name
    /// conflicts.
    pub whisker_runtime_path: PathBuf,
    /// Path to the auto-generated `WhiskerModules` SwiftPM package
    /// — typically `<crate_dir>/gen/ios/whisker_modules`. Pointed
    /// at the gen-tree-managed dir `whisker-build::ios::
    /// stage_module_swift_sources` populates with each module's
    /// `[ios].swift_sources` and the generated
    /// `WhiskerModuleBehaviors.swift`.
    pub whisker_modules_path: PathBuf,
    /// Absolute path to the cargo workspace root that contains the
    /// user app crate's top-level `Cargo.toml` (the one with
    /// `[workspace]`). Embedded into the pbxproj's Run Script Build
    /// Phase as `--workspace=...` so Xcode-driven builds invoke
    /// `whisker-build ios` without the user typing it. Step 7.
    pub workspace_root: PathBuf,
    /// Cargo package name (the user app crate) — the Rust side of
    /// `whisker-build ios --package=...`. Step 7.
    pub user_package: String,
    /// `(key, string-value)` pairs sourced from the engine's
    /// post-pipeline IR (`ctx.ios.info_plist`). Emitted into the
    /// rendered `Info.plist` just before the closing `</dict>`. The
    /// renderer XML-escapes values; keys are assumed safe because
    /// they come from Rust string constants in plugin Configs.
    ///
    /// Only `PlistValue::String` entries surface here for now;
    /// dict / array values are dropped because the Info.plist
    /// template is hand-rolled XML rather than a real plist
    /// serializer. Adding richer value support is a future
    /// renderer change, not a wire-format break.
    #[serde(default)]
    pub extra_info_plist_kvs: BTreeMap<String, String>,
    /// Plugin-supplied additional files dropped into `gen/ios/`.
    /// Keys are relative paths (validated to be relative + free of
    /// `..` traversal at write time); values are
    /// [`FileEntry`]s — UTF-8 contents + optional POSIX mode.
    #[serde(default)]
    pub extra_files: BTreeMap<PathBuf, FileEntry>,
    /// Plugin-supplied structural mutations against the Xcode
    /// `project.pbxproj`. Each variant maps to a small set of
    /// generated entries in the rendered pbxproj — see
    /// [`PbxprojOp`] for the supported ops and
    /// [`render_pbxproj_op_placeholders`] for the renderer's
    /// behaviour. Deterministic UUIDs (FNV-1a over each op's
    /// content) keep the rendered file byte-identical across
    /// rebuilds.
    #[serde(default)]
    pub pbxproj_ops: Vec<PbxprojOp>,
    pub template_version: u32,
}

/// Render the iOS project into `out_dir`. Returns whether files were
/// rewritten. See [`crate::android::sync`] for the fast-path / drift
/// rationale — same approach.
pub fn sync(out_dir: &Path, inputs: &IosInputs) -> Result<bool> {
    let new_fp = fingerprint::fingerprint(
        serde_json::to_vec(inputs)
            .context("serialize IosInputs for fingerprint")?
            .as_slice(),
    );
    let fp_path = out_dir.join(".whisker-fingerprint");
    if let Ok(existing) = std::fs::read_to_string(&fp_path) {
        if existing.trim() == new_fp {
            return Ok(false);
        }
    }

    write_files(out_dir, inputs).context("write iOS project files")?;
    std::fs::write(&fp_path, &new_fp)
        .with_context(|| format!("write fingerprint {}", fp_path.display()))?;
    Ok(true)
}

pub(crate) fn template_vars(inputs: &IosInputs) -> HashMap<&'static str, String> {
    let mut v = HashMap::new();
    v.insert("app_name", inputs.app_name.clone());
    v.insert("version", inputs.version.clone());
    v.insert("build_number", inputs.build_number.to_string());
    v.insert("ios_scheme", inputs.scheme.clone());
    v.insert("ios_bundle_id", inputs.bundle_id.clone());
    v.insert("ios_deployment_target", inputs.deployment_target.clone());
    v.insert(
        "whisker_runtime_ios_path",
        inputs.whisker_runtime_path.display().to_string(),
    );
    v.insert(
        "whisker_modules_ios_path",
        inputs.whisker_modules_path.display().to_string(),
    );
    v.insert(
        "whisker_workspace_root",
        inputs.workspace_root.display().to_string(),
    );
    v.insert("whisker_user_package", inputs.user_package.clone());
    v.insert(
        "extra_info_plist_kvs",
        render_extra_info_plist_kvs(&inputs.extra_info_plist_kvs),
    );
    let pbx = render_pbxproj_op_placeholders(&inputs.pbxproj_ops);
    v.insert("extra_pbxproj_build_file_entries", pbx.build_file_entries);
    v.insert(
        "extra_pbxproj_file_reference_entries",
        pbx.file_reference_entries,
    );
    v.insert("extra_pbxproj_sources_phase_files", pbx.sources_phase_files);
    v.insert(
        "extra_pbxproj_resources_phase_files",
        pbx.resources_phase_files,
    );
    v.insert(
        "extra_pbxproj_frameworks_phase_files",
        pbx.frameworks_phase_files,
    );
    v.insert(
        "extra_pbxproj_plugin_files_group_children",
        pbx.plugin_files_group_children,
    );
    v.insert(
        "extra_pbxproj_target_build_settings",
        pbx.target_build_settings,
    );
    v
}

/// Bundled output of [`render_pbxproj_op_placeholders`] — one
/// field per pbxproj-template placeholder so adding a new
/// op-derived section stays a single-line change here + a
/// matching `{{…}}` in the template.
struct PbxprojRendered {
    build_file_entries: String,
    file_reference_entries: String,
    sources_phase_files: String,
    resources_phase_files: String,
    frameworks_phase_files: String,
    plugin_files_group_children: String,
    target_build_settings: String,
}

/// Translate the engine's `Vec<PbxprojOp>` into the seven
/// pbxproj-template placeholder strings the renderer needs. Empty
/// inputs → empty strings for every placeholder so the template
/// stays valid pbxproj even with no plugin contributions.
///
/// UUIDs are deterministic ([`pbxproj_uuid`]). A given
/// `(op variant, payload)` pair produces the same UUID across
/// every render, which keeps the rendered file byte-identical
/// across rebuilds and lets the fingerprint cache skip path do
/// its job.
fn render_pbxproj_op_placeholders(ops: &[PbxprojOp]) -> PbxprojRendered {
    let mut build_file_entries = String::new();
    let mut file_reference_entries = String::new();
    let mut sources_phase_files = String::new();
    let mut resources_phase_files = String::new();
    let mut frameworks_phase_files = String::new();
    let mut plugin_files_group_children = String::new();
    let mut target_build_settings = String::new();

    for op in ops {
        match op {
            PbxprojOp::AddResource { path } => {
                let path_str = path.display().to_string();
                let fileref_uuid = pbxproj_uuid(&format!("PBXFileReference:{path_str}"));
                let buildfile_uuid = pbxproj_uuid(&format!("PBXBuildFile:Resources:{path_str}"));
                let file_type = last_known_file_type(path);
                build_file_entries.push_str(&format!(
                    "\t\t{buildfile_uuid} /* {path_str} in Resources */ = \
                     {{isa = PBXBuildFile; fileRef = {fileref_uuid} /* {path_str} */; }};\n",
                ));
                file_reference_entries.push_str(&format!(
                    "\t\t{fileref_uuid} /* {path_str} */ = \
                     {{isa = PBXFileReference; lastKnownFileType = {file_type}; \
                     path = \"{path_str}\"; sourceTree = \"<group>\"; }};\n",
                ));
                resources_phase_files.push_str(&format!(
                    "\t\t\t\t{buildfile_uuid} /* {path_str} in Resources */,\n",
                ));
                plugin_files_group_children
                    .push_str(&format!("\t\t\t\t{fileref_uuid} /* {path_str} */,\n",));
            }
            PbxprojOp::AddSource { path } => {
                let path_str = path.display().to_string();
                let fileref_uuid = pbxproj_uuid(&format!("PBXFileReference:{path_str}"));
                let buildfile_uuid = pbxproj_uuid(&format!("PBXBuildFile:Sources:{path_str}"));
                let file_type = last_known_file_type(path);
                build_file_entries.push_str(&format!(
                    "\t\t{buildfile_uuid} /* {path_str} in Sources */ = \
                     {{isa = PBXBuildFile; fileRef = {fileref_uuid} /* {path_str} */; }};\n",
                ));
                file_reference_entries.push_str(&format!(
                    "\t\t{fileref_uuid} /* {path_str} */ = \
                     {{isa = PBXFileReference; lastKnownFileType = {file_type}; \
                     path = \"{path_str}\"; sourceTree = \"<group>\"; }};\n",
                ));
                sources_phase_files.push_str(&format!(
                    "\t\t\t\t{buildfile_uuid} /* {path_str} in Sources */,\n",
                ));
                plugin_files_group_children
                    .push_str(&format!("\t\t\t\t{fileref_uuid} /* {path_str} */,\n",));
            }
            PbxprojOp::LinkSystemFramework { name } => {
                let fileref_uuid = pbxproj_uuid(&format!("PBXFileReference:Framework:{name}"));
                let buildfile_uuid = pbxproj_uuid(&format!("PBXBuildFile:Frameworks:{name}"));
                build_file_entries.push_str(&format!(
                    "\t\t{buildfile_uuid} /* {name} in Frameworks */ = \
                     {{isa = PBXBuildFile; fileRef = {fileref_uuid} /* {name} */; }};\n",
                ));
                file_reference_entries.push_str(&format!(
                    "\t\t{fileref_uuid} /* {name} */ = \
                     {{isa = PBXFileReference; lastKnownFileType = wrapper.framework; \
                     name = \"{name}\"; path = \"System/Library/Frameworks/{name}\"; \
                     sourceTree = SDKROOT; }};\n",
                ));
                frameworks_phase_files.push_str(&format!(
                    "\t\t\t\t{buildfile_uuid} /* {name} in Frameworks */,\n",
                ));
                plugin_files_group_children
                    .push_str(&format!("\t\t\t\t{fileref_uuid} /* {name} */,\n",));
            }
            PbxprojOp::SetBuildSetting { key, value } => {
                target_build_settings.push_str(&format!(
                    "\t\t\t\t\t{key} = \"{}\";\n",
                    escape_pbxproj_string(value),
                ));
            }
        }
    }

    // Trim trailing newlines so the surrounding template's own
    // newlines aren't doubled up. Empty strings stay empty.
    fn trim(s: &mut String) {
        if s.ends_with('\n') {
            s.pop();
        }
    }
    trim(&mut build_file_entries);
    trim(&mut file_reference_entries);
    trim(&mut sources_phase_files);
    trim(&mut resources_phase_files);
    trim(&mut frameworks_phase_files);
    trim(&mut plugin_files_group_children);
    trim(&mut target_build_settings);

    PbxprojRendered {
        build_file_entries,
        file_reference_entries,
        sources_phase_files,
        resources_phase_files,
        frameworks_phase_files,
        plugin_files_group_children,
        target_build_settings,
    }
}

/// Escape a string for inclusion inside a pbxproj double-quoted
/// literal. The pbxproj "OpenStep plist" lexer treats `"` and `\`
/// as the only chars that need backslash-escape inside `"…"`;
/// everything else (whitespace, `$`, `(`, `)`, etc.) is fine.
fn escape_pbxproj_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// Pick a `lastKnownFileType` for a file path, by extension. Falls
/// back to `text` for anything unknown — Xcode tolerates a wrong
/// guess; it just affects the navigator icon.
fn last_known_file_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("swift") => "sourcecode.swift",
        Some("m") => "sourcecode.c.objc",
        Some("mm") => "sourcecode.cpp.objcpp",
        Some("h") => "sourcecode.c.h",
        Some("plist") => "text.plist.xml",
        Some("json") => "text.json",
        Some("png") => "image.png",
        Some("jpg") | Some("jpeg") => "image.jpeg",
        Some("xcassets") => "folder.assetcatalog",
        Some("storyboard") => "file.storyboard",
        Some("xib") => "file.xib",
        _ => "text",
    }
}

/// Deterministic 24-hex-char UUID for a stable string seed.
/// Pbxproj refs are 24-char hex strings (96-bit). We splice two
/// FNV-1a hashes (16 hex each, salted differently) and take the
/// first 24 chars so the output stays in the canonical shape Xcode
/// produces. Determinism is what matters — collision risk across
/// `seed` strings within a single sync is negligible at this
/// length and the rendered pbxproj would fail to parse on
/// collision anyway, surfacing the bug immediately.
fn pbxproj_uuid(seed: &str) -> String {
    let a = crate::fingerprint::fingerprint(seed.as_bytes());
    let b = crate::fingerprint::fingerprint(format!("{seed}-salt").as_bytes());
    format!("{a}{}", &b[..8]).to_uppercase()
}

/// Render the engine-supplied `(key, string)` pairs as XML
/// `<key>…</key><string>…</string>` rows ready to drop straight
/// into the Info.plist template just before `</dict>`. Empty map
/// → empty string (no whitespace) so the template still parses
/// cleanly.
fn render_extra_info_plist_kvs(entries: &BTreeMap<String, String>) -> String {
    if entries.is_empty() {
        return String::new();
    }
    // Indent matching the rest of the template (tab characters,
    // matching the hand-rolled Info.plist's existing style).
    let mut out = String::new();
    for (key, value) in entries {
        out.push_str(&format!(
            "\t<key>{}</key>\n\t<string>{}</string>\n",
            escape_xml(key),
            escape_xml(value),
        ));
    }
    // Strip the trailing newline so the template's own newline
    // before `</dict>` isn't doubled up.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn write_files(out_dir: &Path, inputs: &IosInputs) -> Result<()> {
    let vars = template_vars(inputs);

    // Wipe the previous tree but keep the per-build output dir —
    // expensive to recreate and re-derivable by re-running xcodebuild.
    clean_managed_tree(out_dir, &inputs.scheme).context("clean previous iOS gen tree")?;

    // Top-level text files (plain templates).
    let text_files: &[(PathBuf, &str)] = &[
        (out_dir.join("Info.plist"), INFO_PLIST),
        (
            out_dir.join("Sources/AppDelegate.swift"),
            APP_DELEGATE_SWIFT,
        ),
    ];
    for (path, template) in text_files {
        let rendered =
            render(template, &vars).with_context(|| format!("render {}", path.display()))?;
        write_file(path, rendered.as_bytes())?;
    }

    // Xcode project tree. Filename includes the scheme, content is
    // rendered.
    let xcodeproj = out_dir.join(format!("{}.xcodeproj", inputs.scheme));
    let pbxproj = render(PBXPROJ, &vars).context("render project.pbxproj")?;
    write_file(&xcodeproj.join("project.pbxproj"), pbxproj.as_bytes())?;
    // xcworkspacedata has no placeholders — write as-is.
    write_file(
        &xcodeproj
            .join("project.xcworkspace")
            .join("contents.xcworkspacedata"),
        XCWORKSPACEDATA.as_bytes(),
    )?;
    // Shared xcscheme so opening the project in Xcode.app yields the
    // same Build / Run / Test / Profile / Analyze / Archive surface
    // every contributor sees. Without this, Xcode auto-creates a
    // per-user scheme on first open — works, but isn't shared via
    // source control and the user has to pick a destination on every
    // fresh checkout. Filename mirrors the scheme name so Xcode picks
    // it up by convention (it scans `xcshareddata/xcschemes/*.xcscheme`).
    let xcscheme = render(XCSCHEME, &vars).context("render xcscheme")?;
    write_file(
        &xcodeproj
            .join("xcshareddata/xcschemes")
            .join(format!("{}.xcscheme", inputs.scheme)),
        xcscheme.as_bytes(),
    )?;

    // Plugin-supplied `extra_files`. Paths are validated to be
    // relative + traversal-free; on Unix, `mode` is applied
    // verbatim. iOS doesn't typically need the executable bit, but
    // shipping the helper means a plugin can drop a code-signing
    // script alongside the project.
    for (rel, entry) in &inputs.extra_files {
        crate::render::validate_extra_file_path(rel).with_context(|| {
            format!(
                "extra_files entry `{}` (iOS plugin contribution)",
                rel.display(),
            )
        })?;
        let abs = out_dir.join(rel);
        write_file(&abs, entry.contents.as_bytes())?;
        apply_mode(&abs, entry.mode)?;
    }

    Ok(())
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: Option<u32>) -> Result<()> {
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .with_context(|| format!("stat {} for chmod", path.display()))?
            .permissions();
        perms.set_mode(m);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod {:o} on {}", m, path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: Option<u32>) -> Result<()> {
    // POSIX mode bits don't translate cleanly to Windows ACLs.
    // The IR is platform-agnostic so we accept the field on every
    // host and silently ignore it on Windows — matches how cargo
    // and rustc handle the same situation in `[[bin]]` targets.
    Ok(())
}

fn clean_managed_tree(out_dir: &Path, scheme: &str) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    // The `.xcodeproj` directory is now CNG-owned (we render every
    // file inside it) so we clean it on each sync to avoid stale
    // content. `build` is xcodebuild's `-derivedDataPath` output
    // and is expensive to rebuild; preserve it.
    let xcodeproj_dir = format!("{scheme}.xcodeproj");
    let keep = ["build"];
    for entry in
        std::fs::read_dir(out_dir).with_context(|| format!("read_dir {}", out_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".whisker-fingerprint" {
            continue;
        }
        if keep.iter().any(|k| name.as_os_str() == *k) {
            continue;
        }
        // The xcodeproj itself is regenerated on every sync (because
        // its contents may template differently each time). Skip the
        // keep list and let `remove_path` blow it away.
        let _ = &xcodeproj_dir;
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn remove_path(p: &Path) -> Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p).with_context(|| format!("rm -rf {}", p.display()))
    } else {
        std::fs::remove_file(p).with_context(|| format!("rm {}", p.display()))
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

/// Pull the iOS-relevant subset of `AppConfig` into the renderer
/// input struct. Errors out on required fields. `scheme` defaults to
/// `name`; `bundle_id` defaults to the top-level `app.bundle_id`.
pub fn inputs_from(
    app_config: &AppConfig,
    whisker_runtime_path: PathBuf,
    whisker_modules_path: PathBuf,
    workspace_root: PathBuf,
    user_package: String,
) -> Result<IosInputs> {
    // Run the plugin pipeline with built-ins. `build_initial_context`
    // seeds the IR with core fields from `AppConfig`; plugins can
    // override any of them via `Operation::Override`. The renderer
    // reads the post-pipeline IR — `inputs_from`'s job is now
    // strictly extraction + ergonomic defaults for fields the
    // engine left as `None`.
    let ctx = Engine::with_builtins()
        .compose(app_config, EnabledTargets::ios_only())
        .context("compose Whisker CNG plugin pipeline for iOS")?;
    let ios_ir = ctx
        .ios
        .as_ref()
        .expect("EnabledTargets::ios_only guarantees Some");

    let app_name = ios_ir
        .app_name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required"))?;
    let version = ios_ir
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let build_number = ios_ir.build_number.unwrap_or(1);
    // Scheme defaults to the app name — the engine doesn't apply
    // ergonomic defaults; that's `inputs_from`'s contract.
    let scheme = ios_ir.scheme.clone().unwrap_or_else(|| app_name.clone());
    let bundle_id = ios_ir.bundle_id.clone().ok_or_else(|| {
        anyhow!(
            "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) (or app.bundle_id) is required for iOS"
        )
    })?;
    let deployment_target = ios_ir
        .deployment_target
        .clone()
        .unwrap_or_else(|| "13.0".to_string());

    let extra_info_plist_kvs = extract_info_plist_string_kvs(&ctx);
    let extra_files = ios_ir.extra_files.clone();
    let pbxproj_ops = ios_ir.pbxproj_ops.clone();

    Ok(IosInputs {
        app_name,
        version,
        build_number,
        scheme,
        bundle_id,
        deployment_target,
        whisker_runtime_path,
        whisker_modules_path,
        workspace_root,
        user_package,
        extra_info_plist_kvs,
        extra_files,
        pbxproj_ops,
        // Bumped 12 → 13 for `pbxproj_ops` template-injection
        // (RFC #164 B-direction PR 4). The pbxproj template grew
        // seven new placeholders + a PBXResourcesBuildPhase
        // section + a "Whisker Plugin Files" group; existing
        // `gen/ios/` trees regenerate so the new sections render
        // correctly even before any plugin contributes content.
        template_version: 13,
    })
}

/// Project the iOS info_plist BTreeMap (the IR layer) into the
/// `(key, string-value)` shape the template renderer accepts.
/// Non-string `PlistValue` variants are silently dropped — the
/// template is hand-rolled XML and can't represent dicts / arrays
/// safely. A future renderer that emits real plist XML can lift
/// this restriction without changing the IR.
fn extract_info_plist_string_kvs(ctx: &GenerateContext) -> BTreeMap<String, String> {
    let Some(ios) = ctx.ios.as_ref() else {
        return BTreeMap::new();
    };
    ios.info_plist
        .iter()
        .filter_map(|(k, v)| match v {
            PlistValue::String(s) => Some((k.clone(), s.clone())),
            _ => None,
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cng-ios-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_inputs() -> IosInputs {
        IosInputs {
            app_name: "HelloWorld".into(),
            version: "0.1.0".into(),
            build_number: 1,
            scheme: "HelloWorld".into(),
            bundle_id: "rs.whisker.examples.helloWorld".into(),
            deployment_target: "13.0".into(),
            whisker_runtime_path: PathBuf::from("/abs/platforms/ios"),
            whisker_modules_path: PathBuf::from("/abs/gen/ios/whisker_modules"),
            workspace_root: PathBuf::from("/abs/workspace"),
            user_package: "hello-world".into(),
            extra_info_plist_kvs: BTreeMap::new(),
            extra_files: BTreeMap::new(),
            pbxproj_ops: Vec::new(),
            template_version: 13,
        }
    }

    #[test]
    fn sync_writes_expected_files() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        let regenerated = sync(&out, &sample_inputs()).unwrap();
        assert!(regenerated);
        for expected in [
            "Info.plist",
            "Sources/AppDelegate.swift",
            "HelloWorld.xcodeproj/project.pbxproj",
            "HelloWorld.xcodeproj/project.xcworkspace/contents.xcworkspacedata",
            "HelloWorld.xcodeproj/xcshareddata/xcschemes/HelloWorld.xcscheme",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(expected).exists(), "missing: {expected}");
        }
        // No project.yml — xcodegen is gone.
        assert!(!out.join("project.yml").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_substitutes_placeholders_in_pbxproj() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &sample_inputs()).unwrap();
        let pbxproj =
            std::fs::read_to_string(out.join("HelloWorld.xcodeproj/project.pbxproj")).unwrap();
        assert!(pbxproj.contains("PRODUCT_BUNDLE_IDENTIFIER = \"rs.whisker.examples.helloWorld\""));
        assert!(pbxproj.contains("IPHONEOS_DEPLOYMENT_TARGET = \"13.0\""));
        // XCLocalSwiftPackageReference for WhiskerRuntime (monorepo
        // workflow). The XCRemoteSwiftPackageReference form behind
        // root Package.swift stays available for future
        // published-consumer flows.
        assert!(pbxproj.contains("relativePath = \"/abs/platforms/ios\""));
        // WhiskerModules resolves through the per-app gen-tree dir.
        assert!(pbxproj.contains("relativePath = \"/abs/gen/ios/whisker_modules\""));
        assert!(pbxproj.contains("name = \"HelloWorld\""));
        assert!(pbxproj.contains("productName = \"HelloWorld\""));
        // Catch any unsubstituted placeholders.
        assert!(!pbxproj.contains("{{"));
    }

    #[test]
    fn sync_substitutes_placeholders_in_info_plist() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &sample_inputs()).unwrap();
        let plist = std::fs::read_to_string(out.join("Info.plist")).unwrap();
        assert!(plist.contains("<string>HelloWorld</string>"));
        assert!(plist.contains("<string>0.1.0</string>"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_is_idempotent_when_fingerprint_matches() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        let first = sync(&out, &sample_inputs()).unwrap();
        assert!(first);
        let second = sync(&out, &sample_inputs()).unwrap();
        assert!(!second);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_regenerates_xcodeproj_when_inputs_change() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &sample_inputs()).unwrap();
        let mut next = sample_inputs();
        next.scheme = "NewScheme".into();
        sync(&out, &next).unwrap();
        // New scheme dir exists; old one is gone (entire xcodeproj
        // is re-rendered).
        assert!(out.join("NewScheme.xcodeproj/project.pbxproj").exists());
        assert!(!out.join("HelloWorld.xcodeproj").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inputs_from_errors_when_bundle_id_unset() {
        let cfg = AppConfig {
            name: Some("X".into()),
            ..AppConfig::default()
        };
        let err = inputs_from(
            &cfg,
            PathBuf::new(),
            PathBuf::new(),
            PathBuf::new(),
            String::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bundle_id"), "got: {err:#}");
    }
}
