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
//! │       └── kotlin/<package-path>/MainActivity.kt
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

// Text templates go through `{{placeholder}}` substitution; the gradle
// wrapper jar is copied verbatim, and `gradlew` needs the +x bit on
// Unix, so each group is written separately below.

const APP_BUILD_GRADLE_KTS: &str = include_str!("templates/android/app/build.gradle.kts");
const APP_MANIFEST_XML: &str = include_str!("templates/android/app/src/main/AndroidManifest.xml");
const MAIN_ACTIVITY_KT: &str =
    include_str!("templates/android/app/src/main/kotlin/MainActivity.kt");
const ROOT_BUILD_GRADLE_KTS: &str = include_str!("templates/android/build.gradle.kts");
const SETTINGS_GRADLE_KTS: &str = include_str!("templates/android/settings.gradle.kts");
const GRADLE_PROPERTIES: &str = include_str!("templates/android/gradle.properties");
const GRADLEW: &str = include_str!("templates/android/gradlew");
const GRADLEW_BAT: &str = include_str!("templates/android/gradlew.bat");
const GRADLE_WRAPPER_PROPERTIES: &str =
    include_str!("templates/android/gradle/wrapper/gradle-wrapper.properties");
const GRADLE_WRAPPER_JAR: &[u8] =
    include_bytes!("templates/android/gradle/wrapper/gradle-wrapper.jar");

/// Inputs the Android renderer pulls out of `Config`, plus a few
/// values the cli passes in (the dylib name, the workspace location).
/// Struct rather than a tuple so the fingerprint serialization stays
/// stable as fields are added.
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
    /// Version for `rs.whisker:whisker-runtime-android:<this>` and its
    /// sibling SDK coordinates.
    pub whisker_sdk_version: String,
    /// `rs.whisker:rs.whisker.gradle.plugin:<this>` version pinned in
    /// `pluginManagement.plugins`. Independent of `whisker_sdk_version`
    /// — gradle-plugin and SDK release on separate `gradle-plugin-v*` /
    /// `sdk-v*` tag streams.
    pub whisker_gradle_plugin_version: String,
    /// gh-pages Maven URL hosting Whisker's plugins + SDK. Templates
    /// declare it in both `pluginManagement.repositories` and
    /// `dependencyResolutionManagement.repositories`.
    pub whisker_maven_url: String,
    /// `<uses-permission android:name="…"/>` rows from the engine's
    /// post-pipeline IR, emitted after the template's hardcoded
    /// `INTERNET` permission and dedup'd across plugins.
    #[serde(default)]
    pub extra_permissions: Vec<String>,
    /// `<meta-data android:name="…" android:value="…"/>` rows from the
    /// engine's post-pipeline IR, emitted inside `<application>` in
    /// insertion order.
    #[serde(default)]
    pub extra_meta_data: Vec<MetaDataEntry>,
    /// Attributes on the `<application>` tag itself (e.g.
    /// `android:enableOnBackInvokedCallback="true"`) from the engine's
    /// post-pipeline IR. Dedup'd by attribute name (last writer wins).
    #[serde(default)]
    pub extra_application_attributes: Vec<ApplicationAttribute>,
    /// `MainActivity` deep-link schemes. See `AndroidManifest::main_activity_url_schemes`.
    #[serde(default)]
    pub main_activity_url_schemes: Vec<String>,
    /// Overrides `<application android:theme>`. `None` → the template
    /// default. See `AndroidManifest::application_theme`.
    #[serde(default)]
    pub android_theme: Option<String>,
    /// Extra `MainActivity.kt` imports. See
    /// `AndroidManifest::main_activity_imports`.
    #[serde(default)]
    pub main_activity_imports: Vec<String>,
    /// `MainActivity.onCreate` statements before `super.onCreate`. See
    /// `AndroidManifest::main_activity_pre_super`.
    #[serde(default)]
    pub main_activity_pre_super: Vec<String>,
    /// `MainActivity.onCreate` statements after `super.onCreate`. See
    /// `AndroidManifest::main_activity_post_super`.
    #[serde(default)]
    pub main_activity_post_super: Vec<String>,
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
    /// Keys are relative paths (validated at write time); values are
    /// [`FileEntry`]s — UTF-8 contents + optional POSIX mode.
    ///
    /// Mode is coarser here than in the iOS renderer: `write_file`
    /// takes a `bool` executable flag, so any mode with the
    /// user-execute bit set becomes 0o755 and everything else 0o644.
    #[serde(default)]
    pub extra_files: BTreeMap<PathBuf, FileEntry>,
    /// Bump whenever the template *shape* changes (added file, renamed
    /// placeholder, …). The fingerprint mixes this in, so without a
    /// bump existing `gen/` trees keep their stale output.
    pub template_version: u32,
}

