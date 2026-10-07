//! Desktop artifacts, OS metadata, and optional distribution settings

use super::{ProjectFiles, ProjectPath, Resource, RustBuild, XmlElement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod compose;
#[cfg(test)]
mod tests;
mod validate;
pub(super) use validate::{validate_linux, validate_windows};

/// A Windows desktop application and independently declared MSIX packages
///
/// Win32 manifests and PE resources belong to individual executables. MSIX
/// metadata belongs to a separate, optional package. An unpackaged application
/// therefore does not acquire a package identity merely by using this model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsProjectIr {
    /// ID of the main executable.
    pub application: String,
    /// Executables keyed by stable ID, including any helpers.
    #[serde(default)]
    pub executables: BTreeMap<String, WindowsExecutable>,
    /// Files copied relative to the distribution root.
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// Optional application package ID. Only this package must contain application.
    pub application_package: Option<String>,
    /// Independent MSIX packages, including optional/resource/framework packages.
    /// Each package explicitly owns its files; distribution resources are not implicit.
    #[serde(default)]
    pub packages: BTreeMap<String, MsixPackage>,
    /// Files staged at the generated project root.
    #[serde(default)]
    pub files: ProjectFiles,
}

/// A Windows executable and the metadata embedded in its PE image
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsExecutable {
    /// Artifact build/source and destination in the distribution.
    pub executable: DesktopExecutable,
    /// Win32 application manifest, separate from the MSIX package manifest.
    pub manifest: Option<XmlElement>,
    /// Windows `.ico` file staged in the generated project.
    pub icon: Option<ProjectPath>,
    /// VERSIONINFO resource, when requested.
    pub version_info: Option<WindowsVersionInfo>,
    /// Additional `.rc` scripts compiled into this executable.
    #[serde(default)]
    pub resource_scripts: Vec<ProjectPath>,
}

/// Version numbers and localized VERSIONINFO strings for one PE image
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsVersionInfo {
    /// Four numeric components of FILEVERSION.
    pub file_version: [u16; 4],
    /// Four numeric components of PRODUCTVERSION.
    pub product_version: [u16; 4],
    /// Optional FILEFLAGSMASK, using native numeric bits.
    pub flags_mask: Option<u32>,
    /// Optional FILEFLAGS, using native numeric bits.
    pub flags: Option<u32>,
    /// Optional FILEOS value.
    pub file_os: Option<u32>,
    /// Optional FILETYPE value.
    pub file_type: Option<u32>,
    /// Optional FILESUBTYPE value.
    pub file_subtype: Option<u32>,
    /// String tables keyed by language/code-page identifier, e.g. `040904b0`.
    /// Inner keys include ProductName, CompanyName, and FileDescription.
    /// The renderer derives VarFileInfo Translation from these 8-hex-digit keys.
    #[serde(default)]
    pub strings: BTreeMap<String, BTreeMap<String, String>>,
}

/// MSIX package metadata, independent of any executable's Win32 manifest
///
/// The complete XML retains package identity, applications, capabilities, and
/// extensions. Native package tooling validates the manifest's schema and paths.
/// AppxManifest.xml is reserved in this package. Resources are not inherited from
/// WindowsProjectIr.resources, so auxiliary packages can own disjoint file sets.
/// Certificates and signing keys are supplied by the distribution workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsixPackage {
    /// Complete AppxManifest.xml document root.
    pub manifest: XmlElement,
    /// Executable IDs included in this package.
    pub executables: Vec<String>,
    /// Additional files relative to the MSIX package root.
    #[serde(default)]
    pub resources: Vec<Resource>,
}

/// A Linux desktop application with distribution-independent metadata
///
/// Paths for desktop entries, icons, MIME definitions, and service definitions
/// are relative to an installation prefix, normally under `share/` or `bin/`.
/// Package backends decide how to install these declarations alongside native modules.
/// There is no universal Linux sandbox permission list: permissions belong to
/// the selected distribution format, such as Flatpak.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxProjectIr {
    /// Stable desktop application identifier.
    pub app_id: String,
    /// ID of the main executable.
    pub application: String,
    /// Executables keyed by stable ID.
    #[serde(default)]
    pub executables: BTreeMap<String, DesktopExecutable>,
    /// Data files and icons, relative to the installation prefix.
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// Desktop files keyed by installation path.
    #[serde(default)]
    pub desktop_entries: BTreeMap<ProjectPath, DesktopEntry>,
    /// Shared MIME-info XML documents keyed by installation path.
    #[serde(default)]
    pub mime_packages: BTreeMap<ProjectPath, XmlElement>,
    /// AppStream XML documents keyed by installation path.
    #[serde(default)]
    pub metainfo: BTreeMap<ProjectPath, XmlElement>,
    /// D-Bus service files keyed by installation path.
    #[serde(default)]
    pub dbus_services: BTreeMap<ProjectPath, DbusService>,
    /// Optional packaging declarations, keyed by caller-chosen recipe ID.
    #[serde(default)]
    pub packages: BTreeMap<String, LinuxPackage>,
    /// Files staged at the generated project root.
    #[serde(default)]
    pub files: ProjectFiles,
}

