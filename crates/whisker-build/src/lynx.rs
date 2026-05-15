//! Lynx artifact fetcher.
//!
//! Whisker pins a specific Lynx fork release ([`whiskerrs/lynx`])
//! whose CI produces per-platform tarballs. This module downloads
//! them on demand, verifies SHA-256 against constants checked into
//! the repo, and unpacks them into a per-user cache:
//!
//! ```text
//! ~/.cache/whisker/lynx/<version>/
//! ├── android/
//! │   ├── LynxAndroid.aar
//! │   ├── LynxBase.aar
//! │   ├── LynxTrace.aar
//! │   ├── ServiceAPI.aar
//! │   ├── unpacked/jni/<abi>/*.so       (post-unpack of AARs)
//! │   └── headers/                       (C++ headers for cc-rs)
//! └── ios/
//!     ├── Lynx.xcframework/
//!     ├── LynxBase.xcframework/
//!     ├── LynxServiceAPI.xcframework/
//!     ├── PrimJS.xcframework/
//!     └── headers/
//! ```
//!
//! ## Local override
//!
//! Setting `WHISKER_LYNX_DIR=/abs/path` short-circuits the fetcher
//! entirely. The override path is treated as the per-version cache
//! root above (i.e. `WHISKER_LYNX_DIR/android/LynxAndroid.aar` must
//! exist when `ensure_lynx_android` is called). Useful when:
//!
//! - Bootstrapping the system before the fork's CI is set up
//!   (Phase 4c) — point at the existing `target/lynx-*` directories
//!   that `cargo xtask android build-lynx-aar` / `ios
//!   build-lynx-frameworks` produce.
//! - Whisker contributors patching Lynx locally and wanting to test
//!   without going through the GitHub release cycle.
//! - CI hosts that cache the artifacts elsewhere.
//!
//! ## SHA-256 verification policy
//!
//! Every download is verified against [`LYNX_ANDROID_SHA256`] /
//! [`LYNX_IOS_SHA256`]. An empty constant means "no release artifact
//! exists yet" — the fetcher refuses to download in that state and
//! demands `WHISKER_LYNX_DIR`. Once the fork's CI produces the first
//! tagged release, bump the version + paste in the real checksums.

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

// ----- Pinned version -------------------------------------------------------

/// Whisker fork tag the artifacts are published under
/// (`https://github.com/whiskerrs/lynx/releases/tag/<this>`).
///
/// Schema: `<lynx-upstream-version>-whisker.<patch-iteration>` so a
/// reader can tell at a glance which upstream Lynx is wrapped, and
/// our own patch iterations bump independently.
pub const LYNX_FORK_TAG: &str = "v3.7.0-whisker.0";

/// Version segment that appears in cache paths + tarball filenames.
/// Derived from [`LYNX_FORK_TAG`] minus the leading `v`.
pub const LYNX_VERSION: &str = "3.7.0-whisker.0";

/// SHA-256 of `whisker-lynx-android-<LYNX_VERSION>.tar.gz` as
/// produced by the fork's CI. Pinned to the
/// [v3.7.0-whisker.0 release](https://github.com/whiskerrs/lynx/releases/tag/v3.7.0-whisker.0).
pub const LYNX_ANDROID_SHA256: &str =
    "9bbaa9460afd3bd77fa2df87292cbe626e35fd8cc4eb21157bbe1572374b1eab";

/// SHA-256 of `whisker-lynx-ios-<LYNX_VERSION>.tar.gz`. Pinned to
/// the same release as Android — the fork's CI builds both halves
/// in parallel and uploads them as a pair.
pub const LYNX_IOS_SHA256: &str =
    "06a733c1b9bfef70cccabbb4502d0f02280633f792322e42e20ed4258cba6a54";

/// GitHub Releases URL template. The `<{ver}>` and `<{plat}>`
/// placeholders are filled by [`download_url`].
const URL_TEMPLATE: &str =
    "https://github.com/whiskerrs/lynx/releases/download/v{ver}/whisker-lynx-{plat}-{ver}.tar.gz";

