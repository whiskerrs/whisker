//! Native metadata serialization. Exec values already contain native quoting;
//! only the Desktop Entry string layer is escaped here (never a shell command).
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use whisker_plugin::project::*;

fn key(key: &str) -> Result<()> {
    ensure!(
        !key.is_empty()
            && !key
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '=' | '#'))
            && !key.starts_with('['),
        "invalid desktop metadata key {key:?}"
    );
    Ok(())
}
fn string(value: &str, list: bool) -> Result<String> {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' ' => out.push_str("\\s"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ';' if list => out.push_str("\\;"),
            c => {
                ensure!(!c.is_control(), "unsupported desktop metadata character");
                out.push(c);
            }
        }
    }
    Ok(out)
}
fn values(values: &BTreeMap<String, DesktopEntryValue>) -> Result<String> {
    let mut out = String::new();
    for (k, v) in values {
        key(k)?;
        let v = match v {
            DesktopEntryValue::String(s) => string(s, false)?,
            DesktopEntryValue::List(items) => items
                .iter()
                .map(|s| Ok(format!("{};", string(s, true)?)))
                .collect::<Result<String>>()?,
            DesktopEntryValue::Boolean(v) => v.to_string(),
            DesktopEntryValue::Number(v) => {
                ensure!(v.is_finite(), "invalid desktop number");
                v.to_string()
            }
        };
        out += &format!("{k}={v}\n");
    }
    Ok(out)
}
pub(super) fn desktop(entry: &DesktopEntry) -> Result<String> {
    let mut out = format!("[Desktop Entry]\n{}", values(&entry.entries)?);
    for (id, action) in &entry.actions {
        ensure!(
            id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "invalid desktop action ID"
        );
        out += &format!("\n[Desktop Action {id}]\n{}", values(action)?);
    }
    Ok(out)
}
pub(super) fn dbus(service: &DbusService) -> Result<String> {
    let mut out = String::from("[D-BUS Service]\n");
    for (k, v) in &service.entries {
        key(k)?;
        // D-Bus uses its own key file grammar. Do not reuse Desktop Entry escaping.
        ensure!(
            !v.chars().any(char::is_control) && v.trim() == v,
            "unsupported D-Bus service value"
        );
        out += &format!("{k}={v}\n");
    }
    Ok(out)
}
fn rc_string(value: &str) -> Result<String> {
    ensure!(
        !value.chars().any(char::is_control),
        "control character in Windows resource string"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}
// Escape each UTF-16 code unit: resource compilers must not reinterpret a
// Japanese/emoji value through the machine's ANSI code page.
fn rc_wide_string(value: &str) -> Result<String> {
    ensure!(
        !value.chars().any(char::is_control),
        "control character in Windows version string"
    );
    use std::fmt::Write;
    let mut out = String::from("L\"");
    for unit in value.encode_utf16() {
        write!(out, "\\x{unit:04X}")?;
    }
    out.push('"');
    Ok(out)
}
pub(super) fn rc(exe: &WindowsExecutable, manifest_path: Option<&str>) -> Result<String> {
    let mut rc = String::from("#pragma code_page(65001)\n");
    if let Some(path) = manifest_path {
        rc += &format!("1 24 {}\n", rc_string(path)?);
    }
    if let Some(icon) = &exe.icon {
        rc += &format!("1 ICON {}\n", rc_string(icon.as_str())?);
    }
    if let Some(v) = &exe.version_info {
        let numbers = |v: &[u16; 4]| v.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
        rc += &format!(
            "1 VERSIONINFO\nFILEVERSION {}\nPRODUCTVERSION {}\n",
            numbers(&v.file_version),
            numbers(&v.product_version)
        );
        for (key, value) in [
            ("FILEFLAGSMASK", v.flags_mask),
            ("FILEFLAGS", v.flags),
            ("FILEOS", v.file_os),
            ("FILETYPE", v.file_type),
            ("FILESUBTYPE", v.file_subtype),
        ] {
            if let Some(value) = value {
                rc += &format!("{key} {value}\n");
            }
        }
        rc += "BEGIN\nBLOCK \"StringFileInfo\"\nBEGIN\n";
        for (lang, table) in &v.strings {
            rc += &format!("BLOCK {}\nBEGIN\n", rc_string(lang)?);
            for (key, value) in table {
                rc += &format!(
                    "VALUE {}, {}\n",
                    rc_wide_string(key)?,
                    rc_wide_string(value)?
                );
            }
            rc += "END\n";
        }
        rc += "END\n";
        if !v.strings.is_empty() {
            rc += "BLOCK \"VarFileInfo\"\nBEGIN\nVALUE \"Translation\", ";
            rc += &v
                .strings
                .keys()
                .map(|s| format!("0x{}, 0x{}", &s[..4], &s[4..]))
                .collect::<Vec<_>>()
                .join(", ");
            rc += "\nEND\n";
        }
        rc += "END\n";
    }
    for path in &exe.resource_scripts {
        rc += &format!("#include {}\n", rc_string(path.as_str())?);
    }
    Ok(rc)
}
