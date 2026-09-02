//! Module platform declarations and manifest resolution.
//!
//! This is the implementation behind module discovery's small interface:
//! callers receive a resolved support map and never need to infer platform
//! support from directory names.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// Top-level shape of `[package.metadata.whisker]`.
///
/// `module` is the module-system opt-in. `plugins` belongs to CNG and does not
/// make a crate a module by itself. The legacy fields remain readable only so
/// packages can migrate independently; new manifests must use `module`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManifestRaw {
    #[serde(default)]
    pub module: Option<ModuleSectionRaw>,
    // Legacy module metadata. Remove after all published packages have moved
    // to `module.platforms`.
    #[serde(default)]
    pub ios: Option<IosSectionRaw>,
    #[serde(default)]
    pub android: Option<AndroidSectionRaw>,
    #[serde(default)]
    pub desktop: Option<RustHostSectionRaw>,
    #[serde(default)]
    pub web: Option<RustHostSectionRaw>,
    /// Parsed separately by CNG plugin discovery.
    #[serde(default, rename = "plugins")]
    pub _plugins: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModuleSectionRaw {
    pub platforms: PlatformDeclarationsRaw,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlatformDeclarationsRaw {
    pub android: Option<PlatformDeclarationRaw>,
    pub ios: Option<PlatformDeclarationRaw>,
    pub web: Option<PlatformDeclarationRaw>,
    pub desktop: Option<PlatformDeclarationRaw>,
    pub macos: Option<PlatformDeclarationRaw>,
    pub windows: Option<PlatformDeclarationRaw>,
    pub linux: Option<PlatformDeclarationRaw>,
}

/// One platform declaration. Exactly one of `kind` and `manifest` is required.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlatformDeclarationRaw {
    pub kind: Option<String>,
    pub manifest: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RustHostSectionRaw {
    pub package: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IosSectionRaw {
    #[serde(default)]
    pub swift_sources: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AndroidSectionRaw {
    #[serde(default)]
    pub kotlin_sources: Vec<String>,
    #[serde(default)]
    pub jni_sources: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ResolvedModulePlatforms {
    pub android: Option<ResolvedPlatformImplementation>,
    pub ios: Option<ResolvedPlatformImplementation>,
    pub web: Option<ResolvedPlatformImplementation>,
    pub desktop: Option<ResolvedPlatformImplementation>,
    pub macos: Option<ResolvedPlatformImplementation>,
    pub windows: Option<ResolvedPlatformImplementation>,
    pub linux: Option<ResolvedPlatformImplementation>,
}

/// The implementation behind one declared platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ResolvedPlatformImplementation {
    /// The parent Rust crate is the complete implementation.
    Common,
    /// A native build-system manifest (Gradle or SwiftPM).
    NativeManifest(ResolvedNativeManifest),
    /// A Rust Host adapter linked by Cargo.
    RustHost(ResolvedRustModuleContribution),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedNativeManifest {
    pub path: PathBuf,
    pub kind: NativeManifestKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NativeManifestKind {
    Gradle,
    SwiftPm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModulePlatform {
    Android,
    Ios,
    Web,
    Desktop,
    Macos,
    Windows,
    Linux,
}

impl ModulePlatform {
    fn name(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Web => "web",
            Self::Desktop => "desktop",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

/// One existence-checked Rust Host library shipped beside a module package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedRustModuleContribution {
    pub package: String,
    pub source: ResolvedRustHostSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ResolvedRustHostSource {
    Path(PathBuf),
    Registry { version: String },
}

pub(super) fn resolve_platforms(
    package: &str,
    package_version: &str,
    registry_source: bool,
    manifest_dir: &Path,
    raw: &PlatformDeclarationsRaw,
) -> Result<ResolvedModulePlatforms> {
    if raw.android.is_none()
        && raw.ios.is_none()
        && raw.web.is_none()
        && raw.desktop.is_none()
        && raw.macos.is_none()
        && raw.windows.is_none()
        && raw.linux.is_none()
    {
        return Err(anyhow!(
            "module `{package}` must declare at least one platform under metadata.whisker.module.platforms"
        ));
    }

    Ok(ResolvedModulePlatforms {
        android: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Android,
            raw.android.as_ref(),
        )?,
        ios: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Ios,
            raw.ios.as_ref(),
        )?,
        web: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Web,
            raw.web.as_ref(),
        )?,
        desktop: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Desktop,
            raw.desktop.as_ref(),
        )?,
        macos: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Macos,
            raw.macos.as_ref(),
        )?,
        windows: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Windows,
            raw.windows.as_ref(),
        )?,
        linux: resolve_platform(
            package,
            package_version,
            registry_source,
            manifest_dir,
            ModulePlatform::Linux,
            raw.linux.as_ref(),
        )?,
    })
}

fn resolve_platform(
    package: &str,
    package_version: &str,
    registry_source: bool,
    manifest_dir: &Path,
    platform: ModulePlatform,
    raw: Option<&PlatformDeclarationRaw>,
) -> Result<Option<ResolvedPlatformImplementation>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match (raw.kind.as_deref(), raw.manifest.as_deref()) {
        (Some("common"), None) => Ok(Some(ResolvedPlatformImplementation::Common)),
        (Some(kind), None) => Err(anyhow!(
            "module `{package}` metadata.whisker.module.platforms.{}.kind must be `common`, got {kind:?}",
            platform.name(),
        )),
        (None, Some(path)) => resolve_platform_manifest(
            package,
            package_version,
            registry_source,
            manifest_dir,
            platform,
            path,
        )
        .map(Some),
        (Some(_), Some(_)) => Err(anyhow!(
            "module `{package}` metadata.whisker.module.platforms.{} must declare exactly one of `kind` and `manifest`",
            platform.name(),
        )),
        (None, None) => Err(anyhow!(
            "module `{package}` metadata.whisker.module.platforms.{} must declare `kind = \"common\"` or `manifest = \"...\"`",
            platform.name(),
        )),
    }
}

fn resolve_platform_manifest(
    package: &str,
    package_version: &str,
    registry_source: bool,
    manifest_dir: &Path,
    platform: ModulePlatform,
    raw_path: &str,
) -> Result<ResolvedPlatformImplementation> {
    if raw_path.trim().is_empty() {
        return Err(anyhow!(
            "module `{package}` metadata.whisker.module.platforms.{}.manifest must not be empty",
            platform.name(),
        ));
    }
    let relative = Path::new(raw_path);
    if relative.is_absolute() {
        return Err(anyhow!(
            "module `{package}` metadata.whisker.module.platforms.{}.manifest must be relative to Cargo.toml",
            platform.name(),
        ));
    }
    let path = manifest_dir.join(relative);
    match platform {
        ModulePlatform::Android => resolve_native_manifest(
            package,
            platform,
            &path,
            "build.gradle.kts",
            NativeManifestKind::Gradle,
        ),
        ModulePlatform::Ios => resolve_native_manifest(
            package,
            platform,
            &path,
            "Package.swift",
            NativeManifestKind::SwiftPm,
        ),
        ModulePlatform::Web
        | ModulePlatform::Desktop
        | ModulePlatform::Macos
        | ModulePlatform::Windows
        | ModulePlatform::Linux => {
            resolve_rust_manifest(package, package_version, registry_source, platform, &path)
        }
    }
}

fn resolve_native_manifest(
    package: &str,
    platform: ModulePlatform,
    path: &Path,
    expected_file_name: &str,
    kind: NativeManifestKind,
) -> Result<ResolvedPlatformImplementation> {
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_file_name) {
        return Err(anyhow!(
            "module `{package}` {} manifest must point to `{expected_file_name}`, got {}",
            platform.name(),
            path.display(),
        ));
    }
    let path = path.canonicalize().with_context(|| {
        format!(
            "module `{package}` declares {} manifest {}, but it does not exist",
            platform.name(),
            path.display(),
        )
    })?;
    Ok(ResolvedPlatformImplementation::NativeManifest(
        ResolvedNativeManifest { path, kind },
    ))
}