/// Render the Android project into `out_dir` (typically
/// `<crate_dir>/gen/android`). Returns whether files were actually
/// rewritten — `false` means the cached fingerprint matched and the
/// existing tree was reused.
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

/// Build the `{{var}}` table from `inputs`.
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
        "main_activity_launch_mode",
        if inputs.main_activity_url_schemes.is_empty() {
            String::new()
        } else {
            "\n            android:launchMode=\"singleTask\"".to_string()
        },
    );
    v.insert(
        "extra_main_activity_intent_filter",
        render_main_activity_intent_filter(&inputs.main_activity_url_schemes),
    );
    v.insert(
        "extra_gradle_plugins",
        render_extra_gradle_plugins(&inputs.extra_gradle_plugins),
    );
    v.insert(
        "extra_gradle_dependencies",
        render_extra_gradle_dependencies(&inputs.extra_gradle_dependencies),
    );
    v.insert(
        "android_theme",
        inputs
            .android_theme
            .clone()
            .unwrap_or_else(|| "@android:style/Theme.Material.Light.NoActionBar".to_string()),
    );
    let (main_activity_imports, main_activity_pre_super, main_activity_post_super) =
        render_main_activity(
            &inputs.main_activity_imports,
            &inputs.main_activity_pre_super,
            &inputs.main_activity_post_super,
        );
    v.insert("main_activity_imports", main_activity_imports);
    v.insert("main_activity_pre_super", main_activity_pre_super);
    v.insert("main_activity_post_super", main_activity_post_super);
    v
}

/// Render `MainActivity.kt`'s extra imports + `onCreate` override body.
///
/// Returns `(imports, pre_super, post_super)`:
/// - `imports` — extra `import` lines to append after the baseline
///   Android imports (each prefixed with a leading `\n`), or
///   empty. `android.os.Bundle` is already part of the shell template.
/// - `pre_super` / `post_super` — statements indented for the generated
///   `onCreate`. The native shell always owns the method body.
fn render_main_activity(
    imports: &[String],
    pre_super: &[String],
    post_super: &[String],
) -> (String, String, String) {
    let mut import_lines: Vec<String> = Vec::new();
    for i in imports {
        if i != "android.os.Bundle" && !import_lines.contains(i) {
            import_lines.push(i.clone());
        }
    }
    let imports_str = import_lines
        .iter()
        .map(|i| format!("\nimport {i}"))
        .collect::<String>();

    let indent = |lines: &[String]| {
        lines
            .iter()
            .map(|l| format!("        {l}\n"))
            .collect::<String>()
    };
    (imports_str, indent(pre_super), indent(post_super))
}

/// Render `apply_plugins` entries as Kotlin DSL lines inside the
/// `plugins { … }` block. Two shapes:
///
///   - Bare gradle plugin id (e.g. `"com.google.gms.google-services"`)
///     → wrapped in `id("…")`.
///   - Anything containing a `(` character (e.g. `id("…") version "X"`,
///     `alias(libs.plugins.foo)`, `kotlin("jvm")`) → emitted
///     verbatim. Bare gradle plugin ids never contain `(`, which is
///     what makes the character a safe discriminator.
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

