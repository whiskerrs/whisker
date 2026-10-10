//! Reads an `.ipa`'s identity out of the archive itself, so what is
//! declared to App Store Connect can't drift from the binary.

use anyhow::{Context, Result, anyhow};
use std::io::Read;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub struct IpaInfo {
    pub bundle_id: String,
    pub short_version: String,
    pub bundle_version: String,
    /// The embedded profile lists device UDIDs — ad-hoc or
    /// development signing, which App Store Connect refuses.
    pub device_limited: bool,
}

pub fn inspect(path: &Path) -> Result<IpaInfo> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("{} is not a valid ipa (zip) file", path.display()))?;

    // `Payload/<Name>.app/<file>` — exactly three components, so
    // nested frameworks' and extensions' files don't match.
    let app_file = |zip: &zip::ZipArchive<std::fs::File>, file: &str| -> Option<String> {
        zip.file_names()
            .find(|name| {
                let parts: Vec<&str> = name.split('/').collect();
                parts.len() == 3
                    && parts[0] == "Payload"
                    && parts[1].ends_with(".app")
                    && parts[2] == file
            })
            .map(str::to_string)
    };
    let read = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str| -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        zip.by_name(name)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    };

    let plist_name = app_file(&zip, "Info.plist")
        .ok_or_else(|| anyhow!("{} has no Payload/*.app/Info.plist", path.display()))?;
    let plist = plist::Value::from_reader(std::io::Cursor::new(read(&mut zip, &plist_name)?))
        .with_context(|| format!("parse {plist_name}"))?;
    let string = |key: &str| -> Result<String> {
        plist
            .as_dictionary()
            .and_then(|d| d.get(key))
            .and_then(|v| v.as_string())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("{plist_name} has no {key}"))
    };

    let device_limited = match app_file(&zip, "embedded.mobileprovision") {
        Some(name) => {
            let profile = read(&mut zip, &name)?;
            let needle = b"ProvisionedDevices";
            profile.windows(needle.len()).any(|w| w == needle)
        }
        None => false,
    };

    Ok(IpaInfo {
        bundle_id: string("CFBundleIdentifier")?,
        short_version: string("CFBundleShortVersionString")?,
        bundle_version: string("CFBundleVersion")?,
        device_limited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ipa(profile: Option<&[u8]>) -> tempfile::NamedTempFile {
        let mut info = plist::Dictionary::new();
        info.insert("CFBundleIdentifier".into(), "com.example.app".into());
        info.insert("CFBundleShortVersionString".into(), "1.2.0".into());
        info.insert("CFBundleVersion".into(), "42".into());
        let mut info_bytes = Vec::new();
        plist::Value::Dictionary(info)
            .to_writer_binary(std::io::Cursor::new(&mut info_bytes))
            .unwrap();

        let file = tempfile::NamedTempFile::new().unwrap();
        let mut zip = zip::ZipWriter::new(file.reopen().unwrap());
        let options = zip::write::FileOptions::default();
        // A nested bundle's Info.plist must not be mistaken for the app's.
        zip.start_file("Payload/App.app/Frameworks/X.framework/Info.plist", options)
            .unwrap();
        zip.write_all(b"not the app plist").unwrap();
        zip.start_file("Payload/App.app/Info.plist", options)
            .unwrap();
        zip.write_all(&info_bytes).unwrap();
        if let Some(profile) = profile {
            zip.start_file("Payload/App.app/embedded.mobileprovision", options)
                .unwrap();
            zip.write_all(profile).unwrap();
        }
        zip.finish().unwrap();
        file
    }

    #[test]
    fn reads_identity_from_the_apps_own_info_plist() {
        let file = ipa(Some(b"<key>Name</key><string>App Store</string>"));
        assert_eq!(
            inspect(file.path()).unwrap(),
            IpaInfo {
                bundle_id: "com.example.app".into(),
                short_version: "1.2.0".into(),
                bundle_version: "42".into(),
                device_limited: false,
            }
        );
    }

    #[test]
    fn a_profile_listing_devices_marks_the_ipa_device_limited() {
        let file = ipa(Some(b"<key>ProvisionedDevices</key><array/>"));
        assert!(inspect(file.path()).unwrap().device_limited);
    }

    #[test]
    fn a_non_zip_file_is_a_clear_error() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        let err = inspect(file.path()).unwrap_err();
        assert!(err.to_string().contains("not a valid ipa"));
    }
}
