//! Whisker build plugin for the asset system.
//!
//! Phase 1 gave apps the runtime half — the [`asset!`](crate::asset)
//! macro + the [`resolve`](crate::resolve) path composer. Phase 2 (this
//! module) is the *build* half: a [`Plugin`] that bundles the app's
//! declared asset files into the generated native projects so that, at
//! runtime, the Phase 1 resolver actually finds them.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! use whisker_asset::WhiskerAsset;
//!
//! app.plugin::<WhiskerAsset>(|c| {
//!     c.dir("assets");            // bundle ./assets/** (recursively)
//!     // c.file("branding/logo.png"); // or an individual file
//! });
//! ```
//!
//! Paths are relative to the **app crate root** (the directory holding
//! `Cargo.toml` / `whisker.rs`).
//!
//! ## What `apply` produces
//!
//! For each declared dir/file the plugin enumerates the files
//! (recursively for `dir`) and places each one into **both** platforms
//! under a per-platform namespace that mirrors Phase 1's resolver:
//!
//! - **Android** → `gen/android/app/src/main/assets/whisker/<rel>` via
//!   `ctx.android.extra_files`. AGP copies `assets/` verbatim into the
//!   APK, exposed at `file:///android_asset/whisker/<rel>` — exactly
//!   what [`AssetBase::AndroidAssets`](crate::AssetBase::AndroidAssets)
//!   resolves to. No manifest / gradle registration needed.
//! - **iOS** → `gen/ios/whisker_assets/<rel>` via `ctx.ios.extra_files`,
//!   plus a single **folder-reference** registration of the
//!   `whisker_assets` directory in the app target's Resources build
//!   phase (`PbxprojOp::AddResourceFolder`). The folder reference makes
//!   Xcode copy the whole tree into the `.app` preserving
//!   subdirectories, so an asset lands at
//!   `<bundle>/whisker_assets/<rel>` — what
//!   [`AssetBase::IosDir`](crate::AssetBase::IosDir) resolves to.
//!
//! The `<rel>` for an asset is its path **relative to the declared
//! root**: a file `assets/images/logo.png` declared via `c.dir("assets")`
//! bundles at `whisker/images/logo.png` (Android) /
//! `whisker_assets/images/logo.png` (iOS) and is referenced from Rust
//! as `asset!("images/logo.png")`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use whisker_plugin::{
    FileEntry, GenerateContext, Operation, PbxprojOp, Plugin, PluginConfig, Target,
};

/// iOS bundle subdirectory the assets land under. Matches the
/// `whisker_assets/` segment baked into
/// [`AssetBase::IosDir`](crate::AssetBase::IosDir)'s resolution.
const IOS_NAMESPACE: &str = "whisker_assets";
/// Android `assets/` subdirectory the assets land under. Matches the
/// `whisker/` segment in
/// [`AssetBase::AndroidAssets`](crate::AssetBase::AndroidAssets)'s
/// `file:///android_asset/whisker/` prefix.
const ANDROID_NAMESPACE: &str = "whisker";
/// Path inside `gen/android/` the Android `assets/` dir lives at. AGP's
/// default source set copies `app/src/main/assets/**` into the APK.
const ANDROID_ASSETS_ROOT: &str = "app/src/main/assets";

/// Typed config the user spells in `whisker.rs` via
/// `app.plugin::<WhiskerAsset>(|c| …)`.
///
/// Holds the declared asset roots. Both `dirs` and `files` are paths
/// **relative to the app crate root**; `validate` checks they exist on
/// disk, and `apply` enumerates them.
#[derive(Default, Serialize, Deserialize)]
pub struct WhiskerAssetConfig {
    /// Directories to bundle recursively. Every regular file beneath
    /// each entry is included, keyed by its path relative to that
    /// directory.
    #[serde(default)]
    pub dirs: Vec<PathBuf>,
    /// Individual files to bundle. Keyed by the file's basename (its
    /// path relative to its own parent directory).
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

impl WhiskerAssetConfig {
    /// Bundle a directory recursively. Path is relative to the app
    /// crate root, e.g. `c.dir("assets")`.
    pub fn dir(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.dirs.push(path.into());
        self
    }

    /// Bundle a single file. Path is relative to the app crate root,
    /// e.g. `c.file("branding/logo.png")`. The file bundles under its
    /// **basename** — `branding/logo.png` → `<ns>/logo.png` — so a Rust
    /// `asset!("logo.png")` finds it. Use [`Self::dir`] to preserve a
    /// subdirectory layout.
    pub fn file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.files.push(path.into());
        self
    }
}

