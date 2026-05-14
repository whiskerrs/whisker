//! `Patcher` — the integrator. Turns a [`crate::Change`] into a
//! [`subsecond_types::JumpTable`] (wrapped in [`PatchPlan`]) by
//! stitching together the pieces from I4g-1 through I4g-5.
//!
//! Two constructors:
//!
//! - [`Patcher::new`] takes already-loaded state. Tests use this
//!   to build the captured-args map and the original-binary cache
//!   by hand, so they never need to actually run a real fat build.
//! - [`Patcher::initialize`] is the production path: spawn a fat
//!   build, load the captured args, parse the original binary,
//!   then call `new`.

use anyhow::{Context, Result};
use object::Object;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Target;

use super::{
    build_jump_table, build_thin_rebuild_plan, library_filename,
    load_captured_args, parse_symbol_table, run_fat_build, thin_rebuild,
    validate_environment, CapturedRustcInvocation, HotpatchModuleCache, PatchPlan,
};

pub struct Patcher {
    package: String,
    rustc_path: PathBuf,
    cwd: PathBuf,
    patch_out_dir: PathBuf,
    original_cache: HotpatchModuleCache,
    captured_args: HashMap<String, CapturedRustcInvocation>,
}

impl Patcher {
    /// Direct constructor. Tests use this to inject hand-built
    /// state (so they don't have to run a real `cargo build` or
    /// touch the workspace).
    pub fn new(
        package: String,
        rustc_path: PathBuf,
        cwd: PathBuf,
        patch_out_dir: PathBuf,
        original_cache: HotpatchModuleCache,
        captured_args: HashMap<String, CapturedRustcInvocation>,
    ) -> Self {
        Self {
            package,
            rustc_path,
            cwd,
            patch_out_dir,
            original_cache,
            captured_args,
        }
    }

    /// Production setup: run the fat build with the rustc shim,
    /// load the captured args, and parse the original binary.
    /// Heavy — touches cargo, the filesystem, and rustc — so we
    /// keep it out of unit tests; integration tests stick to
    /// [`Patcher::new`].
    pub fn initialize(
        workspace_root: &Path,
        package: String,
        target: Target,
        shim_path: &Path,
        original_binary: &Path,
    ) -> Result<Self> {
        let cache_dir = super::default_cache_dir(workspace_root);
        run_fat_build(workspace_root, &package, target, shim_path, &cache_dir)
            .context("fat build")?;
        let captured_args = load_captured_args(&cache_dir)?;
        let original_cache = HotpatchModuleCache::from_path(original_binary)?;
        let patch_out_dir = workspace_root.join("target/.tuft/patches");
        let rustc_path = current_rustc();
        Ok(Self::new(
            package,
            rustc_path,
            workspace_root.to_path_buf(),
            patch_out_dir,
            original_cache,
            captured_args,
        ))
    }

    /// Build a single hot-patch from a change. Returns the diff
    /// alongside the JumpTable so the dev loop can log warnings
    /// (added / removed / weak symbols).
    pub async fn build_patch(&self) -> Result<PatchPlan> {
        // Look up the captured invocation by the rustc-style crate
        // name (hyphens → underscores). Tier 1 only patches the user
        // crate today; tracking edits in dependency crates is a
        // future expansion.
        let key = self.package.replace('-', "_");
        let captured = self.captured_args.get(&key).with_context(|| {
            format!(
                "no captured rustc invocation for crate `{}`; was the fat build run?",
                self.package
            )
        })?;

        validate_environment(captured, &self.rustc_path)
            .context("environment validation before thin rebuild")?;

        let plan = build_thin_rebuild_plan(captured, &self.patch_out_dir);
        let new_dylib =
            thin_rebuild(&plan, &self.rustc_path, &self.cwd, &self.package)
                .await
                .context("thin rebuild")?;

        let new_symbols = parse_symbol_table(&new_dylib)
            .with_context(|| format!("parse {}", new_dylib.display()))?;
        let new_base_address = read_image_base(&new_dylib)?;

        Ok(build_jump_table(
            &self.original_cache.symbols,
            &new_symbols,
            new_dylib,
            self.original_cache.aslr_reference,
            new_base_address,
        ))
    }

    /// Where this Patcher would put the next patch dylib —
    /// `<workspace>/target/.tuft/patches/lib<crate>.{so,dylib,dll}`.
    /// Useful for the dev loop's `adb push` step.
    pub fn expected_patch_path(&self) -> PathBuf {
        self.patch_out_dir.join(library_filename(&self.package))
    }
}

/// Current rustc (matches cargo's default resolution): `RUSTC` env
/// wins, otherwise `rustc` on PATH.
fn current_rustc() -> PathBuf {
    PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
}

/// Lift `relative_address_base` out of an arbitrary binary on disk.
/// Used for new patch dylibs (we don't keep them in the cache; the
/// patch is throwaway).
fn read_image_base(path: &Path) -> Result<u64> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {}", path.display()))?;
    let file = object::File::parse(&*bytes)
        .with_context(|| format!("parse {} as object file", path.display()))?;
    Ok(file.relative_address_base())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotpatch::SymbolTable;

    #[test]
    fn new_holds_onto_its_inputs() {
        let cache = HotpatchModuleCache {
            lib: PathBuf::from("/orig.dylib"),
            symbols: SymbolTable::default(),
            aslr_reference: 0x1_0000_0000,
        };
        let p = Patcher::new(
            "demo".into(),
            PathBuf::from("/usr/local/bin/rustc"),
            PathBuf::from("/tmp/cwd"),
            PathBuf::from("/tmp/patches"),
            cache,
            HashMap::new(),
        );
        assert_eq!(p.package, "demo");
        assert_eq!(
            p.expected_patch_path(),
            PathBuf::from("/tmp/patches").join(library_filename("demo")),
        );
    }
}
