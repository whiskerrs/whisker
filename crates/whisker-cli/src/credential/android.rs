//! `whisker credential android` — mint (or import) the upload
//! keystore for one applicationId and store it encrypted.
//!
//! Generation is the default path: `keytool -genkeypair` with
//! random passwords the user never sees. That's safe because the
//! committed repo is the backup (the whole point of the encrypted
//! store) and, under Play App Signing, a lost *upload* key is
//! recoverable through Google support — unlike the pre-Play-App-
//! Signing world where key loss killed the app.
//!
//! Keystores are exact-match per applicationId (no `default`
//! fallback): separate environments (`com.example.app.dev` /
//! `com.example.app`) get separate keys, so a leaked dev keystore
//! never touches production.

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use rand::Rng;
use rand::distributions::Alphanumeric;
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_credentials::{KeystoreMeta, MaterializedDir, android_keystore_rel, android_meta_rel};

use super::prompt;
use crate::manifest;

#[derive(Args, Debug)]
pub struct AndroidArgs {
    /// applicationId to store the keystore under. Defaults to the
    /// value your `whisker.rs` resolves to (honours WHISKER_ENV-style
    /// branching in `configure`).
    #[arg(long)]
    id: Option<String>,

    /// Import an existing keystore file instead of generating a new
    /// one (migration path for already-published apps).
    #[arg(long, value_name = "PATH")]
    import: Option<PathBuf>,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: AndroidArgs) -> Result<()> {
    let m = manifest::resolve(args.manifest_path.as_deref())?;
    let application_id = match args.id {
        Some(id) => id,
        // Same resolution `whisker run android` uses: explicit
        // android.application_id, else the shared bundle_id.
        None => manifest::android_application_id(&m.config).ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.android(|a| a.application_id(\"…\")) (or app.bundle_id) \
                 is required — or pass --id <application_id>"
            )
        })?,
    };

    let store = super::open_or_bootstrap(&m.crate_dir)?;

    let jks_rel = android_keystore_rel(&application_id);
    if store.has(&jks_rel) {
        if !prompt::is_interactive() {
            bail!("a keystore for {application_id} already exists in credentials/");
        }
        println!(
            "A keystore for {application_id} already exists. Replacing it means Play will\n\
             reject uploads signed with the new key unless you reset the upload key with\n\
             Google Play support first."
        );
        if !prompt::confirm("Replace it?")? {
            bail!("aborted — existing keystore kept.");
        }
    }

    match &args.import {
        Some(path) => {
            let (jks_bytes, meta) = import_keystore(path)?;
            store_keystore(&store, &application_id, &jks_bytes, &meta)
        }
        None => create_and_store_keystore(&store, &application_id),
    }
}

/// Generate + store a fresh keystore for `application_id`. Also the
/// inline pre-step `whisker build appbundle|apk` runs when the entry
/// is missing (interactive sessions only — build never creates
/// credentials in CI).
pub(crate) fn create_and_store_keystore(
    store: &whisker_credentials::Store,
    application_id: &str,
) -> Result<()> {
    let (jks_bytes, meta) = generate_keystore(application_id)?;
    store_keystore(store, application_id, &jks_bytes, &meta)
}

fn store_keystore(
    store: &whisker_credentials::Store,
    application_id: &str,
    jks_bytes: &[u8],
    meta: &KeystoreMeta,
) -> Result<()> {
    let jks_rel = android_keystore_rel(application_id);
    let meta_rel = android_meta_rel(application_id);
    store.put(&jks_rel, jks_bytes)?;
    store.put(
        &meta_rel,
        &serde_json::to_vec(meta).context("serialize keystore metadata")?,
    )?;

    println!("  ✓ credentials/{jks_rel}.age");
    println!("  ✓ credentials/{meta_rel}.age");
    println!();
    println!("Commit the credentials/ directory — the repo is this keystore's backup.");
    println!("`whisker build appbundle` / `apk` will use it automatically.");
    Ok(())
}

/// Default path: mint a fresh keystore with `keytool` inside a
/// 0700 staging dir (plaintext never touches the repo), then read
/// the bytes back for encryption.
fn generate_keystore(application_id: &str) -> Result<(Vec<u8>, KeystoreMeta)> {
    let java_home = whisker_build::android::resolve_java_home()
        .context("generating a keystore needs a JDK (`keytool`) — install one or set JAVA_HOME")?;
    let keytool = java_home.join("bin/keytool");

    // PKCS12 (modern keytool default) has a single password; keep
    // both fields equal so the gradle signingConfig stays uniform
    // across generated and imported keystores.
    let store_password = random_password();
    let meta = KeystoreMeta {
        key_password: store_password.clone(),
        store_password,
        key_alias: "upload".to_string(),
    };

    let staging = MaterializedDir::new()?;
    let jks_path = staging.dir_path().join("upload.jks");
    // Passwords go via environment (`-storepass:env`), not argv —
    // argv is world-readable in `ps` for the keytool's lifetime.
    let out = Command::new(&keytool)
        .args(["-genkeypair", "-keystore"])
        .arg(&jks_path)
        .args([
            "-alias",
            &meta.key_alias,
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            // ~30 years. Play requires validity past 2033; effectively
            // "never expires" for an upload key.
            "-validity",
            "10950",
            "-storepass:env",
            "WHISKER_KEYTOOL_STOREPASS",
            "-keypass:env",
            "WHISKER_KEYTOOL_KEYPASS",
            "-dname",
        ])
        .arg(format!("CN={application_id} upload key, O=whisker"))
        .env("WHISKER_KEYTOOL_STOREPASS", &meta.store_password)
        .env("WHISKER_KEYTOOL_KEYPASS", &meta.key_password)
        .output()
        .with_context(|| format!("spawn {}", keytool.display()))?;
    if !out.status.success() {
        bail!(
            "keytool -genkeypair failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let bytes = std::fs::read(&jks_path)
        .with_context(|| format!("read generated keystore {}", jks_path.display()))?;
    println!("  ✓ generated a new upload keystore (alias `upload`, RSA 2048, 30y validity)");
    Ok((bytes, meta))
}

/// Migration path: take an existing keystore + its passwords.
fn import_keystore(path: &Path) -> Result<(Vec<u8>, KeystoreMeta)> {
    if !prompt::is_interactive() {
        bail!("--import needs an interactive terminal for the password prompts");
    }
    let bytes = std::fs::read(path).with_context(|| format!("read keystore {}", path.display()))?;
    let store_password = prompt::password("Keystore password:")?;
    let key_alias = {
        let a = prompt::line("Key alias (see `keytool -list -keystore <file>`):")?;
        if a.is_empty() {
            bail!("key alias is required");
        }
        a
    };
    let key_password = {
        let p = prompt::password("Key password (Enter = same as keystore password):")?;
        if p.is_empty() {
            store_password.clone()
        } else {
            p
        }
    };
    Ok((
        bytes,
        KeystoreMeta {
            store_password,
            key_password,
            key_alias,
        },
    ))
}

/// 32 alphanumeric chars (~190 bits) — nobody types these, they live
/// encrypted next to the keystore.
fn random_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}
