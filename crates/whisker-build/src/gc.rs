//! Removal of build artifacts no recent whisker build still uses.
//!
//! Cargo never deletes a unit once its hash changes (a dependency bump, a
//! feature or flag change), so `target/<triple>/<profile>` grows with every
//! such change. After each successful app build, whisker records the unit
//! hashes the build used in a per-workspace manifest and deletes the units
//! no live manifest references. Manifests live in the target directory, so
//! workspaces sharing one (`CARGO_TARGET_DIR`, `build.target-dir`) keep each
//! other's units.
//!
//! Only the cross-compilation directories whisker builds into are swept;
//! `target/<profile>` also serves `cargo test` / `clippy` and is left alone.
//! Set `WHISKER_NO_GC=1` to skip the sweep.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Units younger than this survive even when unreferenced: a concurrent build
/// in a shared target directory may not have written its manifest yet.
const GRACE: Duration = Duration::from_secs(10 * 60);

/// A manifest whose workspace has not built for this long stops protecting
/// its units.
const MANIFEST_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Superseded incremental sessions of the same crate are kept this long, so
/// two workspaces sharing a target directory don't evict each other's.
const INCREMENTAL_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Disables [`record_and_sweep`] when set to anything but `0` / empty.
pub const NO_GC_ENV: &str = "WHISKER_NO_GC";

/// One build whose used units should be kept.
#[derive(Debug, Clone)]
pub struct BuildScope<'a> {
    pub workspace_root: &'a Path,
    pub triple: &'a str,
    /// `debug` / `release` — the `target/<triple>/<this>` segment.
    pub profile_dir: &'a str,
    /// Distinguishes builds of one workspace that use different units in the
    /// same directory (hot-reload dev vs. shipping release), so they don't
    /// sweep each other.
    pub variant: &'a str,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    workspace_root: PathBuf,
    variant: String,
    updated_unix_secs: u64,
    /// Unit hashes (the `-<16 hex>` suffix cargo gives each unit), plus
    /// `name:<crate>` for crates whose artifacts carry no hash (the user
    /// crate built with `cargo rustc --crate-type`), whose every unit is kept.
    units: BTreeSet<String>,
    /// Unhashed `deps/` entries the captured link of a hot-reload build
    /// references (rustc's saved temp dir and the user crate's objects).
    pinned: BTreeSet<String>,
}

/// What a sweep removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepStats {
    pub entries: usize,
    pub bytes: u64,
}

/// Record the units a successful build used and delete stale ones.
///
/// `messages` are the stdout lines of the build's cargo run with
/// `--message-format=json-*`. Failures are reported as warnings: a sweep
/// must never fail the build it follows.
pub fn record_and_sweep(scope: &BuildScope<'_>, messages: &[String]) {
    if gc_disabled() {
        return;
    }
    if let Err(e) = try_record_and_sweep(scope, messages) {
        crate::ui::warn(format!("skipped stale build artifact cleanup: {e:#}"));
    }
}