/// One executable installed into a desktop distribution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopExecutable {
    /// How the executable is obtained.
    pub source: ExecutableSource,
    /// Relative path in the distribution or Linux installation prefix.
    pub destination: ProjectPath,
}

/// Executable built with Cargo or staged by a plugin
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExecutableSource {
    /// Cargo binary declaration; its kind must be Bin.
    Cargo(RustBuild),
    /// Prebuilt executable staged in the generated project.
    Prebuilt(ProjectPath),
}

/// A desktop entry with native keys and separately identified actions
///
/// Main keys include Type, Name, Exec, Icon, Categories, and MimeType. Localized
/// keys such as `Name[ja]` remain explicit. Values are logical strings (not
/// desktop-file escaped); Exec retains the native command/field-code syntax.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopEntry {
    /// Keys in the `[Desktop Entry]` group.
    #[serde(default)]
    pub entries: BTreeMap<String, DesktopEntryValue>,
    /// `[Desktop Action <id>]` groups. The main Actions key controls their order.
    #[serde(default)]
    pub actions: BTreeMap<String, BTreeMap<String, DesktopEntryValue>>,
}

/// A D-Bus activation service definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbusService {
    /// Keys in `[D-BUS Service]`, including Name and Exec.
    pub entries: BTreeMap<String, String>,
}

/// Optional Linux packaging separate from desktop integration
///
/// Flatpak has a structured declaration because its runtime and sandbox settings
/// affect app capabilities. Other formats can reference a staged recipe without
/// claiming that opaque packaging scripts participate in structural merging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinuxPackage {
    /// A Flatpak build manifest.
    Flatpak {
        /// Runtime, SDK, sandbox options, and build modules.
        manifest: FlatpakManifest,
    },
    /// A recipe interpreted by an external packaging backend.
    Recipe {
        /// Format identifier, for example deb, rpm, or appimage.
        format: String,
        /// Recipe file staged in the generated project.
        path: ProjectPath,
    },
}

/// Flatpak-specific build and sandbox configuration
///
/// The app ID comes from LinuxProjectIr.app_id. Build modules use native Flatpak
/// JSON objects so their source/build-system grammars are not duplicated here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatpakManifest {
    /// Flatpak runtime identifier.
    pub runtime: String,
    /// Runtime branch/version.
    pub runtime_version: String,
    /// SDK identifier used during builds.
    pub sdk: String,
    /// Declared executable or a wrapper installed by the native build modules.
    pub command: FlatpakCommand,
    /// Sandbox permission and environment options.
    #[serde(default)]
    pub finish_args: Vec<String>,
    /// Native modules or staged manifest fragments in build order.
    #[serde(default)]
    pub modules: Vec<FlatpakModule>,
    /// Additional native root keys, e.g. build-options, sdk-extensions, cleanup.
    /// Typed keys are reserved and cannot be repeated here.
    #[serde(default)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// Desktop entry values with unambiguous scalar/list escaping
///
/// Strings and list elements are logical unescaped values; literal semicolons
/// inside list elements are distinct from delimiters. Exec strings retain native
/// field-code and quoting syntax, not shell syntax. Lists preserve order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DesktopEntryValue {
    /// Logical string, including native Exec syntax.
    String(String),
    /// Logical list elements; the renderer emits escaped semicolon syntax.
    List(Vec<String>),
    /// Native true/false value.
    Boolean(bool),
    /// Native numeric value.
    Number(f64),
}

/// Flatpak launch command without assuming every product is built by Cargo
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlatpakCommand {
    /// Use the installed path of a declared executable.
    Executable {
        /// ID in LinuxProjectIr.executables.
        id: String,
    },
    /// A command/wrapper installed by the native Flatpak modules.
    Native {
        /// Native relative command name/path, without shell arguments.
        command: ProjectPath,
    },
}

/// Flatpak module declaration or manifest-fragment reference
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FlatpakModule {
    /// Staged JSON/YAML fragment; the renderer resolves relative to generated root.
    File(ProjectPath),
    /// Native module object, identified for composition by its name property.
    Inline(BTreeMap<String, serde_json::Value>),
}

impl DesktopEntry {
    /// Append logical list values without changing existing order or duplicating entries
    ///
    /// The key is created when absent. A scalar under the same key is a conflict.
    /// This is an explicit additive operation; merge_from requires entire list
    /// equality because arbitrary native lists can have order-sensitive meaning.
    pub fn append_list(&mut self, key: impl Into<String>, values: &[String]) -> anyhow::Result<()> {
        let key = key.into();
        if let Some(value) = self.entries.get(&key) {
            anyhow::ensure!(
                matches!(value, DesktopEntryValue::List(_)),
                "desktop entry {key} is not a list"
            );
        }
        let DesktopEntryValue::List(list) = self
            .entries
            .entry(key)
            .or_insert_with(|| DesktopEntryValue::List(Vec::new()))
        else {
            unreachable!()
        };
        crate::project::merge::append(list, values);
        Ok(())
    }
}
