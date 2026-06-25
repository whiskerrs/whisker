//! Render the Android host project under `gen/android/` from an
//! [`Config`].
//!
//! The output mirrors a small AGP-flavoured Android Studio project:
//!
//! ```text
//! gen/android/
//! ├── app/
//! │   ├── build.gradle.kts
//! │   └── src/main/
//! │       ├── AndroidManifest.xml
//! │       ├── jniLibs/                          (populated at build time)
//! │       └── kotlin/<package-path>/
//! │           ├── MainActivity.kt
//! │           └── <AppName>Application.kt
//! ├── build.gradle.kts
//! ├── settings.gradle.kts
//! ├── gradle.properties
//! ├── gradlew
//! ├── gradlew.bat
//! └── gradle/wrapper/
//!     ├── gradle-wrapper.jar
//!     └── gradle-wrapper.properties
//! ```
//!
//! The package path under `kotlin/` is `applicationId` with dots
//! converted to slashes: `rs.whisker.examples.helloworld` →
//! `rs/whisker/examples/helloworld/`.

use anyhow::{Context, Result, anyhow};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use whisker_config::Config;
use whisker_plugin::{ApplicationAttribute, FileEntry, MetaDataEntry};

use crate::compose::{EnabledTargets, Engine};
use crate::fingerprint;
use crate::render::{escape_xml, render};

// ---- Embedded templates ----------------------------------------------------
//
// Text files go through `{{placeholder}}` substitution. Binary files
// (the gradle wrapper jar) are copied verbatim. `gradlew` is text but
// needs the +x bit on Unix so it lives in its own list.

const APP_BUILD_GRADLE_KTS: &str = include_str!("templates/android/app/build.gradle.kts");
const APP_MANIFEST_XML: &str = include_str!("templates/android/app/src/main/AndroidManifest.xml");
const MAIN_ACTIVITY_KT: &str =
    include_str!("templates/android/app/src/main/kotlin/MainActivity.kt");
const APPLICATION_KT: &str = include_str!("templates/android/app/src/main/kotlin/Application.kt");
const ROOT_BUILD_GRADLE_KTS: &str = include_str!("templates/android/build.gradle.kts");
const SETTINGS_GRADLE_KTS: &str = include_str!("templates/android/settings.gradle.kts");
const GRADLE_PROPERTIES: &str = include_str!("templates/android/gradle.properties");
const GRADLEW: &str = include_str!("templates/android/gradlew");
const GRADLEW_BAT: &str = include_str!("templates/android/gradlew.bat");
const GRADLE_WRAPPER_PROPERTIES: &str =
    include_str!("templates/android/gradle/wrapper/gradle-wrapper.properties");
const GRADLE_WRAPPER_JAR: &[u8] =
    include_bytes!("templates/android/gradle/wrapper/gradle-wrapper.jar");