/// A second `<intent-filter>` for `MainActivity` catching each
/// scheme's `VIEW` deep links. Empty input → empty string.
fn render_main_activity_intent_filter(schemes: &[String]) -> String {
    if schemes.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n            <intent-filter>\n                <action android:name=\"android.intent.action.VIEW\" />\n                <category android:name=\"android.intent.category.DEFAULT\" />\n                <category android:name=\"android.intent.category.BROWSABLE\" />\n",
    );
    for scheme in schemes {
        out.push_str(&format!(
            "                <data android:scheme=\"{}\" />\n",
            escape_xml(scheme),
        ));
    }
    out.push_str("            </intent-filter>");
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

    clean_managed_tree(out_dir).context("clean previous gen tree")?;

    let kotlin_pkg = out_dir
        .join("app/src/main/kotlin")
        .join(application_id_to_path(&inputs.application_id));

    let text_files: &[(PathBuf, &str)] = &[
        (out_dir.join("app/build.gradle.kts"), APP_BUILD_GRADLE_KTS),
        (
            out_dir.join("app/src/main/AndroidManifest.xml"),
            APP_MANIFEST_XML,
        ),
        (kotlin_pkg.join("MainActivity.kt"), MAIN_ACTIVITY_KT),
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

    write_file(
        &out_dir.join("gradle/wrapper/gradle-wrapper.jar"),
        GRADLE_WRAPPER_JAR,
        false,
    )?;

    for (rel, entry) in &inputs.extra_files {
        crate::render::validate_extra_file_path(rel).with_context(|| {
            format!(
                "extra_files entry `{}` (Android plugin contribution)",
                rel.display(),
            )
        })?;
        let abs = out_dir.join(rel);
        let executable = entry.mode.map(|m| m & 0o100 != 0).unwrap_or(false);
        let bytes = entry
            .to_bytes()
            .with_context(|| format!("decode extra_files entry `{}` contents", rel.display()))?;
        write_file(&abs, &bytes, executable)?;
    }

    Ok(())
}

