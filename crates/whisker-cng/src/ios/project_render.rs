//! Xcode serialization of a completed Apple declaration, without application policy.
use super::*;
use crate::project_files::{check_destination, insert, stage_declared};
use crate::project_plist::plist_xml;
use anyhow::{bail, ensure};
use whisker_plugin::project::*;
mod scheme;

/// Final declarations and filesystem/build context outside the native model.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IosProjectInputs {
    pub project: IosProjectIr,
    pub project_name: String,
    pub app_crate_dir: Option<PathBuf>,
    pub cargo_selection: crate::CargoSelection,
    pub template_version: u32,
}
/// Preflight every output and copied input before replacing the managed tree.
pub fn sync_project(out: &Path, inputs: &IosProjectInputs) -> Result<bool> {
    let files = render_project(inputs)?;
    let fp = fingerprint::fingerprint(&serde_json::to_vec(&(inputs, &files, 1u32))?);
    let stamp = out.join(".whisker-fingerprint");
    if std::fs::read_to_string(&stamp)
        .ok()
        .is_some_and(|v| v.trim() == fp)
    {
        return Ok(false);
    }
    for path in files.keys() {
        check_destination(out, &out.join(path.as_str()))?;
    }
    clean_managed_tree(out, &inputs.project_name)?;
    for (path, entry) in files {
        let path = out.join(path.as_str());
        write_file(&path, &entry.to_bytes()?)?;
        apply_mode(&path, entry.mode)?;
    }
    std::fs::write(stamp, fp)?;
    Ok(true)
}
/// Render all native metadata and stage declared source/resource files.
pub fn render_project(inputs: &IosProjectInputs) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    ProjectIr::Ios(inputs.project.clone()).validate_structure()?;
    component(&inputs.project_name)?;
    let apple = &inputs.project.apple;
    let mut files = BTreeMap::new();
    let mut pbx = Pbx::default();
    let mut targets = Vec::new();
    let mut products = Vec::new();
    for (id, target) in &apple.targets {
        component(id)?;
        component(&target.product_name)?;
        if target.kind == AppleTargetKind::Native {
            let (ext, ty, prefix) = product_kind(target)?;
            let product = format!("{prefix}{}{}", target.product_name, ext);
            let reference = pbx.object(&format!("product:{id}"), &product, format!("isa = PBXFileReference; explicitFileType = {ty}; includeInIndex = 0; path = {}; sourceTree = BUILT_PRODUCTS_DIR;", q(&product)))?;
            products.push(reference);
        }
    }
    let mut packages = Vec::new();
    for (id, package) in &apple.swift_packages {
        let (label, body) = match package {
            SwiftPackage::Local { path } => (
                format!("XCLocalSwiftPackageReference {}", q(path.as_str())),
                format!(
                    "isa = XCLocalSwiftPackageReference; relativePath = {};",
                    q(path.as_str())
                ),
            ),
            SwiftPackage::Remote { url, requirement } => (
                id.clone(),
                format!(
                    "isa = XCRemoteSwiftPackageReference; repositoryURL = {}; requirement = {{ {} }};",
                    q(url),
                    requirement_text(requirement)
                ),
            ),
        };
        packages.push(pbx.object(&format!("package:{id}"), &label, body)?);
    }
    let names: Vec<String> = if apple.configurations.is_empty() {
        vec!["Debug".into(), "Release".into()]
    } else {
        apple.configurations.keys().cloned().collect()
    };
    let default = apple.default_configuration.as_deref().unwrap_or(&names[0]);
    for target in apple.targets.values() {
        for name in target.configurations.keys() {
            ensure!(names.contains(name), "unknown target configuration: {name}");
        }
    }
    for scheme in apple.schemes.values() {
        for name in [
            Some(&scheme.run_configuration),
            Some(&scheme.archive_configuration),
            scheme.test_configuration.as_ref(),
            scheme.profile_configuration.as_ref(),
            scheme.analyze_configuration.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            ensure!(names.contains(name), "unknown scheme configuration: {name}");
        }
    }
    let project_configs = pbx.configurations(
        "project",
        &names,
        default,
        &apple.build_settings,
        &apple.configurations,
    )?;
    for (id, target) in &apple.targets {
        targets.push(pbx.target(id, target, apple, &names, default, &mut files)?);
    }
    let product_group = pbx.object(
        "products",
        "Products",
        format!(
            "isa = PBXGroup; children = {}; name = Products; sourceTree = {};\n",
            list(&products),
            q("<group>")
        ),
    )?;
    let navigator = pbx.object(
        "files",
        "Files",
        format!(
            "isa = PBXGroup; children = {}; name = {}; sourceTree = {};",
            list(&pbx.file_refs.values().cloned().collect::<Vec<_>>()),
            q("Files"),
            q("<group>")
        ),
    )?;
    let main = pbx.object(
        "main-group",
        "Main",
        format!(
            "isa = PBXGroup; children = {}; sourceTree = {};",
            list(&[navigator, product_group.clone()]),
            q("<group>")
        ),
    )?;
    let root = pbx.object("project", "Project object", format!("isa = PBXProject; attributes = {{ BuildIndependentTargetsInParallel = 1; LastUpgradeCheck = 1600; }}; buildConfigurationList = {project_configs}; compatibilityVersion = {}; developmentRegion = en; hasScannedForEncodings = 0; knownRegions = (en, Base, ); mainGroup = {main}; productRefGroup = {product_group}; projectDirPath = {}; projectRoot = {}; packageReferences = {}; targets = {};", q("Xcode 14.0"), q(""), q(""), list(&packages), list(&targets)))?;
    let dir = format!("{}.xcodeproj", inputs.project_name);
    put(
        &mut files,
        &format!("{dir}/project.pbxproj"),
        pbx.finish(&root),
    )?;
    put(
        &mut files,
        &format!("{dir}/project.xcworkspace/contents.xcworkspacedata"),
        XCWORKSPACEDATA.into(),
    )?;
    for (name, scheme) in &apple.schemes {
        component(name)?;
        put(
            &mut files,
            &format!("{dir}/xcshareddata/xcschemes/{name}.xcscheme"),
            scheme::render(scheme, apple, &dir)?,
        )?;
    }
    stage_declared(&mut files, &apple.files, inputs.app_crate_dir.as_deref())?;
    crate::project_files::validate(&files)?;
    Ok(files)
}
fn component(s: &str) -> Result<()> {
    ProjectPath::new(s)?;
    ensure!(
        !s.contains('/'),
        "expected a single native path component: {s:?}"
    );
    Ok(())
}
fn put(files: &mut BTreeMap<ProjectPath, FileEntry>, path: &str, value: String) -> Result<()> {
    insert(files, ProjectPath::new(path)?, FileEntry::text(value))
}
fn q(s: &str) -> String {
    serde_json::to_string(s).expect("string")
}
fn key(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./".contains(c))
    {
        s.into()
    } else {
        q(s)
    }
}
fn comment(s: &str) -> String {
    s.replace("*/", "* / ").replace(['\r', '\n'], " ")
}
fn list(items: &[String]) -> String {
    format!(
        "(\n{}\n\t\t\t)",
        items
            .iter()
            .map(|s| format!("\t\t\t\t{s},"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
fn path_text(p: &AppleBuildPath) -> &str {
    match p {
        AppleBuildPath::Project(p) => p.as_str(),
        AppleBuildPath::Expression { expression } => expression,
    }
}
// A target may be needed by several conditional links and embeds. Its build
// dependency is their union; an unfiltered declaration means all platforms.
fn add_dependency(
    dependencies: &mut BTreeMap<String, Vec<String>>,
    target: &str,
    filters: &[String],
) {
    dependencies
        .entry(target.into())
        .and_modify(|current| {
            if current.is_empty() || filters.is_empty() {
                current.clear();
            } else {
                for filter in filters {
                    if !current.contains(filter) {
                        current.push(filter.clone());
                    }
                }
            }
        })
        .or_insert_with(|| filters.to_vec());
}
fn product_kind(t: &AppleTarget) -> Result<(&'static str, &'static str, &'static str)> {
    Ok(
        match t
            .product_type
            .as_deref()
            .context("missing native product type")?
        {
            "com.apple.product-type.application"
            | "com.apple.product-type.application.on-demand-install-capable"
            | "com.apple.product-type.application.watchapp2" => (".app", "wrapper.application", ""),
            "com.apple.product-type.app-extension"
            | "com.apple.product-type.watchkit2-extension"
            | "com.apple.product-type.extensionkit-extension" => {
                (".appex", "wrapper.app-extension", "")
            }
            "com.apple.product-type.framework" | "com.apple.product-type.framework.static" => {
                (".framework", "wrapper.framework", "")
            }
            "com.apple.product-type.bundle.unit-test"
            | "com.apple.product-type.bundle.ui-testing" => (".xctest", "wrapper.cfbundle", ""),
            "com.apple.product-type.bundle" => (".bundle", "wrapper.cfbundle", ""),
            "com.apple.product-type.library.static" => (".a", "archive.ar", "lib"),
            "com.apple.product-type.library.dynamic" => (".dylib", "compiled.mach-o.dylib", "lib"),
            "com.apple.product-type.tool" => ("", "compiled.mach-o.executable", ""),
            other => bail!("unsupported Xcode product type: {other}"),
        },
    )
}
fn requirement_text(r: &SwiftRequirement) -> String {
    match r {
        SwiftRequirement::Exact(v) => format!("kind = exactVersion; version = {};", q(v)),
        SwiftRequirement::UpToNextMajor(v) => {
            format!("kind = upToNextMajorVersion; minimumVersion = {};", q(v))
        }
        SwiftRequirement::UpToNextMinor(v) => {
            format!("kind = upToNextMinorVersion; minimumVersion = {};", q(v))
        }
        SwiftRequirement::Range { minimum, maximum } => format!(
            "kind = versionRange; minimumVersion = {}; maximumVersion = {};",
            q(minimum),
            q(maximum)
        ),
        SwiftRequirement::Branch(v) => format!("kind = branch; branch = {};", q(v)),
        SwiftRequirement::Revision(v) => format!("kind = revision; revision = {};", q(v)),
    }
}
#[derive(Default)]
struct Pbx {
    objects: BTreeMap<String, (String, String)>,
    file_refs: BTreeMap<String, String>,
}
impl Pbx {
    fn reference(seed: &str, label: &str) -> String {
        format!("{} /* {} */", pbxproj_uuid(seed), comment(label))
    }
    fn object(&mut self, seed: &str, label: &str, body: String) -> Result<String> {
        let id = pbxproj_uuid(seed);
        if let Some((old_seed, old_body)) = self.objects.get(&id) {
            ensure!(
                old_seed == seed && old_body == &body,
                "conflicting PBX object {seed}"
            );
        } else {
            self.objects.insert(id, (seed.into(), body));
        }
        Ok(Self::reference(seed, label))
    }
    fn finish(self, root: &str) -> String {
        let mut out = "// !$*UTF8*$!\n{\n\tarchiveVersion = 1;\n\tclasses = {};\n\tobjectVersion = 56;\n\tobjects = {\n".to_string();
        for (id, (_, body)) in self.objects {
            out += &format!("\t\t{id} = {{\n\t\t\t{body}\n\t\t}};\n");
        }
        out += &format!("\t}};\n\trootObject = {root};\n}}\n");
        out
    }
    fn file(&mut self, path: &str, tree: &str, ty: Option<&str>) -> Result<String> {
        let seed = format!("file:{tree}:{path}:{}", ty.unwrap_or(""));
        let r = self.object(
            &seed,
            path,
            format!(
                "isa = PBXFileReference; lastKnownFileType = {}; path = {}; sourceTree = {};",
                ty.unwrap_or_else(|| file_type(path)),
                q(path),
                if tree == "SOURCE_ROOT" || tree == "SDKROOT" || tree == "BUILT_PRODUCTS_DIR" {
                    tree.into()
                } else {
                    q(tree)
                }
            ),
        )?;
        self.file_refs.insert(seed, r.clone());
        Ok(r)
    }
    fn build_path(&mut self, p: &AppleBuildPath) -> Result<String> {
        match p {
            AppleBuildPath::Project(p) => self.file(p.as_str(), "SOURCE_ROOT", None),
            AppleBuildPath::Expression { expression } => {
                if let Some(path) = expression.strip_prefix("$(BUILT_PRODUCTS_DIR)/") {
                    self.file(path, "BUILT_PRODUCTS_DIR", None)
                } else {
                    self.file(expression, "<absolute>", None)
                }
            }
        }
    }
    #[allow(clippy::too_many_arguments)] // Native build-file fields, kept at one serialization seam.
    fn build_file(
        &mut self,
        seed: &str,
        label: &str,
        reference: &str,
        product: bool,
        attributes: &[&str],
        filters: &[String],
        flags: &[String],
    ) -> Result<String> {
        let mut settings = String::new();
        if !attributes.is_empty() {
            settings += &format!(
                "ATTRIBUTES = {};",
                list(&attributes.iter().map(|a| key(a)).collect::<Vec<_>>())
            );
        }
        if !flags.is_empty() {
            settings += &format!(
                "COMPILER_FLAGS = {};",
                q(&flags
                    .iter()
                    .map(|s| shell_quote(s))
                    .collect::<Vec<_>>()
                    .join(" "))
            );
        }
        self.object(
            seed,
            label,
            format!(
                "isa = PBXBuildFile; {} = {reference}; settings = {{ {settings} }}; {}",
                if product { "productRef" } else { "fileRef" },
                if filters.is_empty() {
                    String::new()
                } else {
                    format!(
                        "platformFilters = {};",
                        list(&filters.iter().map(|s| q(s)).collect::<Vec<_>>())
                    )
                }
            ),
        )
    }
    fn phase(
        &mut self,
        seed: &str,
        isa: &str,
        name: &str,
        files: &[String],
        extra: &str,
    ) -> Result<String> {
        self.object(seed,name,format!("isa = {isa};\n\t\t\tbuildActionMask = 2147483647; files = {}; name = {}; runOnlyForDeploymentPostprocessing = 0; {extra}",list(files),q(name)))
    }
    fn script(&mut self, id: &str, script: &AppleBuildScript) -> Result<String> {
        let paths =
            |v: &[AppleBuildPath]| list(&v.iter().map(|p| q(path_text(p))).collect::<Vec<_>>());
        self.phase(&format!("script:{id}:{}",script.name), "PBXShellScriptBuildPhase", &script.name, &[], &format!("shellPath = {}; shellScript = {}; inputPaths = {}; outputPaths = {}; inputFileListPaths = {}; outputFileListPaths = {}; {}", q(&script.shell),q(&script.script),paths(&script.inputs),paths(&script.outputs),paths(&script.input_file_lists),paths(&script.output_file_lists),if script.based_on_dependency_analysis == Some(false) { "alwaysOutOfDate = 1;" } else { "" }))
    }
    fn configurations(
        &mut self,
        id: &str,
        names: &[String],
        default: &str,
        common: &BTreeMap<String, AppleBuildSetting>,
        configs: &BTreeMap<String, AppleBuildConfiguration>,
    ) -> Result<String> {
        let mut refs = Vec::new();
        for name in names {
            let config = configs.get(name);
            let mut settings = common.clone();
            if let Some(c) = config {
                settings.extend(c.settings.clone());
            }
            let body = settings
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{} = {};",
                        key(k),
                        match v {
                            AppleBuildSetting::String(s) => q(s),
                            AppleBuildSetting::List(v) =>
                                list(&v.iter().map(|s| q(s)).collect::<Vec<_>>()),
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\t\t\t\t");
            let base = if let Some(path) = config.and_then(|c| c.xcconfig.as_ref()) {
                format!(
                    "baseConfigurationReference = {};",
                    self.file(path.as_str(), "SOURCE_ROOT", Some("text.xcconfig"))?
                )
            } else {
                String::new()
            };
            refs.push(self.object(&format!("config:{id}:{name}"),name,format!("isa = XCBuildConfiguration; {base} buildSettings = {{\n\t\t\t\t{body}\n\t\t\t}}; name = {};",q(name)))?);
        }
        self.object(&format!("configs:{id}"),id,format!("isa = XCConfigurationList; buildConfigurations = {}; defaultConfigurationIsVisible = 0; defaultConfigurationName = {};",list(&refs),q(default)))
    }
    fn target(
        &mut self,
        id: &str,
        t: &AppleTarget,
        apple: &AppleProjectIr,
        names: &[String],
        default: &str,
        files: &mut BTreeMap<ProjectPath, FileEntry>,
    ) -> Result<String> {
        ensure!(
            t.rust.is_none(),
            "target {id}: direct RustBuild lowering is unsupported; declare a build script and output reference"
        );
        let mut settings = t.build_settings.clone();
        if t.kind == AppleTargetKind::Native {
            settings
                .entry("PRODUCT_NAME".into())
                .or_insert_with(|| AppleBuildSetting::String(t.product_name.clone()));
        }
        for (setting, dictionary, suffix) in [
            ("INFOPLIST_FILE", &t.info_plist, "Info.plist"),
            (
                "CODE_SIGN_ENTITLEMENTS",
                &t.entitlements,
                "App.entitlements",
            ),
        ] {
            if dictionary.is_empty() {
                continue;
            }
            let path = match settings.get(setting) {
                Some(AppleBuildSetting::String(s)) => s.clone(),
                Some(_) => bail!("{setting} must be a project-relative string"),
                None => format!("Metadata/{id}/{suffix}"),
            };
            ensure!(
                !path.contains('$'),
                "generated {setting} cannot use build-setting expressions"
            );
            for config in t.configurations.values() {
                ensure!(
                    config
                        .settings
                        .get(setting)
                        .is_none_or(|v| v == &AppleBuildSetting::String(path.clone())),
                    "configuration overrides generated {setting}"
                );
            }
            put(
                files,
                &path,
                plist_xml(&PropertyListValue::Dict(dictionary.clone()))?,
            )?;
            settings.insert(setting.into(), AppleBuildSetting::String(path));
        }
        let configs = self.configurations(id, names, default, &settings, &t.configurations)?;
        let mut phases = Vec::new();
        for s in t
            .scripts
            .iter()
            .filter(|s| s.position == AppleScriptPosition::BeforeSources)
        {
            phases.push(self.script(id, s)?);
        }
        if t.kind == AppleTargetKind::Native {
            let mut headers = Vec::new();
            for (i, h) in t.headers.iter().enumerate() {
                let r = self.build_path(&h.path)?;
                let attrs: &[&str] = match h.visibility {
                    AppleHeaderVisibility::Public => &["Public"],
                    AppleHeaderVisibility::Private => &["Private"],
                    AppleHeaderVisibility::Project => &[],
                };
                headers.push(self.build_file(
                    &format!("header:{id}:{i}"),
                    path_text(&h.path),
                    &r,
                    false,
                    attrs,
                    &[],
                    &[],
                )?);
            }
            if !headers.is_empty() {
                phases.push(self.phase(
                    &format!("headers:{id}"),
                    "PBXHeadersBuildPhase",
                    "Headers",
                    &headers,
                    "",
                )?);
            }
            let mut sources = Vec::new();
            for (i, s) in t.sources.iter().enumerate() {
                let r = self.build_path(&s.path)?;
                sources.push(self.build_file(
                    &format!("source:{id}:{i}"),
                    &format!("{} in Sources", path_text(&s.path)),
                    &r,
                    false,
                    &[],
                    &s.platform_filters,
                    &s.compiler_flags,
                )?);
            }
            phases.push(self.phase(
                &format!("sources:{id}"),
                "PBXSourcesBuildPhase",
                "Sources",
                &sources,
                "",
            )?);
        }
        for s in t
            .scripts
            .iter()
            .filter(|s| s.position == AppleScriptPosition::BeforeLink)
        {
            phases.push(self.script(id, s)?);
        }
        let mut dependencies = BTreeMap::new();
        let mut linked = Vec::new();
        let mut package_products = Vec::new();
        for (i, d) in t.dependencies.iter().enumerate() {
            let (r, label, product, weak, filters) = match d {
                AppleDependency::Target {
                    target,
                    link,
                    weak,
                    platform_filters,
                } => {
                    add_dependency(&mut dependencies, target, platform_filters);
                    if !link {
                        continue;
                    }
                    (
                        Self::reference(&format!("product:{target}"), target),
                        target.clone(),
                        false,
                        *weak,
                        platform_filters,
                    )
                }
                AppleDependency::SwiftProduct {
                    package,
                    product,
                    weak,
                    platform_filters,
                } => {
                    let r = self.swift_product(package, product)?;
                    if !package_products.contains(&r) {
                        package_products.push(r.clone());
                    }
                    (r, product.clone(), true, *weak, platform_filters)
                }
                AppleDependency::SystemFramework {
                    name,
                    weak,
                    platform_filters,
                } => (
                    self.file(
                        &format!("System/Library/Frameworks/{name}"),
                        "SDKROOT",
                        Some("wrapper.framework"),
                    )?,
                    name.clone(),
                    false,
                    *weak,
                    platform_filters,
                ),
                AppleDependency::File {
                    path,
                    weak,
                    platform_filters,
                } => (
                    self.file(path.as_str(), "SOURCE_ROOT", None)?,
                    path.as_str().into(),
                    false,
                    *weak,
                    platform_filters,
                ),
                AppleDependency::BuildOutput {
                    path,
                    weak,
                    platform_filters,
                } => (
                    self.build_path(path)?,
                    path_text(path).into(),
                    false,
                    *weak,
                    platform_filters,
                ),
            };
            linked.push(self.build_file(
                &format!("link:{id}:{i}"),
                &format!("{label} in Frameworks"),
                &r,
                product,
                if weak { &["Weak"] } else { &[] },
                filters,
                &[],
            )?);
        }
        if t.kind == AppleTargetKind::Native {
            phases.push(self.phase(
                &format!("frameworks:{id}"),
                "PBXFrameworksBuildPhase",
                "Frameworks",
                &linked,
                "",
            )?);
        }
        let mut resources = Vec::new();
        let mut copies = Vec::new();
        for (i, r) in t.resources.iter().enumerate() {
            match r {
                AppleResource::Process { path } => {
                    let r = self.file(path.as_str(), "SOURCE_ROOT", None)?;
                    resources.push(self.build_file(
                        &format!("resource:{id}:{i}"),
                        &format!("{} in Resources", path.as_str()),
                        &r,
                        false,
                        &[],
                        &[],
                        &[],
                    )?);
                }
                AppleResource::Copy { resource } => {
                    if Path::new(resource.source.as_str())
                        .file_name()
                        .and_then(|s| s.to_str())
                        == Some(resource.destination.as_str())
                    {
                        let r = self.file(
                            resource.source.as_str(),
                            "SOURCE_ROOT",
                            if resource.kind == ResourceKind::Directory {
                                Some("folder")
                            } else {
                                None
                            },
                        )?;
                        resources.push(self.build_file(
                            &format!("resource:{id}:{i}"),
                            &format!("{} in Resources", resource.source.as_str()),
                            &r,
                            false,
                            &[],
                            &[],
                            &[],
                        )?);
                    } else {
                        copies.push(self.copy_resource(
                            id,
                            &format!("resource-{i}"),
                            resource,
                            true,
                        )?);
                    }
                }
            }
        }
        for (path, value) in &t.resource_plists {
            let src = format!("Metadata/{id}/Resources/{}", path.as_str());
            put(files, &src, plist_xml(value)?)?;
            copies.push(self.copy_resource(
                id,
                &format!("plist-{}", path.as_str()),
                &Resource {
                    source: ProjectPath::new(src)?,
                    destination: path.clone(),
                    kind: ResourceKind::File,
                },
                true,
            )?);
        }
        if t.kind == AppleTargetKind::Native {
            phases.push(self.phase(
                &format!("resources:{id}"),
                "PBXResourcesBuildPhase",
                "Resources",
                &resources,
                "",
            )?);
        }
        phases.extend(copies);
        for (i, resource) in t.bundle_files.iter().enumerate() {
            phases.push(self.copy_resource(id, &format!("bundle-{i}"), resource, false)?);
        }
        for (i, e) in t.embeds.iter().enumerate() {
            let (r, label, product) = match &e.source {
                AppleEmbedSource::Target { target } => {
                    add_dependency(&mut dependencies, target, &e.platform_filters);
                    (
                        Self::reference(&format!("product:{target}"), target),
                        target.clone(),
                        false,
                    )
                }
                AppleEmbedSource::File { path } => {
                    ensure!(
                        !path.as_str().ends_with(".xcframework"),
                        "embedding a staged XCFramework requires explicit slice selection"
                    );
                    (
                        self.file(path.as_str(), "SOURCE_ROOT", None)?,
                        path.as_str().into(),
                        false,
                    )
                }
                AppleEmbedSource::BuildOutput { path } => {
                    (self.build_path(path)?, path_text(path).into(), false)
                }
                AppleEmbedSource::SwiftProduct { package, product } => {
                    let r = self.swift_product(package, product)?;
                    if !package_products.contains(&r) {
                        package_products.push(r.clone());
                    }
                    (r, product.clone(), true)
                }
            };
            let mut attrs = Vec::new();
            if e.code_sign_on_copy {
                attrs.push("CodeSignOnCopy");
            }
            if e.remove_headers_on_copy {
                attrs.push("RemoveHeadersOnCopy");
            }
            let name = format!("Embed {}", e.destination.as_str());
            let bf = self.build_file(
                &format!("embed:{id}:{i}"),
                &format!("{label} in {name}"),
                &r,
                product,
                &attrs,
                &e.platform_filters,
                &[],
            )?;
            let (spec, path) = match e.destination.as_str() {
                "Frameworks" => (10, ""),
                "PlugIns" => (13, ""),
                other => (1, other),
            };
            phases.push(self.phase(
                &format!("copy:{id}:{i}"),
                "PBXCopyFilesBuildPhase",
                &name,
                &[bf],
                &format!("dstSubfolderSpec = {spec}; dstPath = {};", q(path)),
            )?);
        }
        for s in t
            .scripts
            .iter()
            .filter(|s| s.position == AppleScriptPosition::AfterResources)
        {
            phases.push(self.script(id, s)?);
        }
        let mut deps = Vec::new();
        for (target, filters) in dependencies {
            let proxy = self.object(&format!("proxy:{id}:{target}"),&target,format!("isa = PBXContainerItemProxy; containerPortal = {}; proxyType = 1; remoteGlobalIDString = {}; remoteInfo = {};",pbxproj_uuid("project"),pbxproj_uuid(&format!("target:{target}")),q(&apple.targets[&target].product_name)))?;
            deps.push(self.object(
                &format!("dependency:{id}:{target}"),
                &target,
                format!(
                    "isa = PBXTargetDependency; target = {}; targetProxy = {proxy}; {}",
                    Self::reference(&format!("target:{target}"), &target),
                    if filters.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "platformFilters = {};",
                            list(&filters.iter().map(|f| q(f)).collect::<Vec<_>>())
                        )
                    }
                ),
            )?);
        }
        let native = if t.kind == AppleTargetKind::Native {
            format!(
                "productName = {}; productReference = {}; productType = {}; packageProductDependencies = {};",
                q(&t.product_name),
                Self::reference(&format!("product:{id}"), &t.product_name),
                q(t.product_type.as_ref().unwrap()),
                list(&package_products)
            )
        } else {
            String::new()
        };
        self.object(&format!("target:{id}"),&t.product_name,format!("isa = {}; buildConfigurationList = {configs}; buildPhases = {}; buildRules = (); dependencies = {}; name = {}; {native}", if t.kind == AppleTargetKind::Native {"PBXNativeTarget"} else {"PBXAggregateTarget"},list(&phases),list(&deps),q(&t.product_name)))
    }
    fn swift_product(&mut self, package: &str, product: &str) -> Result<String> {
        self.object(
            &format!("swift:{package}:{product}"),
            product,
            format!(
                "isa = XCSwiftPackageProductDependency; package = {}; productName = {};",
                Self::reference(&format!("package:{package}"), package),
                q(product)
            ),
        )
    }
    fn copy_resource(
        &mut self,
        id: &str,
        name: &str,
        r: &Resource,
        resource: bool,
    ) -> Result<String> {
        let prefix = if resource {
            "${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}"
        } else {
            "${TARGET_BUILD_DIR}/${WRAPPER_NAME}"
        };
        let destination = format!("\"{prefix}/\"{}", shell_quote(r.destination.as_str()));
        let source = format!("\"${{SRCROOT}}/\"{}", shell_quote(r.source.as_str()));
        self.script(id,&AppleBuildScript { name: format!("Copy {name}"), position: AppleScriptPosition::AfterResources, shell: "/bin/sh".into(), script: format!("set -eu\nmkdir -p \"$(dirname {destination})\"\n/usr/bin/ditto {source} {destination}\n"), inputs: vec![AppleBuildPath::Expression { expression: format!("$(SRCROOT)/{}",r.source.as_str()) }], outputs: vec![AppleBuildPath::Expression { expression: format!("$(TARGET_BUILD_DIR)/$({})/{}",if resource {"UNLOCALIZED_RESOURCES_FOLDER_PATH"} else {"WRAPPER_NAME"},r.destination.as_str()) }], input_file_lists: vec![],output_file_lists:vec![],based_on_dependency_analysis:Some(false) })
    }
}
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn file_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|s| s.to_str()) {
        Some("framework") => "wrapper.framework",
        Some("xcframework") => "wrapper.xcframework",
        Some("a") => "archive.ar",
        Some("dylib") => "compiled.mach-o.dylib",
        Some("c") => "sourcecode.c.c",
        Some("cpp" | "cc") => "sourcecode.cpp.cpp",
        Some("metal") => "sourcecode.metal",
        Some("xcconfig") => "text.xcconfig",
        _ => last_known_file_type(Path::new(path)),
    }
}