/// Inputs the Android renderer pulls out of `Config` (+ a few
/// values the cli passes in like the dylib name and the workspace's
/// `platforms/android/whisker-runtime` location).
///
/// Holding these in a struct rather than a big tuple keeps the
/// fingerprint serialization stable and the template-vars build site
/// easy to read.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AndroidInputs {
    pub app_name: String,
    pub version: String,
    pub build_number: u32,
    pub application_id: String,
    pub min_sdk: u32,
    pub target_sdk: u32,
    /// Crate name with hyphens replaced by underscores — what
    /// `System.loadLibrary` and `keepDebugSymbols` reference.
    pub rust_lib_name: String,
    /// Path the generated `settings.gradle.kts` writes into the
    /// `whisker { workspace = file(...) }` block. The Settings
    /// plugin resolves it relative to `gen/android/`, so callers
    /// typically pass `../..` (or similar) — the path to the cargo
    /// workspace root containing the user app's `Cargo.toml`.
    pub whisker_workspace_path: PathBuf,
    /// Cargo crate name of the user app. Echoed into
    /// `whisker { userPackage = "..." }`. The Settings plugin
    /// walks the cargo dep graph rooted here for
    /// `[package.metadata.whisker]`-tagged module deps.
    pub whisker_user_package: String,
    /// `rs.whisker:whisker-runtime-android:<this>` + sibling SDK
    /// coords' version. Step 4.5-e initial release is `0.1.0`.
    pub whisker_sdk_version: String,
    /// `rs.whisker:rs.whisker.gradle.plugin:<this>` version pinned
    /// in `pluginManagement.plugins`. Independent from
    /// `whisker_sdk_version` — gradle-plugin and SDK release on
    /// separate `gradle-plugin-v*` / `sdk-v*` tag streams.
    pub whisker_gradle_plugin_version: String,
    /// gh-pages Maven URL hosting Whisker's plugins + SDK. Templates
    /// declare it in both `pluginManagement.repositories` and
    /// `dependencyResolutionManagement.repositories`.
    pub whisker_maven_url: String,
    /// gh-pages Maven URL hosting the Lynx fork AARs that
    /// `whisker-runtime-android` pulls transitively.
    pub lynx_maven_url: String,
    /// `<uses-permission android:name="…"/>` rows from the engine's
    /// post-pipeline IR. Emitted after the template's hardcoded
    /// `INTERNET` permission. Dedup'd: the same permission
    /// contributed by multiple plugins shows up once.
    #[serde(default)]
    pub extra_permissions: Vec<String>,
    /// `<meta-data android:name="…" android:value="…"/>` rows from
    /// the engine's post-pipeline IR. Emitted inside the
    /// `<application>` block. Preserves insertion order — multiple
    /// plugins contributing entries see deterministic output.
    #[serde(default)]
    pub extra_meta_data: Vec<MetaDataEntry>,
    /// Attributes on the `<application>` tag itself (e.g.
    /// `android:enableOnBackInvokedCallback="true"`) from the engine's
    /// post-pipeline IR. Dedup'd by attribute name (last writer wins).
    #[serde(default)]
    pub extra_application_attributes: Vec<ApplicationAttribute>,
    /// Extra entries the renderer drops into the app module's
    /// `plugins { … }` block, just after the baseline Whisker /
    /// AGP / Kotlin plugin ids. Bare ids (e.g.
    /// `"com.google.gms.google-services"`) get wrapped in
    /// `id("…")`; raw `id(...)` lines pass through verbatim so
    /// users can attach `version "…"` / `apply false` qualifiers.
    #[serde(default)]
    pub extra_gradle_plugins: Vec<String>,
    /// Extra raw lines the renderer drops into the app module's
    /// `dependencies { … }` block. Each entry is emitted verbatim
    /// (e.g.
    /// `"implementation(\"com.google.firebase:firebase-analytics:21.5.0\")"`).
    #[serde(default)]
    pub extra_gradle_dependencies: Vec<String>,
    /// Plugin-supplied additional files dropped into `gen/android/`.
    /// Keys are relative paths (validated at write time); values
    /// are [`FileEntry`]s — UTF-8 contents + optional POSIX mode.
    ///
    /// Mode handling on Android is intentionally coarser than the
    /// iOS renderer's: the existing `write_file` helper takes a
    /// `bool` executable flag, so the renderer projects
    /// `FileEntry::mode` onto "executable yes/no" (any mode with
    /// the user-execute bit set → 0o755, otherwise 0o644). Plugins
    /// that need finer-grained Android permissions today would have
    /// to ship a wrapper script that `chmod`s at build time —
    /// loosening this is a one-line `write_file` refactor when the
    /// first consumer needs it.
    #[serde(default)]
    pub extra_files: BTreeMap<PathBuf, FileEntry>,
    /// Bumped whenever the template *shape* changes (added file,
    /// renamed placeholder, …). The fingerprint mixes this in so
    /// existing `gen/` trees regenerate after an upgrade.
    pub template_version: u32,
}