fn resolve_rust_manifest(
    package: &str,
    package_version: &str,
    registry_source: bool,
    platform: ModulePlatform,
    cargo_toml: &Path,
) -> Result<ResolvedPlatformImplementation> {
    if cargo_toml.file_name().and_then(|name| name.to_str()) != Some("Cargo.toml") {
        return Err(anyhow!(
            "module `{package}` {} manifest must point to a Cargo.toml, got {}",
            platform.name(),
            cargo_toml.display(),
        ));
    }
    let expected_package = format!("{package}-{}", platform.name());
    if !cargo_toml.is_file() {
        if registry_source {
            return Ok(ResolvedPlatformImplementation::RustHost(
                ResolvedRustModuleContribution {
                    package: expected_package,
                    source: ResolvedRustHostSource::Registry {
                        version: package_version.to_string(),
                    },
                },
            ));
        }
        return Err(anyhow!(
            "module `{package}` declares {} manifest {}, but it does not exist",
            platform.name(),
            cargo_toml.display(),
        ));
    }
    let source = std::fs::read_to_string(cargo_toml).with_context(|| {
        format!(
            "read {} Host manifest {}",
            platform.name(),
            cargo_toml.display()
        )
    })?;
    let manifest: toml::Value = toml::from_str(&source).with_context(|| {
        format!(
            "parse {} Host manifest {}",
            platform.name(),
            cargo_toml.display()
        )
    })?;
    let actual_package = manifest
        .get("package")
        .and_then(|value| value.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "{} must declare a non-empty [package].name",
                cargo_toml.display()
            )
        })?;
    if actual_package != expected_package {
        return Err(anyhow!(
            "module `{package}` {} Host package must be named `{expected_package}`, but {} declares `{actual_package}`",
            platform.name(),
            cargo_toml.display(),
        ));
    }
    let root = cargo_toml
        .parent()
        .expect("Cargo.toml has a parent")
        .canonicalize()
        .with_context(|| format!("canonicalize {} Host crate", cargo_toml.display()))?;
    Ok(ResolvedPlatformImplementation::RustHost(
        ResolvedRustModuleContribution {
            package: actual_package.to_string(),
            source: ResolvedRustHostSource::Path(root),
        },
    ))
}