impl PluginConfig for WhiskerAssetConfig {
    const NAME: &'static str = "whisker-asset";
}

/// The plugin the Whisker engine drives — either in-process or as a
/// subprocess via the bundled `whisker-asset-plugin` binary.
pub struct WhiskerAsset;

/// One bundled asset: its logical relative path (the `<rel>` both
/// platforms key under) plus its raw bytes.
struct ResolvedAsset {
    rel: PathBuf,
    bytes: Vec<u8>,
}

impl WhiskerAsset {
    /// Enumerate every declared dir/file into `(rel, bytes)` pairs,
    /// reading from `crate_root`. Detects collisions (two sources
    /// resolving to the same `<rel>`) and missing paths.
    fn collect(cfg: &WhiskerAssetConfig, crate_root: &Path) -> anyhow::Result<Vec<ResolvedAsset>> {
        // rel → source path, for collision diagnostics.
        let mut seen: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
        let mut out: Vec<ResolvedAsset> = Vec::new();

        for dir in &cfg.dirs {
            let abs = crate_root.join(dir);
            if !abs.is_dir() {
                anyhow::bail!(
                    "whisker-asset: declared dir `{}` does not exist (resolved to `{}`, \
                     relative to the app crate root). Create it or remove the \
                     `c.dir(...)` call.",
                    dir.display(),
                    abs.display(),
                );
            }
            collect_dir(&abs, &abs, &mut seen, &mut out)?;
        }

        for file in &cfg.files {
            let abs = crate_root.join(file);
            if !abs.is_file() {
                anyhow::bail!(
                    "whisker-asset: declared file `{}` does not exist (resolved to `{}`, \
                     relative to the app crate root). Fix the path or remove the \
                     `c.file(...)` call.",
                    file.display(),
                    abs.display(),
                );
            }
            // Individual files bundle under their basename.
            let rel = PathBuf::from(
                abs.file_name()
                    .expect("is_file() implies a file name component"),
            );
            insert_asset(&abs, rel, &mut seen, &mut out)?;
        }

        Ok(out)
    }
}

/// Recursively walk `dir`, adding every regular file keyed by its path
/// relative to `root`. Deterministic order (sorted dir entries) so the
/// downstream fingerprint stays stable.
fn collect_dir(
    root: &Path,
    dir: &Path,
    seen: &mut BTreeMap<PathBuf, PathBuf>,
    out: &mut Vec<ResolvedAsset>,
) -> anyhow::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("whisker-asset: read dir `{}`: {e}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|e| {
            anyhow::anyhow!(
                "whisker-asset: read dir entry under `{}`: {e}",
                dir.display()
            )
        })?;
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_dir(root, &path, seen, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .expect("path is under root by construction")
                .to_path_buf();
            insert_asset(&path, rel, seen, out)?;
        }
        // Skip symlinks-to-nowhere / sockets / fifos silently — only
        // regular files are bundleable.
    }
    Ok(())
}

/// Read `abs` and record it under `rel`, rejecting a second source that
/// maps to the same `rel`.
fn insert_asset(
    abs: &Path,
    rel: PathBuf,
    seen: &mut BTreeMap<PathBuf, PathBuf>,
    out: &mut Vec<ResolvedAsset>,
) -> anyhow::Result<()> {
    if let Some(prior) = seen.get(&rel) {
        anyhow::bail!(
            "whisker-asset: two assets collide at `{}` — both `{}` and `{}` would bundle to \
             the same path. Rename one or restructure your `assets/` tree.",
            rel.display(),
            prior.display(),
            abs.display(),
        );
    }
    let bytes = std::fs::read(abs)
        .map_err(|e| anyhow::anyhow!("whisker-asset: read `{}`: {e}", abs.display()))?;
    seen.insert(rel.clone(), abs.to_path_buf());
    out.push(ResolvedAsset { rel, bytes });
    Ok(())
}

impl Plugin for WhiskerAsset {
    type Config = WhiskerAssetConfig;

