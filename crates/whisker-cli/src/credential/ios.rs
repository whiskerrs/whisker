//! `whisker credential ios` — acquire and store the App Store
//! Connect **Team** API key that powers the whole iOS pipeline
//! (automatic signing, cloud-managed distribution certificates,
//! bundle-id auto-registration, and later build upload).
//!
//! The ceremony this wizard compresses: create one key in the ASC
//! web UI, then hand whisker three values. Two of the three are
//! auto-captured (the `.p8` file and its Key ID, detected from the
//! `AuthKey_<KEYID>.p8` download); only the Issuer ID is a paste.
//! The key is validated with a real API call — and the team id
//! resolved — *before* anything is stored, so a wrong key fails
//! here with a translated message, never later inside xcodebuild.

use anyhow::{Context, Result, bail};
use clap::Args;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use whisker_credentials::{AscKey, ios_asc_rel};

use super::{asc, prompt};
use crate::manifest;

/// ASC "Integrations" page — where Team Keys are created.
const KEYS_URL: &str = "https://appstoreconnect.apple.com/access/integrations/api";

/// How long to watch ~/Downloads before falling back to a manual
/// path prompt. Creating + downloading a key takes well under a
/// minute when things go smoothly.
const DOWNLOAD_WAIT: Duration = Duration::from_secs(180);

#[derive(Args, Debug)]
pub struct IosArgs {
    /// Store the key under a specific bundle id instead of the
    /// team-wide `default` entry. Only needed when some bundle ids
    /// ship under a DIFFERENT Apple Developer team.
    #[arg(long, value_name = "BUNDLE_ID")]
    id: Option<String>,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: IosArgs) -> Result<()> {
    if !prompt::is_interactive() {
        bail!(
            "`whisker credential ios` is an interactive wizard — run it locally, commit\n\
             the credentials/ directory, and give CI only $WHISKER_CREDENTIALS_KEY"
        );
    }
    let m = manifest::resolve(args.manifest_path.as_deref())?;
    let store = super::open_or_bootstrap(&m.crate_dir)?;

    let rel = ios_asc_rel(args.id.as_deref());
    if store.has(&rel)
        && !prompt::confirm("An App Store Connect key is already stored. Replace it?")?
    {
        bail!("aborted — existing key kept.");
    }
    acquire_and_store(&store, &rel)
}

/// The wizard body: guide key creation, capture the .p8, validate
/// against the API, store encrypted. Also the inline pre-step
/// `whisker build ipa` runs when no key is stored yet (interactive
/// sessions only).
pub(crate) fn acquire_and_store(store: &whisker_credentials::Store, rel: &str) -> Result<()> {
    println!("① Create an App Store Connect API key (needs the Account Holder or an Admin):");
    println!("     {KEYS_URL}");
    println!("   - Open the **Team Keys** tab (Individual Keys lack an Issuer ID and won't work)");
    println!("   - Name: anything (e.g. `whisker`) / Access: **Admin**");
    println!("     (Admin is required for cloud-managed distribution certificates)");
    println!("   - Generate, then **Download API Key**");
    prompt::line("Press Enter to open the page in your browser…")?;
    open_in_browser(KEYS_URL);

    let p8_path = wait_for_p8()?;
    let key_id = key_id_from(&p8_path)
        .map(Ok)
        .unwrap_or_else(|| prompt::line("Key ID (shown next to the key on the page):"))?;
    let p8_pem =
        std::fs::read_to_string(&p8_path).with_context(|| format!("read {}", p8_path.display()))?;

    println!("③ The Issuer ID is at the top of the same page (a UUID).");
    let issuer_id = loop {
        let v = prompt::line("Issuer ID:")?;
        if looks_like_uuid(&v) {
            break v;
        }
        println!("   That doesn't look like a UUID (8-4-4-4-12 hex) — check the page header.");
    };

    println!("   Validating against the App Store Connect API…");
    let auth = asc::KeyAuth {
        p8_pem: &p8_pem,
        key_id: &key_id,
        issuer_id: &issuer_id,
    };
    asc::validate(&auth)?;
    let team_id = match asc::resolve_team_id(&auth)? {
        Some(id) => {
            println!("   ✓ key works — team {id}");
            id
        }
        // Brand-new team with zero registered bundle ids: nothing to
        // read the seed id from. One manual paste, with directions.
        None => prompt::line("Team ID (developer.apple.com → Membership details, 10 characters):")?,
    };

    let entry = AscKey {
        p8_pem,
        key_id,
        issuer_id,
        team_id,
    };
    store.put(
        rel,
        &serde_json::to_vec(&entry).context("serialize AscKey")?,
    )?;

    println!("  ✓ credentials/{rel}.age");
    println!();
    println!("Commit the credentials/ directory. `whisker build ipa` will use this key for");
    println!("signing, provisioning, and bundle-id registration automatically.");
    println!(
        "Tip: you can now delete {} — whisker keeps its own encrypted copy.",
        p8_path.display()
    );
    Ok(())
}

