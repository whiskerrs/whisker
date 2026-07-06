//! `whisker credential` — signing-credential management.
//!
//! ## Boundary with `whisker build`
//!
//! Everything that *creates or stores* signing material lives here;
//! `whisker build` only *consumes* it (via
//! `whisker_credentials::Store::{ios_signing, android_signing}`)
//! and, when something is missing, either runs these wizards inline
//! (TTY) or fails with the exact command to run (CI). Build code
//! never writes to `credentials/`.
//!
//! ## Bootstrap is implicit
//!
//! There is deliberately no `init` subcommand: the first
//! `whisker credential ios` / `android` bootstraps the store
//! (generates the age key pair, writes `credentials/recipients.txt`)
//! idempotently before running its wizard. The secret key is shown
//! exactly once and never persisted — afterwards it arrives via
//! `$WHISKER_CREDENTIALS_KEY` or an interactive prompt at build
//! time. Because the store is asymmetric, a teammate's clone can
//! ADD credentials without ever holding the secret key.

mod android;
mod asc;
mod ios;
mod prompt;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use std::path::Path;
use whisker_credentials::{AndroidSigning, Identity, MaterializedDir, Store};

pub use android::AndroidArgs;

#[derive(Args, Debug)]
pub struct CredentialArgs {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Acquire the App Store Connect API key (Team Key, Admin) via a
    /// guided wizard and store it encrypted under `credentials/`.
    /// First run also bootstraps the store.
    Ios(ios::IosArgs),
    /// Generate (or import) the Android upload keystore for this
    /// app's applicationId and store it encrypted under
    /// `credentials/`. First run also bootstraps the store.
    Android(android::AndroidArgs),
}

pub fn run(args: CredentialArgs) -> Result<()> {
    match args.cmd {
        Cmd::Ios(a) => ios::run(a),
        Cmd::Android(a) => android::run(a),
    }
}

/// Open the store, bootstrapping it interactively on first use.
///
/// Bootstrap runs BEFORE the calling wizard so an aborted wizard
/// leaves a valid (empty) store, never a broken half-state. If the
/// user declines the "saved the key?" confirmation the bootstrap is
/// rolled back — a store whose secret key nobody saved is
/// permanently undecryptable and must not be left behind.
pub(crate) fn open_or_bootstrap(project_root: &Path) -> Result<Store> {
    if Store::exists(project_root) {
        return Store::open(project_root);
    }
    if !prompt::is_interactive() {
        bail!(
            "credentials store not found at {} — run `whisker credential ios` or \
             `whisker credential android` locally first, then commit the credentials/ directory",
            Store::dir_for(project_root).display()
        );
    }

    println!("First-time credential setup:");
    let (store, secret) = Store::bootstrap(project_root)?;
    println!(
        "  ✓ created {}/{} (public key — safe to commit)",
        whisker_credentials::DIR_NAME,
        whisker_credentials::RECIPIENTS_FILE,
    );
    println!();
    println!("  Your credential encryption key (shown ONCE, never stored by whisker):");
    println!();
    println!("      {secret}");
    println!();
    println!("  Store it in your password manager now. Builds will ask for it");
    println!(
        "  (or read ${}). Losing it means losing every credential in this store.",
        whisker_credentials::IDENTITY_ENV,
    );
    println!();
    let saved = prompt::confirm("Saved to your password manager?")?;
    if !saved {
        // Roll back: an unsaved key makes the store write-only garbage.
        std::fs::remove_dir_all(Store::dir_for(project_root))
            .context("roll back credentials dir")?;
        bail!("aborted — nothing was created. Re-run when ready to save the key.");
    }
    println!(
        "  Note: credentials/ contents are age-encrypted, but in a PUBLIC repository the\n\
         \x20 ciphertext lives in history forever — if the key ever leaks, rotate the\n\
         \x20 underlying credentials themselves, not just the key."
    );
    println!(
        "  CI setup: `gh secret set {}` with this key.",
        whisker_credentials::IDENTITY_ENV,
    );
    println!();
    Ok(store)
}