// ----- Public API -----------------------------------------------------------

/// Platform the caller wants Lynx artifacts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LynxPlatform {
    Android,
    Ios,
}

impl LynxPlatform {
    fn as_str(self) -> &'static str {
        match self {
            LynxPlatform::Android => "android",
            LynxPlatform::Ios => "ios",
        }
    }

    fn expected_sha256(self) -> &'static str {
        match self {
            LynxPlatform::Android => LYNX_ANDROID_SHA256,
            LynxPlatform::Ios => LYNX_IOS_SHA256,
        }
    }
}

/// Ensure the Android Lynx artifacts are cached locally and return
/// the path to the per-platform cache subdir.
pub fn ensure_lynx_android() -> Result<PathBuf> {
    ensure_lynx(LynxPlatform::Android)
}

/// Ensure the iOS Lynx artifacts are cached locally and return the
/// path to the per-platform cache subdir.
pub fn ensure_lynx_ios() -> Result<PathBuf> {
    ensure_lynx(LynxPlatform::Ios)
}

/// Lower-level entry point — useful for the doctor subcommand which
/// wants to peek without forcing a download.
pub fn cache_dir(platform: LynxPlatform) -> Result<PathBuf> {
    Ok(cache_version_root()?.join(platform.as_str()))
}

/// Where the cache for the pinned version lives. Honours
/// [`WHISKER_LYNX_DIR`] override; otherwise resolves to
/// `$XDG_CACHE_HOME/whisker/lynx/<version>` or
/// `$HOME/.cache/whisker/lynx/<version>`.
pub fn cache_version_root() -> Result<PathBuf> {
    if let Some(override_dir) = std::env::var_os("WHISKER_LYNX_DIR") {
        return Ok(PathBuf::from(override_dir));
    }
    let root = if let Some(p) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(p).join("whisker/lynx")
    } else {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| anyhow!("HOME not set; cannot resolve Lynx cache dir"))?;
        PathBuf::from(home).join(".cache/whisker/lynx")
    };
    Ok(root.join(LYNX_VERSION))
}

// ----- Implementation -------------------------------------------------------

fn ensure_lynx(platform: LynxPlatform) -> Result<PathBuf> {
    let cache = cache_dir(platform)?;
    if is_cache_populated(&cache, platform) {
        return Ok(cache);
    }

    // No local cache — either we download (CI release exists) or we
    // require WHISKER_LYNX_DIR (bootstrap / contributor mode).
    if std::env::var_os("WHISKER_LYNX_DIR").is_some() {
        // Override was set but the contents weren't what we expected.
        // Don't silently fall through to downloading from the network
        // — the user explicitly asked for the local path, surface the
        // miss as an error so they can fix it.
        bail!(
            "WHISKER_LYNX_DIR is set but {} doesn't contain the expected {} artifacts. \
             Expected layout: {}/{}/{}",
            cache.display(),
            platform.as_str(),
            cache.parent().unwrap_or(&cache).display(),
            platform.as_str(),
            sentinel_filename(platform),
        );
    }

    let expected_sha = platform.expected_sha256();
    if expected_sha.is_empty() {
        bail!(
            "Lynx {} artifacts not available: no SHA-256 pinned in \
             whisker-build::lynx (fork CI release isn't published yet). \
             Set WHISKER_LYNX_DIR=/path/to/locally-built/artifacts to bootstrap.",
            platform.as_str(),
        );
    }

    let url = download_url(platform);
    eprintln!("[whisker-build::lynx] downloading {url}");
    let bytes = http_get(&url)
        .with_context(|| format!("download {url}"))?;
    verify_sha256(&bytes, expected_sha).with_context(|| {
        format!(
            "checksum mismatch for {url}; \
             release artifact differs from pinned whisker-build::lynx::{}_SHA256",
            platform.as_str().to_uppercase(),
        )
    })?;

    std::fs::create_dir_all(&cache)
        .with_context(|| format!("mkdir -p {}", cache.display()))?;
    extract_tar_gz(&bytes, &cache)
        .with_context(|| format!("extract tar.gz into {}", cache.display()))?;

    if !is_cache_populated(&cache, platform) {
        bail!(
            "tar.gz extracted but {} doesn't look populated — \
             check the release tarball layout",
            cache.display(),
        );
    }
    eprintln!(
        "[whisker-build::lynx] cached at {}",
        cache.display(),
    );
    Ok(cache)
}

