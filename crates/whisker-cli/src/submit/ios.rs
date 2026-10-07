//! `whisker submit ios` — upload the `.ipa` through App Store
//! Connect's Build Upload API: declare the build, reserve the file,
//! PUT its bytes where Apple says, commit, then wait for Apple's
//! processing verdict.
//!
//! The bundle id and version numbers sent to Apple are read out of
//! the ipa itself, so they can't drift from what is actually being
//! uploaded.

use anyhow::{Context, Result, anyhow, bail};
use clap::Args as ClapArgs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use whisker_build::ui;
use whisker_dev_server::Target;

use crate::credential::{self, asc};
use crate::manifest;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Processing normally takes a few minutes; past this the upload is
/// still valid, we just stop watching it.
const PROCESSING_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// The `.ipa` to upload. Defaults to the one the last
    /// `whisker build ipa` produced.
    #[arg(long, value_name = "IPA")]
    path: Option<PathBuf>,

    /// Return as soon as the upload is committed instead of waiting
    /// for App Store Connect to finish processing the build.
    #[arg(long)]
    no_wait: bool,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::IosSimulator)?;
    let ipa = super::artifact(
        args.path,
        whisker_build::ios::exported_ipa(&m.workspace_root, &m.package),
        "whisker build ipa",
    )?;
    let info = inspect_ipa(&ipa)?;
    if info.device_limited {
        bail!(
            "{} is signed for registered devices only (ad-hoc or development) and can't be \
             uploaded — rebuild with `whisker build ipa --method app-store-connect`",
            ipa.display()
        );
    }
    let key = credential::require_asc_key(&m.crate_dir, &info.bundle_id)?;
    let auth = asc::KeyAuth {
        p8_pem: &key.p8_pem,
        key_id: &key.key_id,
        issuer_id: &key.issuer_id,
    };

    let size = ipa.metadata()?.len();
    ui::section("Submit");
    ui::info(format!(
        "submitting {} {} ({}) — {} ({})",
        info.bundle_id,
        info.short_version,
        info.bundle_version,
        ipa.display(),
        super::megabytes(size),
    ));

    let app_id = asc::find_app_id(&auth, &info.bundle_id)?.ok_or_else(|| {
        anyhow!(
            "no app with bundle id {} in App Store Connect. The API cannot create apps —\n\
             add it at https://appstoreconnect.apple.com/apps (＋ → New App), then re-run.",
            info.bundle_id
        )
    })?;
    let upload_id =
        asc::create_build_upload(&auth, &app_id, &info.short_version, &info.bundle_version)?;
    let file_name = ipa
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("{} has no UTF-8 file name", ipa.display()))?;
    let (file_id, operations) = asc::reserve_build_upload_file(&auth, &upload_id, file_name, size)?;

    let step = ui::step(ui::OperationKind::Upload, file_name);
    if let Err(e) = send_parts(&ipa, size, &operations) {
        step.fail("");
        return Err(e);
    }
    asc::commit_build_upload_file(&auth, &file_id)?;
    step.done(super::megabytes(size));

    if args.no_wait {
        ui::info("uploaded — App Store Connect is processing the build");
        return Ok(());
    }
    wait_for_processing(&auth, &upload_id)?;
    ui::info(format!(
        "{} ({}) is processed — available in TestFlight and for App Store review",
        info.short_version, info.bundle_version
    ));
    Ok(())
}

fn send_parts(ipa: &Path, size: u64, operations: &[asc::UploadOperation]) -> Result<()> {
    let mut file = std::fs::File::open(ipa).with_context(|| format!("open {}", ipa.display()))?;
    for (index, op) in operations.iter().enumerate() {
        let offset = op.offset.unwrap_or(0);
        let length = op.length.unwrap_or(size - offset);
        file.seek(SeekFrom::Start(offset))?;
        // An explicit Content-Length keeps ureq from switching to
        // chunked transfer encoding, which the storage backend rejects.
        let mut req = ureq::request(&op.method, &op.url).set("Content-Length", &length.to_string());
        for header in &op.request_headers {
            req = req.set(&header.name, &header.value);
        }
        match req.send((&mut file).take(length)) {
            Ok(_) => {}
            Err(ureq::Error::Status(code, resp)) => bail!(
                "uploading part {}/{} failed ({code}): {}",
                index + 1,
                operations.len(),
                resp.into_string().unwrap_or_default(),
            ),
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("upload part {}/{}", index + 1, operations.len()));
            }
        }
    }
    Ok(())
}

fn wait_for_processing(auth: &asc::KeyAuth, upload_id: &str) -> Result<()> {
    let step = ui::step(ui::OperationKind::Upload, "App Store Connect processing");
    let deadline = Instant::now() + PROCESSING_TIMEOUT;
    loop {
        let status = match asc::build_upload_status(auth, upload_id) {
            Ok(status) => status,
            Err(e) => {
                step.fail("");
                return Err(e);
            }
        };
        match status.state.as_str() {
            "COMPLETE" => {
                step.done("");
                for warning in &status.warnings {
                    ui::warn(warning);
                }
                return Ok(());
            }
            "FAILED" => {
                step.fail("");
                bail!(
                    "App Store Connect rejected the build:\n  {}",
                    if status.errors.is_empty() {
                        "(no detail given)".to_string()
                    } else {
                        status.errors.join("\n  ")
                    }
                );
            }
            _ if Instant::now() >= deadline => {
                step.fail("");
                bail!(
                    "still processing after {} minutes — the upload itself succeeded; check \
                     the build's status in App Store Connect",
                    PROCESSING_TIMEOUT.as_secs() / 60
                );
            }
            _ => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

#[derive(Debug, PartialEq)]
struct IpaInfo {
    bundle_id: String,
    short_version: String,
    bundle_version: String,
    /// The embedded profile lists device UDIDs — ad-hoc or
    /// development signing, which App Store Connect refuses.
    device_limited: bool,
}

fn inspect_ipa(path: &Path) -> Result<IpaInfo> {
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
            inspect_ipa(file.path()).unwrap(),
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
        assert!(inspect_ipa(file.path()).unwrap().device_limited);
    }

    #[test]
    fn a_non_zip_file_is_a_clear_error() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        let err = inspect_ipa(file.path()).unwrap_err();
        assert!(err.to_string().contains("not a valid ipa"));
    }

    #[test]
    fn parts_are_sent_as_the_exact_byte_ranges_apple_asked_for() {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut length = None;
                let mut chunked = false;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = Some(v.trim().parse::<usize>().unwrap());
                    }
                    chunked |= lower.starts_with("transfer-encoding:");
                    if line == "\r\n" {
                        break;
                    }
                }
                assert!(!chunked, "parts must not use chunked encoding");
                let mut body = vec![0u8; length.expect("Content-Length")];
                reader.read_exact(&mut body).unwrap();
                bodies.push(body);
                reader
                    .get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .unwrap();
            }
            bodies
        });

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"0123456789").unwrap();
        let op = |offset, length| asc::UploadOperation {
            method: "PUT".into(),
            url: format!("http://{addr}/part"),
            offset: Some(offset),
            length: Some(length),
            request_headers: vec![],
        };
        send_parts(file.path(), 10, &[op(0, 6), op(6, 4)]).unwrap();
        assert_eq!(
            server.join().unwrap(),
            vec![b"012345".to_vec(), b"6789".to_vec()]
        );
    }
}