fn gc_disabled() -> bool {
    std::env::var_os(NO_GC_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

fn try_record_and_sweep(scope: &BuildScope<'_>, messages: &[String]) -> Result<()> {
    let target_dir = crate::cargo_target_dir(scope.workspace_root);
    let profile_dir = target_dir.join(scope.triple).join(scope.profile_dir);
    let units = used_units(messages, &profile_dir);
    if units.is_empty() {
        return Ok(());
    }
    let state_dir = scope.workspace_root.join("target/.whisker");
    prune_capture_caches(&state_dir);
    let manifest = Manifest {
        workspace_root: scope.workspace_root.to_path_buf(),
        variant: scope.variant.to_string(),
        updated_unix_secs: unix_secs(SystemTime::now()),
        units,
        pinned: pinned_entries(&state_dir.join("linker-args"), &profile_dir.join("deps")),
    };
    let manifest_dir = manifest_dir(&target_dir, scope.triple, scope.profile_dir);
    write_manifest(&manifest_dir, &manifest)?;

    let Some(_lock) = CargoLock::try_acquire(&profile_dir)? else {
        crate::ui::debug(format!(
            "{} is busy; cleanup deferred",
            profile_dir.display()
        ));
        return Ok(());
    };
    let (keep_units, keep_pinned) = live_references(&manifest_dir, SystemTime::now());
    let stats = sweep(&profile_dir, &keep_units, &keep_pinned, SystemTime::now());
    if stats.entries > 0 {
        crate::ui::debug(format!(
            "removed {} stale build artifact(s), {}",
            stats.entries,
            human_bytes(stats.bytes)
        ));
    }
    Ok(())
}

/// Sweep every cross-compilation directory under `workspace_root`'s target
/// directory against the live manifests, without building first.
pub fn sweep_target_dir(workspace_root: &Path) -> Result<SweepStats> {
    let target_dir = crate::cargo_target_dir(workspace_root);
    prune_capture_caches(&workspace_root.join("target/.whisker"));
    let mut total = SweepStats::default();
    let gc_root = target_dir.join(".whisker-gc");
    let Ok(triples) = std::fs::read_dir(&gc_root) else {
        return Ok(total);
    };
    for triple in triples.flatten() {
        let Ok(profiles) = std::fs::read_dir(triple.path()) else {
            continue;
        };
        for profile in profiles.flatten() {
            let manifest_dir = profile.path();
            let profile_dir = target_dir
                .join(triple.file_name())
                .join(profile.file_name());
            if !profile_dir.is_dir() {
                continue;
            }
            let Some(_lock) = CargoLock::try_acquire(&profile_dir)? else {
                continue;
            };
            let (units, pinned) = live_references(&manifest_dir, SystemTime::now());
            let stats = sweep(&profile_dir, &units, &pinned, SystemTime::now());
            total.entries += stats.entries;
            total.bytes += stats.bytes;
        }
    }
    Ok(total)
}

/// Drop `workspace_root`'s manifests so its units stop being protected.
pub fn forget_workspace(workspace_root: &Path) -> Result<()> {
    let gc_root = crate::cargo_target_dir(workspace_root).join(".whisker-gc");
    let Ok(triples) = std::fs::read_dir(&gc_root) else {
        return Ok(());
    };
    for triple in triples.flatten() {
        for profile in std::fs::read_dir(triple.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            for entry in std::fs::read_dir(profile.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let path = entry.path();
                if read_manifest(&path).is_some_and(|m| m.workspace_root == workspace_root) {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("remove {}", path.display()))?;
                }
            }
        }
    }
    Ok(())
}

fn manifest_dir(target_dir: &Path, triple: &str, profile_dir: &str) -> PathBuf {
    target_dir
        .join(".whisker-gc")
        .join(triple)
        .join(profile_dir)
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("mkdir {}", dir.display()))?;
    let key = format!(
        "{:016x}",
        fnv1a(
            format!(
                "{}\0{}",
                manifest.workspace_root.display(),
                manifest.variant
            )
            .as_bytes()
        )
    );
    let path = dir.join(format!("{key}.json"));
    let tmp = dir.join(format!("{key}.json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(manifest)?)
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("rename to {}", path.display()))
}

fn read_manifest(path: &Path) -> Option<Manifest> {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Union of the units and pinned entries of every live manifest in `dir`;
/// expired manifests are deleted.
fn live_references(dir: &Path, now: SystemTime) -> (HashSet<String>, HashSet<String>) {
    let mut units = HashSet::new();
    let mut pinned = HashSet::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(manifest) = read_manifest(&path) else {
            continue;
        };
        let age = unix_secs(now).saturating_sub(manifest.updated_unix_secs);
        if age > MANIFEST_TTL.as_secs() || !manifest.workspace_root.is_dir() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        units.extend(manifest.units);
        pinned.extend(manifest.pinned);
    }
    (units, pinned)
}

/// Unit hashes of every artifact under `profile_dir` named in `messages`,
/// fresh or rebuilt.
fn used_units(messages: &[String], profile_dir: &Path) -> BTreeSet<String> {
    let mut units = BTreeSet::new();
    let hash_of = |path: &str| -> Option<String> {
        let path = Path::new(path);
        if !path.starts_with(profile_dir) {
            return None;
        }
        let name = match path.parent().and_then(Path::file_name) {
            // `build/<pkg>-<hash>/out`
            Some(parent) if path.file_name().is_some_and(|n| n == "out") => parent,
            _ => path.file_name()?,
        };
        name.to_str().and_then(unit_hash).map(str::to_string)
    };
    for line in messages {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match message.get("reason").and_then(|r| r.as_str()) {
            Some("compiler-artifact") => {
                let mut unhashed = false;
                for file in message
                    .get("filenames")
                    .and_then(|f| f.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str())
                {
                    match hash_of(file) {
                        Some(hash) => {
                            units.insert(hash);
                        }
                        None if Path::new(file).starts_with(profile_dir) => unhashed = true,
                        None => {}
                    }
                }
                if unhashed {
                    let target = message.pointer("/target/name").and_then(|n| n.as_str());
                    let package = message
                        .get("package_id")
                        .and_then(|p| p.as_str())
                        .and_then(package_name);
                    for name in target.into_iter().chain(package) {
                        units.insert(format!("name:{name}"));
                        units.insert(format!("name:{}", name.replace('-', "_")));
                    }
                }
            }
            Some("build-script-executed") => {
                if let Some(hash) = message
                    .get("out_dir")
                    .and_then(|o| o.as_str())
                    .and_then(hash_of)
                {
                    units.insert(hash);
                }
            }
            _ => {}
        }
    }
    units
}

/// The 16-hex-digit unit hash in a cargo artifact name: `libfoo-<hash>.rlib`,
/// `foo-<hash>.d`, `foo-<hash>` (fingerprint / build-script dirs).
fn unit_hash(name: &str) -> Option<&str> {
    let bytes = name.as_bytes();
    let mut search_end = name.len();
    while let Some(dash) = name[..search_end].rfind('-') {
        let start = dash + 1;
        let end = start + 16;
        if end <= bytes.len()
            && bytes[start..end].iter().all(u8::is_ascii_hexdigit)
            && (end == bytes.len() || bytes[end] == b'.')
        {
            return Some(&name[start..end]);
        }
        search_end = dash;
    }
    None
}

/// Package name in a cargo package id, in either the `pkgid` spec form
/// (`path+file:///app#my-app@0.1.0`, `registry+…#serde@1.0.0`) or the legacy
/// `my-app 0.1.0 (path+file:///app)` form.
fn package_name(id: &str) -> Option<&str> {
    match id.rsplit_once('#') {
        Some((source, spec)) => match spec.split_once('@') {
            Some((name, _)) => Some(name),
            // `…/my-app#0.1.0`: the name is the source's last path segment.
            None => source.rsplit('/').next(),
        },
        None => id.split_whitespace().next(),
    }
}

/// `deps/` entries the newest captured link of each output references.
fn pinned_entries(linker_cache: &Path, deps_dir: &Path) -> BTreeSet<String> {
    let mut pinned = BTreeSet::new();
    for entry in std::fs::read_dir(linker_cache)
        .into_iter()
        .flatten()
        .flatten()
    {
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        for arg in value
            .get("args")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str())
        {
            let arg = arg.strip_prefix("-Wl,").unwrap_or(arg);
            if let Ok(rest) = Path::new(arg).strip_prefix(deps_dir)
                && let Some(first) = rest.components().next()
            {
                pinned.insert(first.as_os_str().to_string_lossy().into_owned());
            }
        }
    }
    pinned
}