pub(super) fn resolve_legacy_platforms(
    package: &str,
    package_version: &str,
    manifest_dir: &Path,
    manifest: &ManifestRaw,
) -> Result<ResolvedModulePlatforms> {
    let android_path = manifest_dir.join("build.gradle.kts");
    let ios_path = manifest_dir.join("Package.swift");
    Ok(ResolvedModulePlatforms {
        android: android_path
            .is_file()
            .then_some(ResolvedPlatformImplementation::NativeManifest(
                ResolvedNativeManifest {
                    path: android_path,
                    kind: NativeManifestKind::Gradle,
                },
            )),
        ios: ios_path
            .is_file()
            .then_some(ResolvedPlatformImplementation::NativeManifest(
                ResolvedNativeManifest {
                    path: ios_path,
                    kind: NativeManifestKind::SwiftPm,
                },
            )),
        desktop: resolve_legacy_rust_host(
            package,
            package_version,
            "desktop",
            manifest_dir,
            manifest.desktop.as_ref(),
        )?
        .map(ResolvedPlatformImplementation::RustHost),
        web: resolve_legacy_rust_host(
            package,
            package_version,
            "web",
            manifest_dir,
            manifest.web.as_ref(),
        )?
        .map(ResolvedPlatformImplementation::RustHost),
        macos: None,
        windows: None,
        linux: None,
    })
}