/// Delete the previous gen tree but keep `app/build/`, `.gradle/`, and
/// `app/src/main/jniLibs/` — wiping those forces a cold gradle rebuild
/// and a dylib re-copy on every sync.
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
        // Only the files we own under `app/`; recurse one level.
        if entry.file_name() == "app" && entry.path().is_dir() {
            clean_under_app(&entry.path())?;
            continue;
        }
        // `sync` overwrites the fingerprint itself.
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
        // Keep gradle's `build/` output and the jniLibs subtree.
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
        // Keep the jniLibs subtree — the dylib drops here.
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
/// [`Engine::with_builtins`] — call that form directly to register
/// additional plugins.
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
) -> Result<AndroidInputs> {
    // The engine seeds the IR from `Config` and plugins may override
    // any of it, so everything below reads the post-pipeline IR.
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
    let main_activity_url_schemes = android_ir.manifest.main_activity_url_schemes.clone();
    let android_theme = android_ir.manifest.application_theme.clone();
    let main_activity_imports = android_ir.manifest.main_activity_imports.clone();
    let main_activity_pre_super = android_ir.manifest.main_activity_pre_super.clone();
    let main_activity_post_super = android_ir.manifest.main_activity_post_super.clone();
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
        extra_permissions,
        extra_meta_data,
        extra_application_attributes,
        main_activity_url_schemes,
        android_theme,
        main_activity_imports,
        main_activity_pre_super,
        main_activity_post_super,
        extra_gradle_plugins,
        extra_gradle_dependencies,
        extra_files,
        template_version: 34,
    })
}

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
            extra_permissions: Vec::new(),
            extra_meta_data: Vec::new(),
            extra_application_attributes: Vec::new(),
            main_activity_url_schemes: Vec::new(),
            android_theme: None,
            main_activity_imports: Vec::new(),
            main_activity_pre_super: Vec::new(),
            main_activity_post_super: Vec::new(),
            extra_gradle_plugins: Vec::new(),
            extra_gradle_dependencies: Vec::new(),
            extra_files: BTreeMap::new(),
            template_version: 34,
        }
    }

    #[test]
    fn generated_activity_only_composes_the_sdk_view() {
        assert!(MAIN_ACTIVITY_KT.contains("import androidx.activity.ComponentActivity"));
        assert!(MAIN_ACTIVITY_KT.contains("class MainActivity : ComponentActivity()"));
        assert!(MAIN_ACTIVITY_KT.contains("import rs.whisker.runtime.WhiskerView"));
        assert!(MAIN_ACTIVITY_KT.contains("WhiskerWindow.enableEdgeToEdge(this)"));
        assert!(MAIN_ACTIVITY_KT.contains("setContentView(WhiskerView(this))"));
        assert!(APP_BUILD_GRADLE_KTS.contains("androidx.activity:activity:1.8.2"));
    }

    #[test]
    fn extra_files_writes_binary_contents_via_base64() {
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
    fn android_theme_defaults_and_overrides() {
        let mut inputs = sample_inputs();
        assert_eq!(
            template_vars(&inputs)["android_theme"],
            "@android:style/Theme.Material.Light.NoActionBar"
        );
        inputs.android_theme = Some("@style/Theme.App.Splash".into());
        assert_eq!(
            template_vars(&inputs)["android_theme"],
            "@style/Theme.App.Splash"
        );
    }

    #[test]
    fn main_activity_has_no_injections_by_default() {
        let (imports, pre, post) = render_main_activity(&[], &[], &[]);
        assert_eq!(imports, "");
        assert_eq!(pre, "");
        assert_eq!(post, "");
    }

    #[test]
    fn main_activity_injects_oncreate_with_pre_super() {
        let (imports, pre, post) = render_main_activity(
            &["androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen".into()],
            &["installSplashScreen()".into()],
            &[],
        );
        assert!(imports.contains(
            "\nimport androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen"
        ));
        assert_eq!(pre, "        installSplashScreen()\n");
        assert_eq!(post, "");
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
        assert!(
            !out.join("app/src/main/kotlin/rs/whisker/runtime/WhiskerView.kt")
                .exists(),
            "the generated app must consume WhiskerView from the Android SDK"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_substitutes_placeholders_in_generated_files() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();

        let manifest =
            std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(!manifest.contains("android:name=\".HelloWorldApplication\""));
        assert!(manifest.contains("android:label=\"HelloWorld\""));
        assert!(!manifest.contains("{{"));
        // The activity opts out of system keyboard avoidance — the app
        // lays out around the IME inset itself.
        assert!(manifest.contains("android:windowSoftInputMode=\"adjustResize\""));

        let main_activity = std::fs::read_to_string(
            out.join("app/src/main/kotlin/rs/whisker/examples/helloworld/MainActivity.kt"),
        )
        .unwrap();
        assert!(main_activity.starts_with("package rs.whisker.examples.helloworld\n"));

        let settings = std::fs::read_to_string(out.join("settings.gradle.kts")).unwrap();
        assert!(!settings.contains("lynx"));
        assert_eq!(
            settings
                .matches("maven { url = uri(\"https://whiskerrs.github.io/whisker/maven\") }")
                .count(),
            2,
            "the published SDK repository must resolve plugins and AARs",
        );
        assert!(settings.contains("rs.whisker:ksp"));
        assert!(settings.contains("id(\"rs.whisker\") version \"0.1.0\""));
        assert!(settings.contains("includeBuild(localGradlePlugin)"));
        assert!(settings.contains("userPackage = \"hello-world\""));

        let app_gradle = std::fs::read_to_string(out.join("app/build.gradle.kts")).unwrap();
        assert!(app_gradle.contains("id(\"rs.whisker.gradle\")"));
        assert!(!app_gradle.contains("whisker_module_deps.gradle.kts"));

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
        assert_eq!(
            manifest
                .matches("android:enableOnBackInvokedCallback")
                .count(),
            1,
            "deduped to a single occurrence"
        );
        let app_open = manifest.find("<application").unwrap();
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
    fn render_main_activity_intent_filter_empty_is_empty() {
        assert_eq!(render_main_activity_intent_filter(&[]), "");
    }

    #[test]
    fn render_main_activity_intent_filter_includes_scheme() {
        let out = render_main_activity_intent_filter(&["giga".to_string()]);
        assert!(out.contains("android:scheme=\"giga\""));
        assert!(out.contains("android.intent.action.VIEW"));
    }

    #[test]
    fn template_vars_set_launch_mode_only_when_scheme_present() {
        let mut inputs = sample_inputs();
        assert_eq!(template_vars(&inputs)["main_activity_launch_mode"], "");
        inputs.main_activity_url_schemes = vec!["giga".to_string()];
        assert!(template_vars(&inputs)["main_activity_launch_mode"].contains("singleTask"));
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
        )
        .unwrap_err();
        assert!(err.to_string().contains("application_id"), "got: {err:#}");
    }
}