/// Keep only the newest capture per crate (rustc) or output (linker); the
/// dev-server reads nothing else.
fn prune_capture_caches(state_dir: &Path) {
    prune_newest_per_key(&state_dir.join("rustc-args"), |v| {
        let crate_name = v.get("crate_name")?.as_str()?.to_string();
        let args = v.get("args")?.as_array()?;
        let target = args
            .iter()
            .position(|a| a.as_str() == Some("--target"))
            .and_then(|i| args.get(i + 1))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        Some(format!("{crate_name}\0{target}"))
    });
    prune_newest_per_key(&state_dir.join("linker-args"), |v| {
        v.get("output")?.as_str().map(str::to_string)
    });
}

fn prune_newest_per_key(dir: &Path, key: impl Fn(&serde_json::Value) -> Option<String>) {
    let mut newest: HashMap<String, (u128, PathBuf)> = HashMap::new();
    let mut stale = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(value) = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else {
            continue;
        };
        let stamp = value
            .get("timestamp_micros")
            .and_then(|t| t.as_u64())
            .map(u128::from)
            .unwrap_or(0);
        let key = key(&value).unwrap_or_else(|| "_unknown".to_string());
        match newest.get(&key) {
            Some((prev, _)) if *prev >= stamp => stale.push(path),
            _ => {
                if let Some((_, old)) = newest.insert(key, (stamp, path)) {
                    stale.push(old);
                }
            }
        }
    }
    for path in stale {
        let _ = std::fs::remove_file(path);
    }
}