fn download_url(platform: LynxPlatform) -> String {
    URL_TEMPLATE
        .replace("{ver}", LYNX_VERSION)
        .replace("{plat}", platform.as_str())
        .replace("v{ver}", LYNX_FORK_TAG)
}

/// Check the cache has at least one sentinel file we expect to find.
/// Cheap heuristic — saves repeated downloads, but a corrupt cache
/// can still slip through. Callers that detect missing inner files
/// later can `rm -rf` the cache and retry.
fn is_cache_populated(cache: &Path, platform: LynxPlatform) -> bool {
    cache.join(sentinel_filename(platform)).exists()
}

fn sentinel_filename(platform: LynxPlatform) -> &'static str {
    match platform {
        LynxPlatform::Android => "LynxAndroid.aar",
        LynxPlatform::Ios => "Lynx.xcframework",
    }
}

fn http_get(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url).call().context("HTTP request")?;
    let mut bytes: Vec<u8> = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .context("read response body")?;
    Ok(bytes)
}

fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<()> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let got = hasher.finalize();
    let got_hex = hex_lower(&got);
    if !got_hex.eq_ignore_ascii_case(expected_hex) {
        bail!("expected SHA-256 {expected_hex}, got {got_hex}");
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(hex_nibble(b >> 4));
        out.push(hex_nibble(b & 0x0f));
    }
    out
}

fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => unreachable!("nibble"),
    }
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<()> {
    // Release tarballs wrap their contents in a single
    // `whisker-lynx-<platform>-<ver>/` directory (the CI workflow
    // does `tar czf NAME -C $WORKSPACE $STAGE_DIR`). Strip that
    // top-level component so the unpacked layout matches what
    // `cache_dir(platform)` returns.
    //
    // If a future tarball is packed without the wrapper, this still
    // works — `strip_root_component` is a no-op on top-level files.
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries().context("read tar entries")? {
        let mut entry = entry.context("tar entry")?;
        let path = entry.path().context("entry path")?.into_owned();
        let stripped = strip_root_component(&path);
        if stripped.as_os_str().is_empty() {
            // top-level dir entry itself — no file to extract
            continue;
        }
        let target = dest.join(stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        entry
            .unpack(&target)
            .with_context(|| format!("unpack {}", target.display()))?;
    }
    Ok(())
}

/// Drop the first path component if there is one. `foo/bar/baz` →
/// `bar/baz`; `foo` → `` (empty). Used by [`extract_tar_gz`] to
/// flatten the wrapping dir release tarballs have.
fn strip_root_component(p: &Path) -> PathBuf {
    let mut it = p.components();
    it.next();
    it.collect()
}

// ----- Convenience: write the version line that the CI workflow
// ----- consumes when tagging a release. The Whisker CI side reads
// ----- this file to confirm the tag matches the pinned const.

/// Filename of the artifact tarball for `platform`. Useful for tests
/// and for CI scripts that need to construct the same name we
/// download from.
pub fn tarball_filename(platform: LynxPlatform) -> String {
    format!(
        "whisker-lynx-{plat}-{ver}.tar.gz",
        plat = platform.as_str(),
        ver = LYNX_VERSION,
    )
}

// ----- Workspace integration ------------------------------------------------
//
// Downstream consumers (whisker-driver-sys/build.rs, the CNG-rendered
// Android settings.gradle.kts, native/ios/Package.swift's
// `binaryTarget(path:)` lines) all reference paths under
// `<workspace>/target/lynx-{android,android-unpacked,ios,headers}/`.
// Rather than reshape every consumer to know about the user cache
// dir, we keep those canonical paths working by symlinking them onto
// the cache. The symlinks live under `target/` so `cargo clean`
// removes them; `link_into_workspace` is idempotent and rebuilds
// them on next invocation.

/// Create the per-workspace symlinks for `platform`'s Lynx
/// artifacts. Must be called after [`ensure_lynx_android`] /
/// [`ensure_lynx_ios`] for that platform.
///
/// Symlinks created:
///
/// | platform | symlink path | target |
/// |---|---|---|
/// | Android | `<ws>/target/lynx-android`          | `<cache>/<ver>/android`         |
/// | Android | `<ws>/target/lynx-android-unpacked` | `<cache>/<ver>/android/unpacked`|
/// | iOS     | `<ws>/target/lynx-ios`              | `<cache>/<ver>/ios`             |
/// | both    | `<ws>/target/lynx-headers`          | `<cache>/<ver>/<plat>/headers`  |
///
/// `lynx-headers` is created by whichever platform call runs first;
/// the headers are byte-identical between the two tarballs so a
/// second call is a no-op when the symlink already points at a valid
/// destination.
pub fn link_into_workspace(workspace_root: &Path, platform: LynxPlatform) -> Result<()> {
    let cache = cache_dir(platform)?;
    let target = workspace_root.join("target");
    std::fs::create_dir_all(&target)
        .with_context(|| format!("mkdir -p {}", target.display()))?;

    match platform {
        LynxPlatform::Android => {
            relink(&target.join("lynx-android"), &cache)?;
            relink(&target.join("lynx-android-unpacked"), &cache.join("unpacked"))?;
        }
        LynxPlatform::Ios => {
            relink(&target.join("lynx-ios"), &cache)?;
        }
    }
    let headers = cache.join("headers");
    if headers.is_dir() {
        relink(&target.join("lynx-headers"), &headers)?;
    }
    Ok(())
}

/// Create or refresh a symlink at `link` pointing to `target`.
/// Idempotent: if a correct symlink already exists, no-op. Replaces
/// stale symlinks pointing elsewhere. Refuses to clobber non-symlink
/// files / dirs (safety: those might be user data).
fn relink(link: &Path, target: &Path) -> Result<()> {
    match std::fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if let Ok(existing) = std::fs::read_link(link) {
                if existing == target {
                    return Ok(());
                }
            }
            std::fs::remove_file(link)
                .with_context(|| format!("remove stale symlink {}", link.display()))?;
        }
        Ok(_) => {
            bail!(
                "{} exists but isn't a symlink — refusing to clobber. \
                 If this is leftover from a pre-Phase-4 build, `rm -rf` it manually.",
                link.display(),
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("stat {}", link.display())),
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).with_context(|| {
            format!("symlink {} -> {}", link.display(), target.display())
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        bail!("symlink-into-workspace is only supported on Unix hosts");
    }
    Ok(())
}

// ----- A tiny "write a tarball, then extract it" helper so unit
// ----- tests don't depend on the network.

#[cfg(test)]
fn write_tar_gz_for_test(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut gz =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut tar = tar::Builder::new(&mut gz);
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, *contents).unwrap();
        }
        tar.finish().unwrap();
    }
    gz.finish().unwrap()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-build-lynx-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn download_url_replaces_placeholders() {
        let url = download_url(LynxPlatform::Android);
        assert!(url.contains(LYNX_VERSION));
        assert!(url.contains("android"));
        assert!(url.contains(LYNX_FORK_TAG));
        assert!(url.starts_with("https://github.com/whiskerrs/lynx/releases/download/"));
    }

    #[test]
    fn tarball_filename_matches_url_segment() {
        let name = tarball_filename(LynxPlatform::Ios);
        assert_eq!(name, format!("whisker-lynx-ios-{LYNX_VERSION}.tar.gz"));
        // The URL should embed exactly the same filename.
        assert!(download_url(LynxPlatform::Ios).ends_with(&name));
    }

    #[test]
    fn cache_dir_honours_override() {
        let tmp = unique_tempdir();
        // SAFETY: tests are single-threaded by default and we restore
        // the env in the same scope.
        unsafe {
            std::env::set_var("WHISKER_LYNX_DIR", &tmp);
        }
        let android = cache_dir(LynxPlatform::Android).unwrap();
        let ios = cache_dir(LynxPlatform::Ios).unwrap();
        assert_eq!(android, tmp.join("android"));
        assert_eq!(ios, tmp.join("ios"));
        unsafe {
            std::env::remove_var("WHISKER_LYNX_DIR");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cache_dir_uses_xdg_when_set() {
        let tmp = unique_tempdir();
        unsafe {
            std::env::remove_var("WHISKER_LYNX_DIR");
            std::env::set_var("XDG_CACHE_HOME", &tmp);
        }
        let p = cache_version_root().unwrap();
        assert!(p.starts_with(&tmp));
        assert!(p.ends_with(LYNX_VERSION));
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn verify_sha256_accepts_correct_hex() {
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let h = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_sha256(b"hello", h).expect("known hash");
    }

    #[test]
    fn verify_sha256_rejects_wrong_hex() {
        let err = verify_sha256(b"hello", "0".repeat(64).as_str()).unwrap_err();
        assert!(err.to_string().contains("SHA-256"), "got: {err:#}");
    }

    #[test]
    fn extract_tar_gz_strips_the_wrapping_root_dir() {
        // Mirrors the CI tarball layout: a single top-level dir
        // wrapping the actual contents.
        let bytes = write_tar_gz_for_test(&[
            ("whisker-lynx-android-x/LynxAndroid.aar", b"FAKE_AAR_BYTES"),
            ("whisker-lynx-android-x/headers/Lynx/foo.h", b"// stub"),
        ]);
        let dest = unique_tempdir();
        extract_tar_gz(&bytes, &dest).expect("extract");
        // The wrapping `whisker-lynx-android-x/` should be gone.
        assert!(dest.join("LynxAndroid.aar").is_file());
        assert!(dest.join("headers/Lynx/foo.h").is_file());
        assert_eq!(
            std::fs::read(dest.join("LynxAndroid.aar")).unwrap(),
            b"FAKE_AAR_BYTES",
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn strip_root_component_drops_first_segment() {
        assert_eq!(strip_root_component(Path::new("a/b/c")), PathBuf::from("b/c"));
        assert_eq!(strip_root_component(Path::new("only-root")), PathBuf::new());
        assert_eq!(strip_root_component(Path::new("a/")), PathBuf::new());
    }

    #[test]
    fn ensure_lynx_uses_override_when_populated() {
        let tmp = unique_tempdir();
        let android_dir = tmp.join("android");
        std::fs::create_dir_all(&android_dir).unwrap();
        std::fs::write(android_dir.join("LynxAndroid.aar"), b"x").unwrap();
        unsafe {
            std::env::set_var("WHISKER_LYNX_DIR", &tmp);
        }
        let resolved = ensure_lynx_android().expect("override path used");
        assert_eq!(resolved, android_dir);
        unsafe {
            std::env::remove_var("WHISKER_LYNX_DIR");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_lynx_errors_when_override_set_but_empty() {
        let tmp = unique_tempdir();
        unsafe {
            std::env::set_var("WHISKER_LYNX_DIR", &tmp);
        }
        let err = ensure_lynx_android().unwrap_err();
        assert!(
            err.to_string().contains("WHISKER_LYNX_DIR is set"),
            "got: {err:#}",
        );
        unsafe {
            std::env::remove_var("WHISKER_LYNX_DIR");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_lynx_errors_when_no_sha_pinned_and_no_override() {
        // Default state of the constants is "" — we can hit this
        // branch by ensuring neither override is set and the cache
        // dir is empty. Use a clean XDG_CACHE_HOME so we don't read
        // a populated user cache.
        let tmp = unique_tempdir();
        unsafe {
            std::env::remove_var("WHISKER_LYNX_DIR");
            std::env::set_var("XDG_CACHE_HOME", &tmp);
        }
        // Skip the test if the SHA constants are pinned to non-empty
        // (i.e. P4c has happened and a real release exists). In that
        // case this code path would actually go fetch from the net.
        if !LYNX_ANDROID_SHA256.is_empty() {
            unsafe {
                std::env::remove_var("XDG_CACHE_HOME");
            }
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }
        let err = ensure_lynx_android().unwrap_err();
        assert!(
            err.to_string().contains("not available"),
            "got: {err:#}",
        );
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // Avoid "function never used in non-test" lint when tests don't
    // exercise the helper.
    #[test]
    fn write_tar_gz_helper_is_callable() {
        let bytes = write_tar_gz_for_test(&[("a", b"b")]);
        let _: Vec<u8> = bytes;
    }

    #[cfg(unix)]
    #[test]
    fn relink_creates_symlink_when_absent() {
        let tmp = unique_tempdir();
        let target = tmp.join("dest");
        std::fs::create_dir_all(&target).unwrap();
        let link = tmp.join("link");
        relink(&link, &target).unwrap();
        let resolved = std::fs::read_link(&link).unwrap();
        assert_eq!(resolved, target);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn relink_replaces_stale_symlink() {
        let tmp = unique_tempdir();
        let stale_target = tmp.join("old");
        let new_target = tmp.join("new");
        std::fs::create_dir_all(&stale_target).unwrap();
        std::fs::create_dir_all(&new_target).unwrap();
        let link = tmp.join("link");
        std::os::unix::fs::symlink(&stale_target, &link).unwrap();

        relink(&link, &new_target).unwrap();
        let resolved = std::fs::read_link(&link).unwrap();
        assert_eq!(resolved, new_target);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn relink_is_idempotent_when_already_correct() {
        let tmp = unique_tempdir();
        let target = tmp.join("dest");
        std::fs::create_dir_all(&target).unwrap();
        let link = tmp.join("link");
        relink(&link, &target).unwrap();
        // Second call should be a no-op and still leave the link in
        // place.
        relink(&link, &target).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn relink_refuses_to_clobber_real_file() {
        let tmp = unique_tempdir();
        let link = tmp.join("link");
        std::fs::write(&link, b"important").unwrap();
        let target = tmp.join("dest");
        std::fs::create_dir_all(&target).unwrap();
        let err = relink(&link, &target).unwrap_err();
        assert!(err.to_string().contains("refusing to clobber"), "got: {err:#}");
        // File should still be there with its original contents.
        assert_eq!(std::fs::read(&link).unwrap(), b"important");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn link_into_workspace_lays_out_android_symlinks() {
        let tmp = unique_tempdir();
        let cache_root = tmp.join("cache-version");
        let android = cache_root.join("android");
        std::fs::create_dir_all(android.join("unpacked")).unwrap();
        std::fs::create_dir_all(android.join("headers")).unwrap();
        std::fs::write(android.join("LynxAndroid.aar"), b"").unwrap();
        unsafe {
            std::env::set_var("WHISKER_LYNX_DIR", &cache_root);
        }
        let workspace = tmp.join("ws");
        std::fs::create_dir_all(workspace.join("target")).unwrap();

        link_into_workspace(&workspace, LynxPlatform::Android).expect("link android");
        assert!(workspace.join("target/lynx-android").exists());
        assert!(workspace.join("target/lynx-android-unpacked").exists());
        assert!(workspace.join("target/lynx-headers").exists());

        unsafe {
            std::env::remove_var("WHISKER_LYNX_DIR");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