    fn validate(&self, cfg: &WhiskerAssetConfig) -> anyhow::Result<()> {
        // Nothing declared → nothing to validate. (apply also no-ops.)
        if cfg.dirs.is_empty() && cfg.files.is_empty() {
            return Ok(());
        }
        // Reject `..` escapes up front — paths must stay under the app
        // crate root. (The renderer also validates the gen-relative
        // path, but catching it here gives a clearer message.)
        for p in cfg.dirs.iter().chain(cfg.files.iter()) {
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                anyhow::bail!(
                    "whisker-asset: declared path `{}` contains `..` — asset paths must be \
                     relative to the app crate root and may not escape it.",
                    p.display(),
                );
            }
        }
        Ok(())
    }

    fn apply(&self, ctx: &mut GenerateContext, cfg: &WhiskerAssetConfig) -> anyhow::Result<()> {
        if cfg.dirs.is_empty() && cfg.files.is_empty() {
            return Ok(());
        }

        let crate_root = ctx.app_crate_dir.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "whisker-asset: the engine did not supply the app crate dir, so paths like \
                 `c.dir(\"assets\")` can't be resolved. This is a Whisker bug — the plugin \
                 runtime must populate `GenerateContext::app_crate_dir`."
            )
        })?;

        let assets = Self::collect(cfg, &crate_root)?;
        if assets.is_empty() {
            // Declared roots existed but held no regular files. Not an
            // error — an empty `assets/` dir is a no-op.
            return Ok(());
        }

        // ----- Android: drop each file under app/src/main/assets/whisker/ ----
        if let Some(android) = ctx.android.as_mut() {
            let mut count = 0usize;
            for asset in &assets {
                let dest = Path::new(ANDROID_ASSETS_ROOT)
                    .join(ANDROID_NAMESPACE)
                    .join(&asset.rel);
                android
                    .extra_files
                    .insert(dest, FileEntry::binary(&asset.bytes));
                count += 1;
            }
            ctx.journal.record(
                WhiskerAssetConfig::NAME,
                Target::Android,
                "extra_files",
                Operation::ArrayPush { count },
            );
        }

        // ----- iOS: drop under whisker_assets/ + one folder reference --------
        if let Some(ios) = ctx.ios.as_mut() {
            let mut count = 0usize;
            for asset in &assets {
                let dest = Path::new(IOS_NAMESPACE).join(&asset.rel);
                ios.extra_files
                    .insert(dest, FileEntry::binary(&asset.bytes));
                count += 1;
            }
            ctx.journal.record(
                WhiskerAssetConfig::NAME,
                Target::Ios,
                "extra_files",
                Operation::ArrayPush { count },
            );

            // Register the whole `whisker_assets` directory as a single
            // folder reference so Xcode copies the tree into the bundle
            // preserving subdirectories (one op regardless of file
            // count — the folder ref covers everything beneath it).
            ios.pbxproj_ops.push(PbxprojOp::AddResourceFolder {
                path: PathBuf::from(IOS_NAMESPACE),
            });
            ctx.journal.record(
                WhiskerAssetConfig::NAME,
                Target::Ios,
                "pbxproj_ops",
                Operation::ArrayPush { count: 1 },
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use whisker_plugin::{AndroidProjectIr, IosProjectIr};

    // ----- Temp-dir + fixture helpers --------------------------------------

    fn unique_tempdir(label: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-asset-test-{label}-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn ctx_both(crate_root: &Path) -> GenerateContext {
        GenerateContext {
            ios: Some(IosProjectIr::default()),
            android: Some(AndroidProjectIr::default()),
            app_crate_dir: Some(crate_root.to_path_buf()),
            ..Default::default()
        }
    }

    // ----- Builder ---------------------------------------------------------

    #[test]
    fn dir_and_file_accumulate() {
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("assets").dir("more").file("branding/logo.png");
        assert_eq!(
            cfg.dirs,
            vec![PathBuf::from("assets"), PathBuf::from("more")]
        );
        assert_eq!(cfg.files, vec![PathBuf::from("branding/logo.png")]);
    }

    // ----- validate --------------------------------------------------------

    #[test]
    fn validate_rejects_missing_dir() {
        let root = unique_tempdir("vmissing-dir");
        let cfg = {
            let mut c = WhiskerAssetConfig::default();
            c.dir("assets");
            c
        };
        // validate itself doesn't touch disk; apply does. Verify the
        // missing-path error surfaces via apply (the real pipeline path).
        let mut ctx = ctx_both(&root);
        let err = WhiskerAsset.apply(&mut ctx, &cfg).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_rejects_missing_file() {
        let root = unique_tempdir("vmissing-file");
        let mut cfg = WhiskerAssetConfig::default();
        cfg.file("nope.png");
        let mut ctx = ctx_both(&root);
        let err = WhiskerAsset.apply(&mut ctx, &cfg).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_rejects_parent_traversal() {
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("../escape");
        let err = WhiskerAsset.validate(&cfg).unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
    }

    #[test]
    fn validate_rejects_collision() {
        let root = unique_tempdir("vcollide");
        // A dir holding `logo.png` AND a top-level file also named
        // `logo.png` both want `<ns>/logo.png`.
        write(&root, "assets/logo.png", b"a");
        write(&root, "branding/logo.png", b"b");
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("assets").file("branding/logo.png");
        let mut ctx = ctx_both(&root);
        let err = WhiskerAsset.apply(&mut ctx, &cfg).unwrap_err();
        assert!(err.to_string().contains("collide"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_config_contributes_nothing() {
        let root = unique_tempdir("empty");
        let mut ctx = ctx_both(&root);
        WhiskerAsset
            .apply(&mut ctx, &WhiskerAssetConfig::default())
            .unwrap();
        assert!(ctx.ios.unwrap().extra_files.is_empty());
        assert!(ctx.android.unwrap().extra_files.is_empty());
        assert!(ctx.journal.records.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // ----- apply -----------------------------------------------------------

    #[test]
    fn apply_bundles_dir_into_both_platforms() {
        let root = unique_tempdir("apply-dir");
        write(&root, "assets/images/logo.png", &[0x89, 0x50, 0x4e, 0x47]);
        write(&root, "assets/data/config.json", b"{\"k\":1}");
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("assets");
        let mut ctx = ctx_both(&root);
        WhiskerAsset.apply(&mut ctx, &cfg).unwrap();

        // Android: app/src/main/assets/whisker/<rel>
        let android = ctx.android.as_ref().unwrap();
        let a_logo = PathBuf::from("app/src/main/assets/whisker/images/logo.png");
        assert!(
            android.extra_files.contains_key(&a_logo),
            "android logo missing"
        );
        assert_eq!(
            android.extra_files[&a_logo].to_bytes().unwrap(),
            vec![0x89, 0x50, 0x4e, 0x47],
        );
        assert!(android.extra_files.contains_key(&PathBuf::from(
            "app/src/main/assets/whisker/data/config.json"
        )));

        // iOS: whisker_assets/<rel>
        let ios = ctx.ios.as_ref().unwrap();
        let i_logo = PathBuf::from("whisker_assets/images/logo.png");
        assert!(ios.extra_files.contains_key(&i_logo), "ios logo missing");
        assert_eq!(
            ios.extra_files[&i_logo].to_bytes().unwrap(),
            vec![0x89, 0x50, 0x4e, 0x47],
        );

        // iOS folder reference registered exactly once.
        let folder_refs: Vec<_> = ios
            .pbxproj_ops
            .iter()
            .filter(|op| {
                matches!(op, PbxprojOp::AddResourceFolder { path } if path == Path::new("whisker_assets"))
            })
            .collect();
        assert_eq!(
            folder_refs.len(),
            1,
            "expected exactly one folder reference"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_bundles_individual_file_by_basename() {
        let root = unique_tempdir("apply-file");
        write(&root, "branding/logo.png", b"img");
        let mut cfg = WhiskerAssetConfig::default();
        cfg.file("branding/logo.png");
        let mut ctx = ctx_both(&root);
        WhiskerAsset.apply(&mut ctx, &cfg).unwrap();

        // Bundles under basename, not the source subpath.
        assert!(
            ctx.ios
                .as_ref()
                .unwrap()
                .extra_files
                .contains_key(&PathBuf::from("whisker_assets/logo.png"))
        );
        assert!(
            ctx.android
                .as_ref()
                .unwrap()
                .extra_files
                .contains_key(&PathBuf::from("app/src/main/assets/whisker/logo.png"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_errors_without_app_crate_dir() {
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("assets");
        let mut ctx = GenerateContext {
            ios: Some(IosProjectIr::default()),
            android: Some(AndroidProjectIr::default()),
            app_crate_dir: None,
            ..Default::default()
        };
        let err = WhiskerAsset.apply(&mut ctx, &cfg).unwrap_err();
        assert!(err.to_string().contains("app crate dir"), "{err}");
    }

    #[test]
    fn apply_android_only_skips_ios() {
        let root = unique_tempdir("apply-android-only");
        write(&root, "assets/a.txt", b"x");
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("assets");
        let mut ctx = GenerateContext {
            android: Some(AndroidProjectIr::default()),
            app_crate_dir: Some(root.clone()),
            ..Default::default()
        };
        WhiskerAsset.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.android.as_ref().unwrap().extra_files.len(), 1);
        assert!(ctx.ios.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_empty_dir_is_a_noop() {
        let root = unique_tempdir("apply-empty-dir");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir("assets");
        let mut ctx = ctx_both(&root);
        WhiskerAsset.apply(&mut ctx, &cfg).unwrap();
        assert!(ctx.ios.unwrap().extra_files.is_empty());
        assert!(ctx.android.unwrap().extra_files.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