/// Render the Android project into `out_dir` (typically
/// `<crate_dir>/gen/android`). Returns whether files were actually
/// rewritten — `false` means the cached fingerprint matched and the
/// existing tree was reused. The caller decides what to do with that
/// (log "in sync", skip a downstream sync, …).
pub fn sync(out_dir: &Path, inputs: &AndroidInputs) -> Result<bool> {
    let new_fp = fingerprint::fingerprint(
        serde_json::to_vec(inputs)
            .context("serialize AndroidInputs for fingerprint")?
            .as_slice(),
    );
    let fp_path = out_dir.join(".whisker-fingerprint");
    if let Ok(existing) = std::fs::read_to_string(&fp_path) {
        if existing.trim() == new_fp {
            return Ok(false);
        }
    }

    write_files(out_dir, inputs).context("write Android project files")?;
    std::fs::write(&fp_path, &new_fp)
        .with_context(|| format!("write fingerprint {}", fp_path.display()))?;
    Ok(true)
}

/// Build the `{{var}}` table from `inputs`. Split out so unit tests
/// can assert against the result without going through file I/O.
pub(crate) fn template_vars(inputs: &AndroidInputs) -> HashMap<&'static str, String> {
    let mut v = HashMap::new();
    v.insert("app_name", inputs.app_name.clone());
    v.insert("version", inputs.version.clone());
    v.insert("build_number", inputs.build_number.to_string());
    v.insert("android_application_id", inputs.application_id.clone());
    v.insert(
        "android_application_class",
        application_class_name(&inputs.app_name),
    );
    v.insert("android_min_sdk", inputs.min_sdk.to_string());
    v.insert("android_target_sdk", inputs.target_sdk.to_string());
    v.insert("android_project_name", project_name(&inputs.app_name));
    v.insert("rust_lib_name", inputs.rust_lib_name.clone());
    v.insert(
        "whisker_workspace_path",
        inputs.whisker_workspace_path.display().to_string(),
    );
    v.insert("whisker_user_package", inputs.whisker_user_package.clone());
    v.insert("whisker_sdk_version", inputs.whisker_sdk_version.clone());
    v.insert(
        "whisker_gradle_plugin_version",
        inputs.whisker_gradle_plugin_version.clone(),
    );
    v.insert("whisker_maven_url", inputs.whisker_maven_url.clone());
    v.insert("lynx_maven_url", inputs.lynx_maven_url.clone());
    v.insert(
        "extra_uses_permissions",
        render_extra_permissions(&inputs.extra_permissions),
    );
    v.insert(
        "extra_application_meta_data",
        render_extra_meta_data(&inputs.extra_meta_data),
    );
    v.insert(
        "extra_application_attributes",
        render_extra_application_attributes(&inputs.extra_application_attributes),
    );
    v.insert(
        "extra_gradle_plugins",
        render_extra_gradle_plugins(&inputs.extra_gradle_plugins),
    );
    v.insert(
        "extra_gradle_dependencies",
        render_extra_gradle_dependencies(&inputs.extra_gradle_dependencies),
    );
    v
}

