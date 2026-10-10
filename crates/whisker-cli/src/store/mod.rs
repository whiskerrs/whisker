//! `whisker store` — keep the stores' own records of the app (store
//! page text, categories, review contacts) in step with `store.rs`:
//! `push` writes what it declares, `pull` prints what the store has.
//!
//! Split from `whisker submit` on purpose: submit sends a binary and
//! what belongs to that one binary; this sends what describes the app
//! regardless of any binary, and is run when that description
//! changes.

mod appstore;
mod playstore;

use anyhow::{Context, Result, ensure};
use clap::{Args, Subcommand};
use std::path::Path;
use whisker_config::store::{SCHEMA_VERSION, StoreConfig, StoreReport};

#[derive(Args, Debug)]
pub struct StoreArgs {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Send what `store.rs` declares to a store. Only what is set is
    /// written; everything else is left as the store has it.
    Push(PushArgs),
    /// Read what a store currently holds and print it as a
    /// `store.rs`. Changes nothing, in the store or on disk —
    /// redirect the output to start a `store.rs` from the live data.
    Pull(PullArgs),
}

#[derive(Args, Debug)]
struct PullArgs {
    #[command(subcommand)]
    target: PullTarget,
}

#[derive(Subcommand, Debug)]
enum PullTarget {
    /// App Store Connect — everything `store push appstore` writes.
    Appstore(appstore::PullArgs),
    /// Google Play — everything `store push playstore` writes.
    Playstore(playstore::PullArgs),
}

#[derive(Args, Debug)]
struct PushArgs {
    #[command(subcommand)]
    target: Target,
}

#[derive(Subcommand, Debug)]
enum Target {
    /// App Store Connect — the `appstore` section, except
    /// `beta_build` (sent by `whisker submit ios`).
    Appstore(appstore::Args),
    /// Google Play — the `playstore` section, except `release` (sent
    /// by `whisker submit android --track`).
    Playstore(playstore::Args),
}

pub fn run(args: StoreArgs) -> Result<()> {
    match args.cmd {
        Cmd::Push(push) => match push.target {
            Target::Appstore(a) => appstore::run(a),
            Target::Playstore(a) => playstore::run(a),
        },
        Cmd::Pull(pull) => match pull.target {
            PullTarget::Appstore(a) => appstore::pull(a),
            PullTarget::Playstore(a) => playstore::pull(a),
        },
    }
}

/// Run the app's `store.rs`, if it has one.
pub(crate) fn load(crate_dir: &Path) -> Result<Option<StoreConfig>> {
    let Some(bytes) = whisker_cng::run_store(&crate_dir.join("Cargo.toml"))? else {
        return Ok(None);
    };
    let report: StoreReport = serde_json::from_slice(&bytes).context("read store.rs report")?;
    ensure!(
        report.schema_version == SCHEMA_VERSION,
        "store.rs reported schema version {}, but this whisker CLI reads {SCHEMA_VERSION} — \
         match the app's `whisker` version to the CLI",
        report.schema_version
    );
    Ok(Some(report.config))
}

/// `store push` has nothing to do without a `store.rs`.
fn require(crate_dir: &Path) -> Result<StoreConfig> {
    load(crate_dir)?.with_context(|| {
        format!(
            "no store.rs next to {} — create one that calls `whisker::store::run`",
            crate_dir.join("Cargo.toml").display()
        )
    })
}