/// Step ②: watch ~/Downloads for a fresh `AuthKey_*.p8`. Falls back
/// to a manual path prompt on timeout (remote browser, odd download
/// dir, …). A 60s grace window before "now" catches the
/// clicked-download-before-Enter case.
fn wait_for_p8() -> Result<PathBuf> {
    let downloads = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .filter(|p| p.is_dir());
    let Some(downloads) = downloads else {
        return prompt_p8_path();
    };
    println!(
        "② Waiting for AuthKey_*.p8 to appear in {} (Ctrl-C to abort)…",
        downloads.display()
    );
    // 60s grace before "now" catches the clicked-download-before-
    // Enter case; after a declined candidate, only strictly newer
    // downloads qualify.
    let mut cutoff = SystemTime::now() - Duration::from_secs(60);
    let deadline = std::time::Instant::now() + DOWNLOAD_WAIT;
    while std::time::Instant::now() < deadline {
        if let Some(found) = newest_authkey(&downloads, cutoff) {
            println!("   ✓ detected {}", found.display());
            if prompt::confirm("Use this key?")? {
                return Ok(found);
            }
            cutoff = SystemTime::now();
            continue;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("   No download detected.");
    prompt_p8_path()
}

fn prompt_p8_path() -> Result<PathBuf> {
    let raw = prompt::line("Path to the downloaded .p8 file (drag & drop works):")?;
    // Terminal drag & drop escapes spaces; undo the common cases.
    let cleaned = raw.trim().replace("\\ ", " ");
    let path = PathBuf::from(cleaned.trim_matches('\'').trim_matches('"'));
    if !path.is_file() {
        bail!("{} is not a file", path.display());
    }
    Ok(path)
}

/// Scan for `AuthKey_*.p8` files modified after `cutoff`; newest wins.
fn newest_authkey(dir: &Path, cutoff: SystemTime) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("AuthKey_") || !name.ends_with(".p8") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if modified < cutoff {
            continue;
        }
        if best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, entry.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// `AuthKey_ABC123XYZ.p8` → `ABC123XYZ`. None if the user renamed
/// the file (wizard then asks for the Key ID).
fn key_id_from(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_prefix("AuthKey_")?.strip_suffix(".p8")?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| id.to_string())
}

fn looks_like_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(len, part)| part.len() == *len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Best-effort: failure to open a browser is not an error — the URL
/// is already on screen.
fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_id_parses_from_apple_filename() {
        assert_eq!(
            key_id_from(Path::new("/tmp/AuthKey_ABC123XYZ.p8")).as_deref(),
            Some("ABC123XYZ")
        );
        assert_eq!(key_id_from(Path::new("/tmp/renamed.p8")), None);
        assert_eq!(key_id_from(Path::new("/tmp/AuthKey_.p8")), None);
    }

    #[test]
    fn uuid_shape_check() {
        assert!(looks_like_uuid("57246542-96fe-1a63-e053-0824d011072a"));
        assert!(!looks_like_uuid("ABC123XYZ"));
        assert!(!looks_like_uuid("57246542-96fe-1a63-e053"));
    }
}