/// Acquire the decryption identity: `$WHISKER_CREDENTIALS_KEY` first,
/// then an interactive hidden prompt (3 attempts). The pasted key is
/// checked against `recipients.txt` *before* any decryption so a
/// wrong-repo key fails with a specific message instead of age's
/// generic "no matching keys".
pub(crate) fn obtain_identity(store: &Store) -> Result<Identity> {
    if let Some(identity) = Identity::from_env()? {
        if !identity.matches(store) {
            bail!(
                "${} does not match this repository's credentials/recipients.txt \
                 (key for a different repo, or the store was re-bootstrapped?)",
                whisker_credentials::IDENTITY_ENV,
            );
        }
        return Ok(identity);
    }
    if !prompt::is_interactive() {
        bail!(
            "${} is not set — add the credential key to this environment's secrets \
             (`gh secret set {}` for GitHub Actions)",
            whisker_credentials::IDENTITY_ENV,
            whisker_credentials::IDENTITY_ENV,
        );
    }
    for _ in 0..3 {
        let pasted =
            prompt::password("Credential key (AGE-SECRET-KEY-…, from your password manager):")?;
        match Identity::parse(&pasted) {
            Ok(identity) if identity.matches(store) => return Ok(identity),
            Ok(_) => println!("   That key doesn't match this repository's credentials store."),
            Err(e) => println!("   {e}"),
        }
    }
    bail!("no valid credential key after 3 attempts")
}

/// Build pre-step for `whisker build ipa`: staged iOS signing
/// inputs, with the same two invariants as the Android variant
/// (CI never creates credentials; keep the guard alive through
/// xcodebuild).
pub(crate) fn require_ios_signing(
    project_root: &Path,
    bundle_id: &str,
) -> Result<(MaterializedDir, whisker_credentials::IosSigning)> {
    let store = if Store::exists(project_root) {
        Store::open(project_root)?
    } else {
        if !prompt::is_interactive() {
            bail!(
                "no credentials store at {} — run `whisker credential ios` locally, \
                 commit the credentials/ directory, and set ${} in CI",
                Store::dir_for(project_root).display(),
                whisker_credentials::IDENTITY_ENV,
            );
        }
        println!("iOS release signing isn't set up for this app yet.");
        open_or_bootstrap(project_root)?
    };

    if store.resolve_ios_rel(bundle_id).is_none() {
        if !prompt::is_interactive() {
            bail!(
                "no App Store Connect key in credentials/ — run `whisker credential ios` \
                 locally and commit"
            );
        }
        println!("No App Store Connect API key stored yet — starting the setup wizard.");
        ios::acquire_and_store(&store, &whisker_credentials::ios_asc_rel(None))?;
    }

    let identity = obtain_identity(&store)?;
    let staging = MaterializedDir::new()?;
    let signing = store
        .ios_signing(bundle_id, &identity, &staging)?
        .ok_or_else(|| {
            anyhow!("App Store Connect key entry vanished — re-run `whisker credential ios`")
        })?;
    Ok((staging, signing))
}

/// Build pre-step for `whisker build appbundle|apk`: produce staged
/// Android signing inputs, creating missing pieces inline when a
/// human is present. Two invariants live here:
///
/// - **CI never creates credentials.** A keystore minted on an
///   ephemeral runner would sign a release nobody can ever sign
///   again — non-interactive sessions fail with the exact command
///   to run locally instead.
/// - Keep the returned [`MaterializedDir`] alive for the whole
///   gradle invocation; the signing paths point into it.
pub(crate) fn require_android_signing(
    project_root: &Path,
    application_id: &str,
) -> Result<(MaterializedDir, AndroidSigning)> {
    let store = if Store::exists(project_root) {
        Store::open(project_root)?
    } else {
        if !prompt::is_interactive() {
            bail!(
                "no credentials store at {} — run `whisker credential android` locally, \
                 commit the credentials/ directory, and set ${} in CI",
                Store::dir_for(project_root).display(),
                whisker_credentials::IDENTITY_ENV,
            );
        }
        println!("Android release signing isn't set up for this app yet.");
        open_or_bootstrap(project_root)?
    };

    if !store.has(&whisker_credentials::android_keystore_rel(application_id)) {
        if !prompt::is_interactive() {
            bail!(
                "no upload keystore for {application_id} in credentials/ — run \
                 `whisker credential android --id {application_id}` locally and commit"
            );
        }
        println!("No upload keystore for {application_id} yet — creating one.");
        android::create_and_store_keystore(&store, application_id)?;
    }

    let identity = obtain_identity(&store)?;
    let staging = MaterializedDir::new()?;
    let signing = store
        .android_signing(application_id, &identity, &staging)?
        .ok_or_else(|| {
            anyhow!(
                "keystore entry for {application_id} is incomplete — re-run \
                 `whisker credential android --id {application_id}`"
            )
        })?;
    Ok((staging, signing))
}