/// Delete everything under `profile_dir` that no live manifest references
/// and that is older than [`GRACE`].
fn sweep(
    profile_dir: &Path,
    keep_units: &HashSet<String>,
    keep_pinned: &HashSet<String>,
    now: SystemTime,
) -> SweepStats {
    let mut stats = SweepStats::default();
    let old_enough = |path: &Path, grace: Duration| {
        std::fs::symlink_metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age >= grace)
    };
    let mut remove = |path: &Path| {
        let bytes = disk_usage(path);
        let removed = if path.is_dir() && !path.is_symlink() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        if removed.is_ok() {
            stats.entries += 1;
            stats.bytes += bytes;
        }
    };

    for sub in ["deps", "build", ".fingerprint"] {
        for entry in std::fs::read_dir(profile_dir.join(sub))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let stale = match unit_hash(name) {
                Some(hash) => {
                    let stem = name
                        .rsplit_once(&format!("-{hash}"))
                        .map_or(name, |(stem, _)| stem);
                    let crate_name = if sub == "deps" {
                        stem.strip_prefix("lib").unwrap_or(stem)
                    } else {
                        stem
                    };
                    !keep_units.contains(hash)
                        && !keep_units.contains(&format!("name:{crate_name}"))
                }
                // rustc's `-Csave-temps` leftovers: codegen units and the
                // `rmeta*` / `rustc*` temp dirs.
                None if sub == "deps" => {
                    !keep_pinned.contains(name)
                        && (name.contains(".rcgu.")
                            || (path.is_dir()
                                && (name.starts_with("rmeta") || name.starts_with("rustc"))))
                }
                None => false,
            };
            if stale && old_enough(&path, GRACE) {
                remove(&path);
            }
        }
    }

    let mut sessions: HashMap<String, Vec<(SystemTime, PathBuf)>> = HashMap::new();
    for entry in std::fs::read_dir(profile_dir.join("incremental"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((crate_name, _)) = name.rsplit_once('-') else {
            continue;
        };
        let modified = latest_mtime(&path);
        sessions
            .entry(crate_name.to_string())
            .or_default()
            .push((modified, path));
    }
    for (_, mut dirs) in sessions {
        dirs.sort_by(|a, b| b.0.cmp(&a.0));
        for (modified, path) in dirs.into_iter().skip(1) {
            if now
                .duration_since(modified)
                .is_ok_and(|age| age >= INCREMENTAL_TTL)
            {
                remove(&path);
            }
        }
    }
    stats
}

/// Newest mtime of `dir` and its direct children (rustc writes the session
/// subdirectories, not the crate directory itself).
fn latest_mtime(dir: &Path) -> SystemTime {
    let own = std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().and_then(|m| m.modified()).ok())
        .fold(own, SystemTime::max)
}

fn disk_usage(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| disk_usage(&e.path()))
        .sum()
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Human-readable byte count for summaries.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Exclusive hold on cargo's own build-directory lock, so a sweep never runs
/// while cargo builds into the same directory.
struct CargoLock(std::fs::File);

impl CargoLock {
    fn try_acquire(profile_dir: &Path) -> Result<Option<Self>> {
        use std::os::fd::AsRawFd;
        let path = profile_dir.join(".cargo-lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        // SAFETY: `file` owns a valid descriptor for the duration of the call.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            Ok(Some(Self(file)))
        } else {
            Ok(None)
        }
    }
}

