//! Android backend: serialize declarations and stage declared files, without
//! introducing application policy. Build-tool compatibility remains native.
use super::*;
use crate::project_files::{check_destination, insert};
use anyhow::ensure;
use whisker_plugin::project::*;

/// A final Android declaration plus generation context outside the native model.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AndroidProjectInputs {
    pub project: AndroidProjectIr,
    pub app_crate_dir: Option<PathBuf>,
    pub cargo_selection: crate::CargoSelection,
    pub template_version: u32,
}

/// Materialize every declared output before touching the previous tree.
/// Copied input bytes participate in the fingerprint, including directory contents.
pub fn sync_project(out: &Path, inputs: &AndroidProjectInputs) -> Result<bool> {
    let files = render_project(inputs)?;
    let bytes = serde_json::to_vec(&(inputs, &files, 1u32))?;
    let fp = fingerprint::fingerprint(&bytes);
    let fp_path = out.join(".whisker-fingerprint");
    if std::fs::read_to_string(&fp_path)
        .ok()
        .is_some_and(|s| s.trim() == fp)
    {
        return Ok(false);
    }
    for path in files.keys() {
        check_destination(out, &out.join(path.as_str()))?;
    }
    clean_managed_tree(out)?;
    for (path, entry) in files {
        let path = out.join(path.as_str());
        write_file(
            &path,
            &entry.to_bytes()?,
            entry.mode.is_some_and(|m| m & 0o100 != 0),
        )?;
        #[cfg(unix)]
        if let Some(mode) = entry.mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    std::fs::write(fp_path, fp)?;
    Ok(true)
}

/// Render structured settings and stage resources. Reject overlapping ownership
/// instead of silently allowing a staged file to mask a generated build/manifest.
pub fn render_project(inputs: &AndroidProjectInputs) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    ProjectIr::Android(Box::new(inputs.project.clone())).validate_structure()?;
    let p = &inputs.project;
    let mut files = BTreeMap::new();
    put(&mut files, "settings.gradle.kts", settings(p)?)?;
    put(&mut files, "build.gradle.kts", script(&p.root_build, "")?)?;
    let mut properties = String::new();
    for (k, v) in &p.properties {
        properties.push_str(&format!("{}={}\n", property(k), property(v)));
    }
    put(&mut files, "gradle.properties", properties)?;
    for (id, m) in &p.modules {
        if !matches!(m.kind, AndroidModuleKind::External { .. }) {
            put(
                &mut files,
                &format!("{}/build.gradle.kts", m.directory.as_str()),
                module(m).with_context(|| format!("render module {id}"))?,
            )?;
        }
        if let Some(build) = android_build(&m.kind) {
            for (name, source) in &build.source_sets {
                if let Some(manifest) = &source.manifest {
                    put(
                        &mut files,
                        &format!("{}/src/{name}/AndroidManifest.xml", m.directory.as_str()),
                        format!(
                            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n{}\n",
                            xml(
                                manifest,
                                &BTreeMap::from([(
                                    "xml".into(),
                                    "http://www.w3.org/XML/1998/namespace".into()
                                )])
                            )?
                        ),
                    )?;
                }
            }
        }
    }
    crate::project_files::stage_declared(&mut files, &p.files, inputs.app_crate_dir.as_deref())?;
    for module in p.modules.values() {
        if let AndroidModuleKind::External { build_file } = &module.kind {
            ensure!(
                files.contains_key(build_file),
                "external module build file is not staged: {}",
                build_file.as_str()
            );
        }
    }
    for path in files.keys() {
        ensure!(
            path.as_str() != ".whisker-fingerprint",
            "reserved generation fingerprint path"
        );
        for (i, _) in path.as_str().match_indices('/') {
            ensure!(
                !files.contains_key(&ProjectPath::new(&path.as_str()[..i])?),
                "overlapping output: {}",
                path.as_str()
            );
        }
    }
    // Decode all generated binaries before deleting the old tree.
    for entry in files.values() {
        entry.to_bytes()?;
    }
    Ok(files)
}
fn put(files: &mut BTreeMap<ProjectPath, FileEntry>, path: &str, text: String) -> Result<()> {
    insert(files, ProjectPath::new(path)?, FileEntry::text(text))
}
pub(crate) fn quote(s: &str) -> String {
    serde_json::to_string(s)
        .expect("string JSON")
        .replace('$', "\\$")
}
fn property(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            '=' | ':' | ' ' | '#' | '!' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}
fn block(name: &str, body: &str) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    format!(
        "{name} {{\n{}}}\n",
        body.lines()
            .map(|l| format!("    {l}\n"))
            .collect::<String>()
    )
}
fn lines(entries: &[String]) -> String {
    if entries.is_empty() {
        String::new()
    } else {
        format!("{}\n", entries.join("\n"))
    }
}
fn plugin(p: &GradlePlugin) -> String {
    let mut s = if let Some(a) = &p.alias {
        format!("alias({a})")
    } else {
        format!("id({})", quote(&p.id))
    };
    if let Some(v) = &p.version {
        s += &format!(" version {}", quote(v));
    }
    if !p.apply {
        s += " apply false";
    }
    s + "\n"
}
fn script(s: &GradleBuildScript, generated: &str) -> Result<String> {
    Ok(format!(
        "{}{}{}{}{}",
        s.imports
            .iter()
            .map(|i| format!("import {i}\n"))
            .collect::<String>(),
        block("buildscript", &lines(&s.buildscript)),
        block(
            "plugins",
            &(s.plugins.iter().map(plugin).collect::<String>() + &lines(&s.plugin_statements))
        ),
        generated,
        lines(&s.statements)
    ))
}
fn settings(p: &AndroidProjectIr) -> Result<String> {
    let s = &p.settings;
    let m = &s.plugin_management;
    let includes = |paths: &[ProjectPath]| {
        paths
            .iter()
            .map(|p| format!("includeBuild({})\n", quote(p.as_str())))
            .collect::<String>()
    };
    let repos = |rs: &[GradleRepository]| {
        block(
            "repositories",
            &rs.iter()
                .map(|r| format!("{}\n", r.expression))
                .collect::<String>(),
        )
    };
    let management = format!(
        "{}{}{}{}{}",
        lines(&m.statements),
        includes(&m.included_builds),
        repos(&m.repositories),
        block(
            "plugins",
            &m.plugins
                .iter()
                .map(|(id, v)| format!("id({}) version {}\n", quote(id), quote(v)))
                .collect::<String>()
        ),
        block("resolutionStrategy", &lines(&m.resolution_strategy))
    );
    let resolution = format!(
        "{}{}{}",
        s.dependency_resolution
            .repositories_mode
            .as_ref()
            .map(|mode| format!(
                "repositoriesMode.set(RepositoriesMode.{})\n",
                match mode {
                    GradleRepositoriesMode::PreferProject => "PREFER_PROJECT",
                    GradleRepositoriesMode::PreferSettings => "PREFER_SETTINGS",
                    GradleRepositoriesMode::FailOnProjectRepos => "FAIL_ON_PROJECT_REPOS",
                }
            ))
            .unwrap_or_default(),
        repos(&s.dependency_resolution.repositories),
        lines(&s.dependency_resolution.statements)
    );
    let mut body = format!(
        "{}{}{}{}{}{}{}",
        s.imports
            .iter()
            .map(|i| format!("import {i}\n"))
            .collect::<String>(),
        block("pluginManagement", &management),
        block("buildscript", &lines(&s.buildscript)),
        block("plugins", &s.plugins.iter().map(plugin).collect::<String>()),
        block("dependencyResolutionManagement", &resolution),
        block("android", &sdk(&s.android_sdk, true)?),
        includes(&s.included_builds)
    );
    for (id, m) in &p.modules {
        body += &format!(
            "include({})\nproject({}).projectDir = file({})\n",
            quote(id),
            quote(id),
            quote(m.directory.as_str())
        );
        if let AndroidModuleKind::External { build_file } = &m.kind {
            let prefix = format!("{}/", m.directory.as_str());
            let name = build_file
                .as_str()
                .strip_prefix(&prefix)
                .context("external build file must be inside its module directory")?;
            ensure!(
                !name.contains('/'),
                "external build file must be directly in its module directory"
            );
            body += &format!("project({}).buildFileName = {}\n", quote(id), quote(name));
        }
    }
    body += &lines(&s.statements);
    Ok(body)
}
pub(crate) fn android_build(kind: &AndroidModuleKind) -> Option<&AndroidBuild> {
    match kind {
        AndroidModuleKind::Application(a) => Some(&a.android),
        AndroidModuleKind::Library(a) => Some(a),
        AndroidModuleKind::DynamicFeature(f) => Some(&f.android),
        AndroidModuleKind::Test(t) => Some(&t.android),
        _ => None,
    }
}
fn sdk(s: &AndroidSdk, runtime: bool) -> Result<String> {
    let mut out = String::new();
    if let Some(compile) = &s.compile {
        match compile {
            AndroidCompileSdk::Release {
                api,
                minor,
                extension,
            } => {
                ensure!(
                    minor.is_none(),
                    "Android renderer does not support minor compile SDK versions; use explicit native DSL"
                );
                out += &format!("compileSdk = {api}\n");
                if let Some(e) = extension {
                    out += &format!("compileSdkExtension = {e}\n");
                }
            }
            AndroidCompileSdk::Preview { codename } => {
                out += &format!("compileSdkPreview = {}\n", quote(codename))
            }
            AndroidCompileSdk::AddOn { vendor, name, api } => {
                out += &format!(
                    "compileSdkAddon({}, {}, {api})\n",
                    quote(vendor),
                    quote(name)
                )
            }
        }
    }
    if runtime {
        for (name, value) in [("minSdk", &s.min), ("targetSdk", &s.target)] {
            if let Some(value) = value {
                out += &match value {
                    AndroidApiLevel::Release(api) => format!("{name} = {api}\n"),
                    AndroidApiLevel::Preview(code) => format!("{name}Preview = {}\n", quote(code)),
                };
            }
        }
    }
    Ok(out)
}
fn values(v: &AndroidVariantValues) -> String {
    let mut out = String::new();
    for (k, v) in &v.manifest_placeholders {
        out += &format!("manifestPlaceholders[{}] = {v}\n", quote(k));
    }
    for (k, v) in &v.build_config_fields {
        out += &format!(
            "buildConfigField({}, {}, {})\n",
            quote(&v.type_name),
            quote(k),
            v.value
        );
    }
    for (ty, entries) in &v.res_values {
        for (k, v) in entries {
            out += &format!("resValue({}, {}, {})\n", quote(ty), quote(k), v);
        }
    }
    out
}
fn string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|v| quote(v))
        .collect::<Vec<_>>()
        .join(", ")
}
fn build(b: &AndroidBuild, m: &AndroidModule) -> Result<String> {
    let mut out = format!(
        "namespace = {}\n{}",
        quote(&b.namespace),
        sdk(&b.sdk, false)?
    );
    let runtime = AndroidSdk {
        compile: None,
        min: b.sdk.min.clone(),
        target: b.sdk.target.clone(),
    };
    let mut default =
        sdk(&runtime, true)? + &values(&b.default_config) + &lines(&b.default_config_statements);
    if let AndroidModuleKind::Application(a) = &m.kind {
        if let Some(id) = &a.application_id {
            default = format!("applicationId = {}\n{default}", quote(id));
        }
        if !a.dynamic_features.is_empty() {
            out += &format!(
                "dynamicFeatures += setOf({})\n",
                string_list(&a.dynamic_features)
            );
        }
        if !a.asset_packs.is_empty() {
            out += &format!("assetPacks += setOf({})\n", string_list(&a.asset_packs));
        }
    }
    if let AndroidModuleKind::Test(t) = &m.kind {
        out += &format!("targetProjectPath = {}\n", quote(&t.target));
    }
    out += &block("defaultConfig", &default);
    if !b.variants.flavor_dimensions.is_empty() {
        out += &format!(
            "flavorDimensions += listOf({})\n",
            string_list(&b.variants.flavor_dimensions)
        );
    }
    let mut types = String::new();
    for (name, v) in &b.variants.build_types {
        let mut body = values(&v.values);
        if !v.matching_fallbacks.is_empty() {
            body += &format!(
                "matchingFallbacks += listOf({})\n",
                string_list(&v.matching_fallbacks)
            );
        }
        body += &lines(&v.statements);
        types += &block(&format!("maybeCreate({}).apply", quote(name)), &body);
    }
    out += &block("buildTypes", &types);
    let mut flavors = String::new();
    for (name, v) in &b.variants.product_flavors {
        let mut body = format!("dimension = {}\n{}", quote(&v.dimension), values(&v.values));
        if !v.matching_fallbacks.is_empty() {
            body += &format!(
                "matchingFallbacks += listOf({})\n",
                string_list(&v.matching_fallbacks)
            );
        }
        for (dimension, fallbacks) in &v.missing_dimension_strategies {
            body += &format!(
                "missingDimensionStrategy({}, {})\n",
                quote(dimension),
                string_list(fallbacks)
            );
        }
        body += &lines(&v.statements);
        flavors += &block(&format!("maybeCreate({}).apply", quote(name)), &body);
    }
    out += &block("productFlavors", &flavors);
    let mut sources = String::new();
    for (name, s) in &b.source_sets {
        ensure!(
            s.baseline_profiles.is_empty(),
            "Android renderer does not support baseline profile source directories; configure the native plugin explicitly"
        );
        let mut body = String::new();
        if s.manifest.is_some() {
            body += &format!(
                "manifest.srcFile(rootProject.file({}))\n",
                quote(&format!(
                    "{}/src/{name}/AndroidManifest.xml",
                    m.directory.as_str()
                ))
            );
        }
        for (field, paths) in [
            ("java", &s.sources),
            ("res", &s.android_resources),
            ("resources", &s.java_resources),
            ("aidl", &s.aidl),
            ("shaders", &s.shaders),
            ("assets", &s.assets),
            ("jniLibs", &s.jni_libraries),
        ] {
            if !paths.is_empty() {
                body += &format!(
                    "{field}.setSrcDirs(listOf({}))\n",
                    paths
                        .iter()
                        .map(|p| format!("rootProject.file({})", quote(p.as_str())))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        sources += &block(&format!("maybeCreate({}).apply", quote(name)), &body);
    }
    out += &block("sourceSets", &sources);
    out += &lines(&b.statements);
    Ok(block("android", &out))
}
fn module(m: &AndroidModule) -> Result<String> {
    let mut generated = String::new();
    if let Some(b) = android_build(&m.kind) {
        generated += &build(b, m)?;
    }
    if let AndroidModuleKind::AssetPack(pack) = &m.kind {
        ensure!(
            pack.assets
                .iter()
                .all(|p| p.as_str() == format!("{}/src/main/assets", m.directory.as_str())),
            "asset pack supports only its native src/main/assets directory"
        );
        generated += &block(
            "assetPack",
            &format!(
                "packName = {}\n{}",
                quote(&pack.pack_name),
                block(
                    "dynamicDelivery",
                    &format!(
                        "deliveryType = {}",
                        quote(match pack.delivery {
                            AssetPackDelivery::InstallTime => "install-time",
                            AssetPackDelivery::FastFollow => "fast-follow",
                            AssetPackDelivery::OnDemand => "on-demand",
                        })
                    )
                )
            ),
        );
    }
    let mut deps = String::new();
    for d in &m.dependencies {
        let source = match &d.source {
            GradleDependencySource::Maven(v) => quote(v),
            GradleDependencySource::Project(v) => format!("project({})", quote(v)),
            GradleDependencySource::Kotlin(v) => v.clone(),
        };
        // add supports configuration names not available as Kotlin accessors.
        deps += &format!("add({}, {source})\n", quote(&d.configuration));
    }
    if let AndroidModuleKind::DynamicFeature(f) = &m.kind {
        deps += &format!("add(\"implementation\", project({}))\n", quote(&f.base));
    }
    generated += &block("dependencies", &deps);
    script(&m.build, &generated)
}
use crate::project_xml::xml;
