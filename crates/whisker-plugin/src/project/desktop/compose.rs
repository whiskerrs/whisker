use super::*;
use crate::project::merge::{
    Merge, append, equal, named, objects, optional, optional_object, public_merge, values,
};
use anyhow::Result;
public_merge!(
    WindowsProjectIr,
    WindowsExecutable,
    WindowsVersionInfo,
    MsixPackage,
    LinuxProjectIr,
    DesktopEntry,
    DbusService,
    FlatpakManifest
);
impl Merge for WindowsProjectIr {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        identity(
            &mut self.application,
            &b.application,
            &format!("{p}.application"),
        )?;
        objects(
            &mut self.executables,
            &b.executables,
            &format!("{p}.executables"),
        )?;
        named(
            &mut self.resources,
            &b.resources,
            |r| r.destination.clone(),
            &format!("{p}.resources"),
        )?;
        optional(
            &mut self.application_package,
            &b.application_package,
            &format!("{p}.application_package"),
        )?;
        objects(&mut self.packages, &b.packages, &format!("{p}.packages"))?;
        values(&mut self.files, &b.files, &format!("{p}.files"))
    }
}
impl Merge for WindowsExecutable {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        equal(&self.executable, &b.executable, &format!("{p}.executable"))?;
        optional(&mut self.manifest, &b.manifest, &format!("{p}.manifest"))?;
        optional(&mut self.icon, &b.icon, &format!("{p}.icon"))?;
        optional_object(
            &mut self.version_info,
            &b.version_info,
            &format!("{p}.version_info"),
        )?;
        append(&mut self.resource_scripts, &b.resource_scripts);
        Ok(())
    }
}
impl Merge for WindowsVersionInfo {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        equal(
            &self.file_version,
            &b.file_version,
            &format!("{p}.file_version"),
        )?;
        equal(
            &self.product_version,
            &b.product_version,
            &format!("{p}.product_version"),
        )?;
        optional(
            &mut self.flags_mask,
            &b.flags_mask,
            &format!("{p}.flags_mask"),
        )?;
        optional(&mut self.flags, &b.flags, &format!("{p}.flags"))?;
        optional(&mut self.file_os, &b.file_os, &format!("{p}.file_os"))?;
        optional(&mut self.file_type, &b.file_type, &format!("{p}.file_type"))?;
        optional(
            &mut self.file_subtype,
            &b.file_subtype,
            &format!("{p}.file_subtype"),
        )?;
        for (lang, table) in &b.strings {
            values(
                self.strings.entry(lang.clone()).or_default(),
                table,
                &format!("{p}.strings[{lang}]"),
            )?;
        }
        Ok(())
    }
}
impl Merge for MsixPackage {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        equal(
            &self.manifest,
            &b.manifest,
            &format!("{p}.manifest (use explicit XML edits)"),
        )?;
        append(&mut self.executables, &b.executables);
        named(
            &mut self.resources,
            &b.resources,
            |r| r.destination.clone(),
            &format!("{p}.resources"),
        )
    }
}
impl Merge for LinuxProjectIr {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        identity(&mut self.app_id, &b.app_id, &format!("{p}.app_id"))?;
        identity(
            &mut self.application,
            &b.application,
            &format!("{p}.application"),
        )?;
        values(
            &mut self.executables,
            &b.executables,
            &format!("{p}.executables"),
        )?;
        named(
            &mut self.resources,
            &b.resources,
            |r| r.destination.clone(),
            &format!("{p}.resources"),
        )?;
        objects(
            &mut self.desktop_entries,
            &b.desktop_entries,
            &format!("{p}.desktop_entries"),
        )?;
        values(
            &mut self.mime_packages,
            &b.mime_packages,
            &format!("{p}.mime_packages"),
        )?;
        values(&mut self.metainfo, &b.metainfo, &format!("{p}.metainfo"))?;
        objects(
            &mut self.dbus_services,
            &b.dbus_services,
            &format!("{p}.dbus_services"),
        )?;
        objects(&mut self.packages, &b.packages, &format!("{p}.packages"))?;
        values(&mut self.files, &b.files, &format!("{p}.files"))
    }
}
impl Merge for DesktopEntry {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        values(&mut self.entries, &b.entries, &format!("{p}.entries"))?;
        for (id, action) in &b.actions {
            values(
                self.actions.entry(id.clone()).or_default(),
                action,
                &format!("{p}.actions[{id}]"),
            )?;
        }
        Ok(())
    }
}
impl Merge for DbusService {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        values(&mut self.entries, &b.entries, &format!("{p}.entries"))
    }
}
impl Merge for LinuxPackage {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        match (&mut *self, b) {
            (Self::Flatpak { manifest: a }, Self::Flatpak { manifest: b }) => a.merge(b, p),
            _ => equal(self, b, p),
        }
    }
}
impl Merge for FlatpakManifest {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        equal(&self.runtime, &b.runtime, &format!("{p}.runtime"))?;
        equal(
            &self.runtime_version,
            &b.runtime_version,
            &format!("{p}.runtime_version"),
        )?;
        equal(&self.sdk, &b.sdk, &format!("{p}.sdk"))?;
        equal(&self.command, &b.command, &format!("{p}.command"))?;
        // These are native options, potentially overriding each other: require an
        // identical ordered policy when both sides specify one.
        if self.finish_args.is_empty() {
            self.finish_args = b.finish_args.clone();
        } else if !b.finish_args.is_empty() {
            equal(
                &self.finish_args,
                &b.finish_args,
                &format!("{p}.finish_args"),
            )?;
        }
        named(
            &mut self.modules,
            &b.modules,
            module_key,
            &format!("{p}.modules"),
        )?;
        objects(
            &mut self.properties,
            &b.properties,
            &format!("{p}.properties"),
        )
    }
}
pub(super) fn module_key(module: &FlatpakModule) -> String {
    match module {
        FlatpakModule::File(path) => format!("file:{}", path.as_str()),
        FlatpakModule::Inline(values) => format!(
            "inline:{}",
            values.get("name").and_then(|v| v.as_str()).unwrap_or("")
        ),
    }
}

// Empty identities are unset during application initialization; final validation
// still requires a named executable (and Linux app ID).
fn identity(a: &mut String, b: &str, path: &str) -> Result<()> {
    if a.is_empty() {
        *a = b.into();
    } else if !b.is_empty() {
        equal(a, &b.to_owned(), path)?;
    }
    Ok(())
}