/// Render `apply_plugins` entries as Kotlin DSL lines inside the
/// `plugins { … }` block. Two shapes:
///
///   - Bare gradle plugin id (e.g. `"com.google.gms.google-services"`)
///     → wrapped in `id("…")`.
///   - Anything containing a `(` character (e.g. `id("…") version "X"`,
///     `alias(libs.plugins.foo)`, `kotlin("jvm")`) → emitted
///     verbatim. The Kotlin DSL's plugin block accepts every
///     callable that returns a `PluginDependencySpec`, and bare
///     gradle plugin ids never contain `(`, so this is a safe
///     discriminator.
fn render_extra_gradle_plugins(entries: &[String]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for entry in entries {
        if entry.contains('(') {
            out.push_str(&format!("    {entry}\n"));
        } else {
            out.push_str(&format!("    id(\"{entry}\")\n"));
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn render_extra_gradle_dependencies(entries: &[String]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for entry in entries {
        out.push_str(&format!("    {entry}\n"));
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Render the engine-supplied permissions as `<uses-permission>`
/// rows, dedup'd. Empty input → empty string so the template still
/// parses when no plugin contributed.
fn render_extra_permissions(perms: &[String]) -> String {
    if perms.is_empty() {
        return String::new();
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = String::new();
    for p in perms {
        if seen.insert(p.as_str()) {
            out.push_str(&format!(
                "    <uses-permission android:name=\"{}\" />\n",
                escape_xml(p),
            ));
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn render_extra_meta_data(entries: &[MetaDataEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for e in entries {
        out.push_str(&format!(
            "        <meta-data android:name=\"{}\" android:value=\"{}\" />\n",
            escape_xml(&e.name),
            escape_xml(&e.value),
        ));
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Render `<application>`-tag attributes as `android:name="value"`
/// lines, one per attribute, indented to sit under the template's
/// `<application` open tag. Dedup'd by attribute name (LAST writer
/// wins — a later plugin overriding `enableOnBackInvokedCallback`
/// replaces an earlier one). Empty input → empty string.
fn render_extra_application_attributes(attrs: &[ApplicationAttribute]) -> String {
    if attrs.is_empty() {
        return String::new();
    }
    // Keep last-writer-wins while preserving first-seen order for a
    // deterministic, readable manifest.
    let mut order: Vec<&str> = Vec::new();
    let mut by_name: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for a in attrs {
        if by_name.insert(a.name.as_str(), a.value.as_str()).is_none() {
            order.push(a.name.as_str());
        }
    }
    let mut out = String::new();
    for name in order {
        let value = by_name[name];
        out.push_str(&format!(
            "        {}=\"{}\"\n",
            escape_xml(name),
            escape_xml(value),
        ));
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Application class. `HelloWorld` → `HelloWorldApplication`. Strips
/// non-identifier characters and ensures the leading char is alpha.
fn application_class_name(app_name: &str) -> String {
    let cleaned: String = app_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if cleaned.is_empty() {
        return "WhiskerApp_Application".into();
    }
    format!("{cleaned}Application")
}

/// `rootProject.name`. Lowercase, hyphenated form of the app name —
/// e.g. `Podcast` → `podcast-android`. Matches the existing
/// example convention (gradle warns on uppercase project names).
fn project_name(app_name: &str) -> String {
    let mut out = String::new();
    for (i, c) in app_name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(c.to_lowercase());
    }
    if out.is_empty() {
        out.push_str("whisker-app");
    }
    format!("{out}-android")
}

/// Convert `rs.whisker.examples.helloworld` → `rs/whisker/examples/helloworld`.
/// Used to build the on-disk path under `app/src/main/kotlin/`.
fn application_id_to_path(application_id: &str) -> PathBuf {
    application_id
        .split('.')
        .filter(|s| !s.is_empty())
        .fold(PathBuf::new(), |acc, seg| acc.join(seg))
}

fn write_files(out_dir: &Path, inputs: &AndroidInputs) -> Result<()> {
    let vars = template_vars(inputs);

    // Wipe the existing tree, but spare anything we know is a runtime
    // build artifact (so we don't blow away gradle's cache on every
    // sync). Today that means `app/build/`, `.gradle/`, and the
    // `app/src/main/jniLibs/` directory whose bytes are produced by
    // `cargo build` outside the renderer.
    clean_managed_tree(out_dir).context("clean previous gen tree")?;

    let kotlin_pkg = out_dir
        .join("app/src/main/kotlin")
        .join(application_id_to_path(&inputs.application_id));

    let app_class_filename = format!("{}.kt", application_class_name(&inputs.app_name));

    // Text templates.
    let text_files: &[(PathBuf, &str)] = &[
        (out_dir.join("app/build.gradle.kts"), APP_BUILD_GRADLE_KTS),
        (
            out_dir.join("app/src/main/AndroidManifest.xml"),
            APP_MANIFEST_XML,
        ),
        (kotlin_pkg.join("MainActivity.kt"), MAIN_ACTIVITY_KT),
        (kotlin_pkg.join(&app_class_filename), APPLICATION_KT),
        (out_dir.join("build.gradle.kts"), ROOT_BUILD_GRADLE_KTS),
        (out_dir.join("settings.gradle.kts"), SETTINGS_GRADLE_KTS),
        (out_dir.join("gradle.properties"), GRADLE_PROPERTIES),
        (
            out_dir.join("gradle/wrapper/gradle-wrapper.properties"),
            GRADLE_WRAPPER_PROPERTIES,
        ),
    ];
    for (path, template) in text_files {
        let rendered =
            render(template, &vars).with_context(|| format!("render {}", path.display()))?;
        write_file(path, rendered.as_bytes(), false)?;
    }

    // `gradlew` is shell — needs +x.
    write_file(&out_dir.join("gradlew"), GRADLEW.as_bytes(), true)?;
    write_file(&out_dir.join("gradlew.bat"), GRADLEW_BAT.as_bytes(), false)?;

    // Binary.
    write_file(
        &out_dir.join("gradle/wrapper/gradle-wrapper.jar"),
        GRADLE_WRAPPER_JAR,
        false,
    )?;

    // Plugin-supplied `extra_files`. Paths are validated to be
    // relative and traversal-free; on Unix, `mode` is applied via
    // the existing `write_file` executable flag (0o755 when set
    // and `>= 0o100`, otherwise the default 0o644).
    for (rel, entry) in &inputs.extra_files {
        crate::render::validate_extra_file_path(rel).with_context(|| {
            format!(
                "extra_files entry `{}` (Android plugin contribution)",
                rel.display(),
            )
        })?;
        let abs = out_dir.join(rel);
        // The Android renderer's `write_file` takes a `bool`
        // executable flag (0o755 on Unix). Apply that for any
        // mode that has the user-execute bit set.
        let executable = entry.mode.map(|m| m & 0o100 != 0).unwrap_or(false);
        let bytes = entry
            .to_bytes()
            .with_context(|| format!("decode extra_files entry `{}` contents", rel.display()))?;
        write_file(&abs, &bytes, executable)?;
    }

    Ok(())
}

/// Delete the previous gen tree but keep `app/build/`, `.gradle/`,
/// and `app/src/main/jniLibs/`. These three are runtime build
/// artifacts; wiping them on every sync forces gradle into a cold
/// rebuild and `cargo build` to re-copy the dylib, which would make
/// the dev loop unbearable.
fn clean_managed_tree(out_dir: &Path) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    let keep = ["app/build", ".gradle", "app/src/main/jniLibs"];
    for entry in
        std::fs::read_dir(out_dir).with_context(|| format!("read_dir {}", out_dir.display()))?
    {
        let entry = entry?;
        let rel = entry
            .path()
            .strip_prefix(out_dir)
            .map(|p| p.to_path_buf())
            .ok();
        if let Some(rel) = rel {
            if keep.iter().any(|k| rel == Path::new(k)) {
                continue;
            }
        }
        // Don't blow away top-level `app/` either — only the files we
        // own under it. Recurse one level.
        if entry.file_name() == "app" && entry.path().is_dir() {
            clean_under_app(&entry.path())?;
            continue;
        }
        // Skip our own fingerprint file — it'll be overwritten in `sync`.
        if entry.file_name() == ".whisker-fingerprint" {
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn clean_under_app(app_dir: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(app_dir).with_context(|| format!("read_dir {}", app_dir.display()))?
    {
        let entry = entry?;
        // Keep `build/` (gradle's output) and the jniLibs subtree.
        if entry.file_name() == "build" {
            continue;
        }
        if entry.path().is_dir() && entry.file_name() == "src" {
            clean_under_src(&entry.path())?;
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn clean_under_src(src_dir: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(src_dir).with_context(|| format!("read_dir {}", src_dir.display()))?
    {
        let entry = entry?;
        if entry.path().is_dir() && entry.file_name() == "main" {
            clean_under_main(&entry.path())?;
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn clean_under_main(main_dir: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(main_dir).with_context(|| format!("read_dir {}", main_dir.display()))?
    {
        let entry = entry?;
        // Keep the jniLibs subtree (dylib drops here).
        if entry.file_name() == "jniLibs" {
            continue;
        }
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

fn write_file(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// Pull the Android-relevant subset of `Config` into the renderer
/// input struct. Errors out on required-but-missing fields (an
/// applicationId is mandatory; everything else has a default).
///
/// Thin wrapper over [`inputs_from_with_engine`] using
/// [`Engine::with_builtins`]. Callers that want extra plugins
/// (e.g. subprocess plugins discovered via cargo metadata) should
/// call the `_with_engine` form directly.
// Eight arguments — over clippy's seven-arg default. Bundling them
// behind a builder or a config struct would just push the same value
// list one level deeper without changing the call site, so allow.
#[allow(clippy::too_many_arguments)]
pub fn inputs_from(
    app_config: &Config,
    rust_lib_name: String,
    whisker_workspace_path: PathBuf,
    whisker_user_package: String,
    whisker_sdk_version: String,
    whisker_gradle_plugin_version: String,
    whisker_maven_url: String,
    lynx_maven_url: String,
) -> Result<AndroidInputs> {
    inputs_from_with_engine(
        &Engine::with_builtins(),
        app_config,
        rust_lib_name,
        whisker_workspace_path,
        whisker_user_package,
        whisker_sdk_version,
        whisker_gradle_plugin_version,
        whisker_maven_url,
        lynx_maven_url,
    )
}

/// Like [`inputs_from`] but takes a pre-built [`Engine`] so the
/// caller can register additional plugins (e.g. subprocess plugins
/// discovered from `[package.metadata.whisker.plugins]`).
#[allow(clippy::too_many_arguments)]
pub fn inputs_from_with_engine(
    engine: &Engine,
    app_config: &Config,
    rust_lib_name: String,
    whisker_workspace_path: PathBuf,
    whisker_user_package: String,
    whisker_sdk_version: String,
    whisker_gradle_plugin_version: String,
    whisker_maven_url: String,
    lynx_maven_url: String,
) -> Result<AndroidInputs> {
    // Run the plugin pipeline. `build_initial_context` seeds the
    // IR with core fields from `Config`; plugins can override
    // any of them. The renderer reads the post-pipeline IR.
    let ctx = engine
        .compose(app_config, EnabledTargets::android_only())
        .context("compose Whisker CNG plugin pipeline for Android")?;
    let android_ir = ctx
        .android
        .as_ref()
        .expect("EnabledTargets::android_only guarantees Some");

    let app_name = android_ir
        .app_name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required"))?;
    let version = android_ir
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let build_number = android_ir.build_number.unwrap_or(1);
    let application_id = android_ir.application_id.clone().ok_or_else(|| {
        anyhow!(
            "whisker.rs: app.android(|a| a.application_id(\"…\")) (or app.bundle_id) is required for Android"
        )
    })?;
    let min_sdk = android_ir.min_sdk.unwrap_or(24);
    let target_sdk = android_ir.target_sdk.unwrap_or(34);

    let extra_permissions = android_ir.manifest.permissions.clone();
    let extra_meta_data = android_ir.manifest.application_meta_data.clone();
    let extra_application_attributes = android_ir.manifest.application_attributes.clone();
    let extra_gradle_plugins = android_ir.gradle.apply_plugins.clone();
    let extra_gradle_dependencies = android_ir.gradle.dependencies.clone();
    let extra_files = android_ir.extra_files.clone();

    Ok(AndroidInputs {
        app_name,
        version,
        build_number,
        application_id,
        min_sdk,
        target_sdk,
        rust_lib_name,
        whisker_workspace_path,
        whisker_user_package,
        whisker_sdk_version,
        whisker_gradle_plugin_version,
        whisker_maven_url,
        lynx_maven_url,
        extra_permissions,
        extra_meta_data,
        extra_application_attributes,
        extra_gradle_plugins,
        extra_gradle_dependencies,
        extra_files,
        // Bumped 9 → 10 for `<application>` attribute injection
        // (`extra_application_attributes`): the template's
        // `<application` open tag grew a placeholder + the inputs/
        // fingerprint shape grew a field, so existing trees regenerate.
        template_version: 10,
    })
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
        let p = std::env::temp_dir().join(format!("whisker-cng-android-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_inputs() -> AndroidInputs {
        AndroidInputs {
            app_name: "HelloWorld".into(),
            version: "0.1.0".into(),
            build_number: 1,
            application_id: "rs.whisker.examples.helloworld".into(),
            min_sdk: 24,
            target_sdk: 34,
            rust_lib_name: "hello_world".into(),
            whisker_workspace_path: PathBuf::from("../.."),
            whisker_user_package: "hello-world".into(),
            whisker_sdk_version: "0.1.0".into(),
            whisker_gradle_plugin_version: "0.1.0".into(),
            whisker_maven_url: "https://whiskerrs.github.io/whisker/maven".into(),
            lynx_maven_url: "https://whiskerrs.github.io/lynx/maven".into(),
            extra_permissions: Vec::new(),
            extra_meta_data: Vec::new(),
            extra_application_attributes: Vec::new(),
            extra_gradle_plugins: Vec::new(),
            extra_gradle_dependencies: Vec::new(),
            extra_files: BTreeMap::new(),
            template_version: 10,
        }
    }

    #[test]
    fn extra_files_writes_binary_contents_via_base64() {
        // whisker-asset drops assets under app/src/main/assets/whisker/
        // as base64 FileEntry::binary — the renderer must decode them.
        let mut inputs = sample_inputs();
        let raw = vec![0x00u8, 0x01, 0xfe, 0xff];
        inputs.extra_files.insert(
            PathBuf::from("app/src/main/assets/whisker/images/logo.png"),
            FileEntry::binary(&raw),
        );
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &inputs).unwrap();
        let written =
            std::fs::read(out.join("app/src/main/assets/whisker/images/logo.png")).unwrap();
        assert_eq!(written, raw);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn template_vars_carry_required_keys() {
        let inputs = sample_inputs();
        let vars = template_vars(&inputs);
        assert_eq!(
            vars["android_application_id"],
            "rs.whisker.examples.helloworld"
        );
        assert_eq!(vars["android_application_class"], "HelloWorldApplication");
        assert_eq!(vars["android_min_sdk"], "24");
        assert_eq!(vars["android_target_sdk"], "34");
        assert_eq!(vars["rust_lib_name"], "hello_world");
        assert_eq!(vars["build_number"], "1");
        assert_eq!(vars["version"], "0.1.0");
    }

    #[test]
    fn application_class_strips_punctuation() {
        assert_eq!(
            application_class_name("Hello World"),
            "HelloWorldApplication"
        );
        assert_eq!(application_class_name("My-App"), "MyAppApplication");
    }

    #[test]
    fn project_name_lowercases_and_appends_android_suffix() {
        assert_eq!(project_name("HelloWorld"), "hello-world-android");
    }

    #[test]
    fn application_id_to_path_splits_on_dots() {
        assert_eq!(
            application_id_to_path("rs.whisker.examples.helloworld"),
            PathBuf::from("rs/whisker/examples/helloworld"),
        );
    }

    #[test]
    fn sync_writes_known_files_to_out_dir() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        let regenerated = sync(&out, &sample_inputs()).expect("sync");
        assert!(regenerated);

        for expected in [
            "app/build.gradle.kts",
            "app/src/main/AndroidManifest.xml",
            "app/src/main/kotlin/rs/whisker/examples/helloworld/MainActivity.kt",
            "app/src/main/kotlin/rs/whisker/examples/helloworld/HelloWorldApplication.kt",
            "build.gradle.kts",
            "settings.gradle.kts",
            "gradle.properties",
            "gradlew",
            "gradlew.bat",
            "gradle/wrapper/gradle-wrapper.properties",
            "gradle/wrapper/gradle-wrapper.jar",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(expected).exists(), "missing: {expected}");
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_substitutes_placeholders_in_generated_files() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();

        let manifest =
            std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(manifest.contains("android:name=\".HelloWorldApplication\""));
        assert!(manifest.contains("android:label=\"HelloWorld\""));
        assert!(!manifest.contains("{{"));

        let main_activity = std::fs::read_to_string(
            out.join("app/src/main/kotlin/rs/whisker/examples/helloworld/MainActivity.kt"),
        )
        .unwrap();
        assert!(main_activity.starts_with("package rs.whisker.examples.helloworld\n"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn application_attributes_render_on_the_application_tag() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        let mut inputs = sample_inputs();
        inputs.extra_application_attributes = vec![
            ApplicationAttribute {
                name: "android:enableOnBackInvokedCallback".into(),
                value: "true".into(),
            },
            // Duplicate name → last-writer-wins, rendered once.
            ApplicationAttribute {
                name: "android:enableOnBackInvokedCallback".into(),
                value: "true".into(),
            },
        ];
        sync(&out, &inputs).unwrap();

        let manifest =
            std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(
            manifest.contains("android:enableOnBackInvokedCallback=\"true\""),
            "attribute should appear in the manifest:\n{manifest}"
        );
        // Rendered exactly once (dedup by name) and inside the
        // `<application …>` open tag (before the first child element).
        assert_eq!(
            manifest
                .matches("android:enableOnBackInvokedCallback")
                .count(),
            1,
            "deduped to a single occurrence"
        );
        let app_open = manifest.find("<application").unwrap();
        // First `>` at or after the `<application` open tag closes it.
        let app_close = app_open + manifest[app_open..].find('>').unwrap();
        let attr_pos = manifest.find("enableOnBackInvokedCallback").unwrap();
        assert!(
            attr_pos > app_open && attr_pos < app_close,
            "attribute must sit inside the <application …> open tag"
        );
        assert!(!manifest.contains("{{"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_is_idempotent_when_fingerprint_matches() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        let first = sync(&out, &sample_inputs()).unwrap();
        assert!(first);
        let second = sync(&out, &sample_inputs()).unwrap();
        assert!(!second, "second sync should be a no-op");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_regenerates_when_inputs_change() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();
        let mut next = sample_inputs();
        next.target_sdk = 35;
        let regenerated = sync(&out, &next).unwrap();
        assert!(regenerated);
        let app_gradle = std::fs::read_to_string(out.join("app/build.gradle.kts")).unwrap();
        assert!(app_gradle.contains("compileSdk = 35"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_preserves_jnilibs_across_regeneration() {
        // The dylib `cargo build` drops here must survive a sync.
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();
        let jni = out.join("app/src/main/jniLibs/arm64-v8a");
        std::fs::create_dir_all(&jni).unwrap();
        let dylib = jni.join("libhello_world.so");
        std::fs::write(&dylib, b"FAKE_DYLIB").unwrap();

        let mut next = sample_inputs();
        next.min_sdk = 25;
        sync(&out, &next).unwrap();
        assert!(dylib.exists(), "dylib was wiped by re-sync");
        assert_eq!(std::fs::read(&dylib).unwrap(), b"FAKE_DYLIB");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inputs_from_errors_when_application_id_unset() {
        let cfg = Config {
            name: Some("X".into()),
            ..Config::default()
        };
        let err = inputs_from(
            &cfg,
            "x".into(),
            PathBuf::new(),
            "x".into(),
            "0.1.0".into(),
            "0.1.0".into(),
            "https://whiskerrs.github.io/whisker/maven".into(),
            "https://whiskerrs.github.io/lynx/maven".into(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("application_id"), "got: {err:#}");
    }
}