impl Drop for CargoLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor stays valid until `self.0` drops after this.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("whisker-gc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn unit_hash_reads_cargo_artifact_names() {
        assert_eq!(
            unit_hash("libserde-2bf6b42e2b453ca1.rlib"),
            Some("2bf6b42e2b453ca1")
        );
        assert_eq!(
            unit_hash("serde-2bf6b42e2b453ca1.d"),
            Some("2bf6b42e2b453ca1")
        );
        assert_eq!(
            unit_hash("serde_json-2bf6b42e2b453ca1"),
            Some("2bf6b42e2b453ca1")
        );
        assert_eq!(
            unit_hash("tokio-tungstenite-0123456789abcdef"),
            Some("0123456789abcdef")
        );
        assert_eq!(unit_hash("libapp.so"), None);
        assert_eq!(unit_hash("app.app.c044c7a815ae222e-cgu.0.rcgu.o"), None);
        assert_eq!(unit_hash("rustcJYafpA"), None);
    }

    #[test]
    fn used_units_collects_fresh_artifacts_and_build_script_outputs() {
        let profile = Path::new("/t/aarch64-linux-android/debug");
        let messages = vec![
            r#"{"reason":"compiler-artifact","fresh":true,"filenames":["/t/aarch64-linux-android/debug/deps/libserde-2bf6b42e2b453ca1.rlib"]}"#.to_string(),
            r#"{"reason":"compiler-artifact","fresh":false,"filenames":["/t/debug/build/serde-1111111111111111/build-script-build"]}"#.to_string(),
            r#"{"reason":"build-script-executed","out_dir":"/t/aarch64-linux-android/debug/build/serde-2222222222222222/out"}"#.to_string(),
            "not json".to_string(),
        ];
        let units = used_units(&messages, profile);
        assert_eq!(
            units.into_iter().collect::<Vec<_>>(),
            vec![
                "2222222222222222".to_string(),
                "2bf6b42e2b453ca1".to_string()
            ]
        );
    }

    #[test]
    fn used_units_names_crates_whose_artifacts_carry_no_hash() {
        let profile = Path::new("/t/aarch64-linux-android/debug");
        let messages = vec![
            r#"{"reason":"compiler-artifact","package_id":"path+file:///app#disk-probe@0.1.0","target":{"name":"disk_probe"},"filenames":["/t/aarch64-linux-android/debug/deps/libdisk_probe.so"]}"#.to_string(),
        ];
        let units = used_units(&messages, profile);
        assert!(units.contains("name:disk_probe"));
        assert!(units.contains("name:disk-probe"));
    }

    #[test]
    fn package_name_reads_both_id_forms() {
        assert_eq!(
            package_name("path+file:///app#disk-probe@0.1.0"),
            Some("disk-probe")
        );
        assert_eq!(package_name("path+file:///w/my-app#0.1.0"), Some("my-app"));
        assert_eq!(
            package_name("my-app 0.1.0 (path+file:///w/my-app)"),
            Some("my-app")
        );
    }

    #[test]
    fn sweep_removes_only_unreferenced_old_entries() {
        let profile = scratch("sweep");
        let kept = profile.join("deps/libkeep-aaaaaaaaaaaaaaaa.rlib");
        let stale = profile.join("deps/libold-bbbbbbbbbbbbbbbb.rlib");
        let stale_fp = profile.join(".fingerprint/old-bbbbbbbbbbbbbbbb/lib-old");
        let temps = profile.join("deps/old.old.1234-cgu.0.rcgu.o");
        let pinned_dir = profile.join("deps/rustcAbCdEf/symbols.o");
        let uplifted = profile.join("deps/libapp.so");
        let named_fp = profile.join(".fingerprint/my-app-cccccccccccccccc/lib-app");
        let named_dep = profile.join("deps/my_app-cccccccccccccccc.d");
        for p in [
            &kept,
            &stale,
            &stale_fp,
            &temps,
            &pinned_dir,
            &uplifted,
            &named_fp,
            &named_dep,
        ] {
            touch(p);
        }
        let keep_units = HashSet::from([
            "aaaaaaaaaaaaaaaa".to_string(),
            "name:my-app".to_string(),
            "name:my_app".to_string(),
        ]);
        let keep_pinned = HashSet::from(["rustcAbCdEf".to_string()]);

        let fresh = sweep(&profile, &keep_units, &keep_pinned, SystemTime::now());
        assert_eq!(fresh.entries, 0, "entries inside the grace period survive");

        let later = SystemTime::now() + GRACE + Duration::from_secs(1);
        let stats = sweep(&profile, &keep_units, &keep_pinned, later);
        assert_eq!(stats.entries, 3);
        assert!(kept.exists());
        assert!(!stale.exists());
        assert!(!stale_fp.parent().unwrap().exists());
        assert!(!temps.exists());
        assert!(pinned_dir.exists());
        assert!(uplifted.exists());
        assert!(named_fp.exists() && named_dep.exists());
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn sweep_keeps_the_newest_incremental_session_per_crate() {
        let profile = scratch("incremental");
        let older = profile.join("incremental/app-1abc/s-old/dep-graph.bin");
        touch(&older);
        std::thread::sleep(Duration::from_millis(20));
        let newer = profile.join("incremental/app-2def/s-new/dep-graph.bin");
        touch(&newer);
        let later = SystemTime::now() + INCREMENTAL_TTL + Duration::from_secs(1);
        sweep(&profile, &HashSet::new(), &HashSet::new(), later);
        assert!(!older.exists());
        assert!(newer.exists());
        let _ = std::fs::remove_dir_all(&profile);
    }

    #[test]
    fn manifests_of_other_workspaces_protect_their_units() {
        let target = scratch("manifests");
        let ws_a = target.join("a");
        let ws_b = target.join("b");
        std::fs::create_dir_all(&ws_a).unwrap();
        std::fs::create_dir_all(&ws_b).unwrap();
        let dir = manifest_dir(&target, "aarch64-apple-ios-sim", "release");
        for (ws, unit) in [(&ws_a, "aaaaaaaaaaaaaaaa"), (&ws_b, "bbbbbbbbbbbbbbbb")] {
            write_manifest(
                &dir,
                &Manifest {
                    workspace_root: ws.clone(),
                    variant: "hot".into(),
                    updated_unix_secs: unix_secs(SystemTime::now()),
                    units: BTreeSet::from([unit.to_string()]),
                    pinned: BTreeSet::new(),
                },
            )
            .unwrap();
        }
        let (units, _) = live_references(&dir, SystemTime::now());
        assert!(units.contains("aaaaaaaaaaaaaaaa") && units.contains("bbbbbbbbbbbbbbbb"));

        std::fs::remove_dir_all(&ws_b).unwrap();
        let (units, _) = live_references(&dir, SystemTime::now());
        assert!(units.contains("aaaaaaaaaaaaaaaa") && !units.contains("bbbbbbbbbbbbbbbb"));

        let expired = SystemTime::now() + MANIFEST_TTL + Duration::from_secs(1);
        let (units, _) = live_references(&dir, expired);
        assert!(units.is_empty());
        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn capture_caches_keep_the_newest_entry_per_key() {
        let state = scratch("captures");
        let rustc = state.join("rustc-args");
        std::fs::create_dir_all(&rustc).unwrap();
        for (file, stamp, target) in [
            ("app-1.json", 1, "aarch64-linux-android"),
            ("app-2.json", 2, "aarch64-linux-android"),
            ("app-3.json", 3, "aarch64-apple-ios-sim"),
        ] {
            std::fs::write(
                rustc.join(file),
                format!(
                    r#"{{"crate_name":"app","args":["--target","{target}"],"timestamp_micros":{stamp}}}"#
                ),
            )
            .unwrap();
        }
        prune_capture_caches(&state);
        assert!(!rustc.join("app-1.json").exists());
        assert!(rustc.join("app-2.json").exists());
        assert!(rustc.join("app-3.json").exists());
        let _ = std::fs::remove_dir_all(&state);
    }
}