fn resolve_legacy_rust_host(
    package: &str,
    package_version: &str,
    target: &str,
    manifest_dir: &Path,
    published: Option<&RustHostSectionRaw>,
) -> Result<Option<ResolvedRustModuleContribution>> {
    if let Some(published) = published
        && published.package.trim().is_empty()
    {
        return Err(anyhow!(
            "module `{package}` metadata.whisker.{target}.package must not be empty"
        ));
    }
    let cargo_toml = manifest_dir.join(target).join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(published.map(|published| ResolvedRustModuleContribution {
            package: published.package.clone(),
            source: ResolvedRustHostSource::Registry {
                version: package_version.to_string(),
            },
        }));
    }
    let source = std::fs::read_to_string(&cargo_toml)
        .with_context(|| format!("read {target} Host manifest {}", cargo_toml.display()))?;
    let manifest: toml::Value = toml::from_str(&source)
        .with_context(|| format!("parse {target} Host manifest {}", cargo_toml.display()))?;
    let host_package = manifest
        .get("package")
        .and_then(|value| value.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "module `{package}` {target}/Cargo.toml must declare a non-empty [package].name"
            )
        })?;
    if let Some(published) = published
        && published.package != host_package
    {
        return Err(anyhow!(
            "module `{package}` metadata.whisker.{target}.package is {:?}, but {target}/Cargo.toml declares package {:?}",
            published.package,
            host_package,
        ));
    }
    let host_root = cargo_toml
        .parent()
        .expect("Cargo.toml has a parent")
        .canonicalize()
        .with_context(|| format!("canonicalize {target} Host crate {}", cargo_toml.display()))?;
    Ok(Some(ResolvedRustModuleContribution {
        package: host_package.to_string(),
        source: ResolvedRustHostSource::Path(host_root),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-module-platforms-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn resolves_explicit_platform_contract_without_directory_guessing() {
        let root = tempdir();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(
            root.join("web/Cargo.toml"),
            "[package]\nname = \"whisker-map-web\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let raw: ManifestRaw = serde_json::from_value(serde_json::json!({
            "module": { "platforms": {
                "ios": { "kind": "common" },
                "web": { "manifest": "web/Cargo.toml" }
            }}
        }))
        .unwrap();
        let platforms = resolve_platforms(
            "whisker-map",
            "1.2.3",
            false,
            &root,
            &raw.module.unwrap().platforms,
        )
        .unwrap();
        assert_eq!(platforms.ios, Some(ResolvedPlatformImplementation::Common));
        let web = match platforms.web.unwrap() {
            ResolvedPlatformImplementation::RustHost(host) => host,
            other => panic!("expected Rust Host, got {other:?}"),
        };
        assert_eq!(web.package, "whisker-map-web");
        assert!(matches!(web.source, ResolvedRustHostSource::Path(_)));
        assert!(platforms.android.is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_ambiguous_platform_declarations() {
        let raw = PlatformDeclarationRaw {
            kind: Some("common".into()),
            manifest: Some("web/Cargo.toml".into()),
        };
        let error = resolve_platform(
            "whisker-map",
            "1.2.3",
            false,
            Path::new("/module"),
            ModulePlatform::Web,
            Some(&raw),
        )
        .unwrap_err();
        assert!(error.to_string().contains("exactly one"));
    }

    #[test]
    fn registry_rust_hosts_use_the_conventional_package_name() {
        let resolved = resolve_platform_manifest(
            "whisker-map",
            "1.2.3",
            true,
            Path::new("/cargo/registry/whisker-map-1.2.3"),
            ModulePlatform::Web,
            "web/Cargo.toml",
        )
        .unwrap();
        assert_eq!(
            resolved,
            ResolvedPlatformImplementation::RustHost(ResolvedRustModuleContribution {
                package: "whisker-map-web".into(),
                source: ResolvedRustHostSource::Registry {
                    version: "1.2.3".into()
                },
            })
        );
    }

    #[test]
    fn published_legacy_host_falls_back_to_the_outer_package_version() {
        let contribution = resolve_legacy_rust_host(
            "whisker-example",
            "1.2.3",
            "desktop",
            Path::new("/definitely/not/a/module/package"),
            Some(&RustHostSectionRaw {
                package: "whisker-example-desktop-host".into(),
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(contribution.package, "whisker-example-desktop-host");
        assert_eq!(
            contribution.source,
            ResolvedRustHostSource::Registry {
                version: "1.2.3".into()
            }
        );
    }
}
