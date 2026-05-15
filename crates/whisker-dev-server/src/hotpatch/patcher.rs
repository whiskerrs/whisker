//! `Patcher` — the integrator. Turns a [`crate::Change`] into a
//! [`subsecond_types::JumpTable`] (wrapped in [`PatchPlan`]) by
//! stitching together the pieces from I4g-1 through I4g-X2:
//!
//!   - captured rustc args + linker args from the fat build
//!     (`wrapper`, `whisker-rustc-shim`, `whisker-linker-shim`)
//!   - rustc `--emit=obj` + own linker invoke (`thin_build`,
//!     `link_plan`, `runner::thin_rebuild_obj`)
//!   - parse the resulting patch dylib (`symbol_table`)
//!   - diff against the cached original (`HotpatchModuleCache` +
//!     `build_jump_table`)
//!
//! Two constructors:
//!
//! - [`Patcher::new`] takes already-loaded state. Tests use this
//!   to build the captured maps and the original-binary cache by
//!   hand, so they never need to actually run a real fat build.
//! - [`Patcher::initialize`] is the production path: spawn a fat
//!   build with both shims active, load both captures, parse the
//!   original binary, then call `new`.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{
    build_jump_table, build_link_plan, load_captured_args, load_captured_linker_args,
    parse_symbol_table, run_link_plan, run_obj_plan, thin_build, validate_environment,
    CapturedLinkerInvocation, CapturedRustcInvocation, HotpatchModuleCache, LinkerOs, PatchPlan,
};

pub struct Patcher {
    package: String,
    rustc_path: PathBuf,
    linker_path: PathBuf,
    cwd: PathBuf,
    patch_out_dir: PathBuf,
    target_os: LinkerOs,
    original_cache: HotpatchModuleCache,
    captured_rustc_args: HashMap<String, CapturedRustcInvocation>,
    captured_linker_args: HashMap<String, CapturedLinkerInvocation>,
}

impl Patcher {
    /// Direct constructor. Tests use this to inject hand-built
    /// state (so they don't have to run a real `cargo build` or
    /// touch the workspace).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        package: String,
        rustc_path: PathBuf,
        linker_path: PathBuf,
        cwd: PathBuf,
        patch_out_dir: PathBuf,
        target_os: LinkerOs,
        original_cache: HotpatchModuleCache,
        captured_rustc_args: HashMap<String, CapturedRustcInvocation>,
        captured_linker_args: HashMap<String, CapturedLinkerInvocation>,
    ) -> Self {
        Self {
            package,
            rustc_path,
            linker_path,
            cwd,
            patch_out_dir,
            target_os,
            original_cache,
            captured_rustc_args,
            captured_linker_args,
        }
    }

    /// Production setup. **Fat build already done** — the dev loop
    /// runs it through Builder::with_capture, so this constructor
    /// only needs to read the resulting caches and parse the
    /// original binary. Splitting the build out lets the dev loop
    /// reuse its existing initial-build phase rather than spawning
    /// cargo a second time.
    ///
    /// `original_binary` is the file the device actually loaded —
    /// for Android that's `lib<crate>.so` extracted from the APK or
    /// found under the Gradle-built jniLibs tree.
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        workspace_root: &Path,
        package: String,
        rustc_cache_dir: &Path,
        linker_cache_dir: &Path,
        real_linker: &Path,
        original_binary: &Path,
        target_os: LinkerOs,
    ) -> Result<Self> {
        let captured_rustc_args = load_captured_args(rustc_cache_dir)
            .with_context(|| format!("load rustc cache {}", rustc_cache_dir.display()))?;
        let captured_linker_args = load_captured_linker_args(linker_cache_dir)
            .with_context(|| format!("load linker cache {}", linker_cache_dir.display()))?;
        let original_cache = HotpatchModuleCache::from_path(original_binary)
            .with_context(|| format!("parse original binary {}", original_binary.display()))?;
        let patch_out_dir = workspace_root.join("target/.whisker/patches");
        let rustc_path = current_rustc();
        Ok(Self::new(
            package,
            rustc_path,
            real_linker.to_path_buf(),
            workspace_root.to_path_buf(),
            patch_out_dir,
            target_os,
            original_cache,
            captured_rustc_args,
            captured_linker_args,
        ))
    }

    /// Build a single hot-patch from a change. Returns the diff
    /// alongside the JumpTable so the dev loop can log warnings
    /// (added / removed / weak symbols).
    ///
    /// `aslr_reference` is the runtime address of `main` reported by
    /// the connected device through the `hello` WebSocket handshake.
    /// We compute the ASLR slide as
    /// `aslr_reference - cache.aslr_reference` and bake the result
    /// into a small stub object that resolves every host symbol the
    /// patch references — see `stub_object` for the rationale. Pass
    /// `0` for cases where no device has connected yet (the patch
    /// will still build but won't dispatch correctly at runtime; the
    /// caller should refrain from sending it in that state).
    pub async fn build_patch(&self, aslr_reference: u64) -> Result<PatchPlan> {
        // Look up the captured rustc invocation by the rustc-style
        // crate name (hyphens → underscores). Tier 1 only patches
        // the user crate today; tracking edits in dependency crates
        // is a future expansion.
        let crate_key = self.package.replace('-', "_");
        let captured_rustc = self.captured_rustc_args.get(&crate_key).with_context(|| {
            format!(
                "no captured rustc invocation for crate `{}`; was the fat build run?",
                self.package,
            )
        })?;

        // Linker capture is keyed by output basename. The fat build's
        // crate-type is whatever cargo chose (typically `cdylib` for
        // a Whisker user crate, sometimes `bin` + dylib for examples).
        // Try the most-likely names in order.
        let captured_linker = self.lookup_captured_linker().with_context(|| {
            format!(
                "no captured linker invocation for `{}`; was the fat build run with linker capture?",
                self.package,
            )
        })?;

        validate_environment(captured_rustc, &self.rustc_path)
            .context("environment validation before thin rebuild")?;

        // Stage 1: rustc the user crate to a single `.o` file.
        let obj_plan = thin_build::build_obj_plan(captured_rustc, &self.patch_out_dir);
        let object = run_obj_plan(&obj_plan, &self.rustc_path, &self.cwd)
            .await
            .context("rustc --emit=obj for thin patch")?;

        // Stage 2: synthesize a stub `.o` that maps every host symbol
        // the patch refers to onto its live runtime address. The stub
        // path lives next to the rebuilt object so cleanup is "delete
        // the patch_out_dir" and we don't have to track it separately.
        //
        // `aslr_reference == 0` is the "no device reported its base
        // yet" / test-fixture path. In that case the host's
        // `dynamic_lookup` (macOS) or `--unresolved-symbols=ignore-all`
        // (Linux) satisfies the patch's references against the
        // already-loaded test process — same as before Option B. Real
        // device dispatch always goes through the stub branch since
        // `lib.rs::run` skips Tier 1 entirely when no aslr_reference
        // has been reported.
        let extras: Vec<PathBuf> = if aslr_reference == 0 {
            Vec::new()
        } else {
            let stub_path = self.patch_out_dir.join("aslr-stub.o");
            let stub_bytes = super::create_undefined_symbol_stub(
                &self.original_cache,
                &object,
                self.target_os,
                aslr_reference,
            )
            .context("create_undefined_symbol_stub")?;
            std::fs::write(&stub_path, stub_bytes)
                .with_context(|| format!("write stub object to {}", stub_path.display()))?;
            let mut e = vec![stub_path];
            // Belt-and-suspenders on Linux/Android: the stub is
            // Text-only and emits weak symbols, so non-Text host
            // refs (thread-locals, static OnceCells like
            // `whisker_runtime::signal::ARENA`, `__data_start` style
            // markers) and any Text whose name didn't survive
            // `.llvm.X` ThinLTO normalization still need a fallback.
            // Linking the host `.so` here adds a `DT_NEEDED` entry,
            // so the Android dynamic linker fills them in at
            // `dlopen` time — but only when the stub couldn't (the
            // weak Text stubs lose to strong host defs, which is
            // what we want for `_Unwind_Resume`, `whisker_bridge_*`,
            // etc.).
            if matches!(self.target_os, LinkerOs::Linux) {
                e.push(self.original_cache.lib.clone());
            }
            e
        };

        // Stage 3: link the `.o` (+ optional stub `.o`) into a patch dylib.
        let output_dylib = self.expected_patch_path();
        let link_plan = build_link_plan(
            &captured_linker.args,
            &object,
            &output_dylib,
            self.target_os,
            &extras,
        );
        let new_dylib = run_link_plan(&link_plan, &self.linker_path, &self.cwd)
            .await
            .context("link patch dylib (object + stub)")?;

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
    /// `<patch_out_dir>/lib<crate>.{so,dylib,dll}`. The filename is
    /// chosen for the *target* OS (e.g. Android's `.so` even when the
    /// dev session runs on macOS) so the on-device runtime can
    /// recognise it.
    pub fn expected_patch_path(&self) -> PathBuf {
        self.patch_out_dir.join(thin_build::library_filename_for_os(
            &self.package,
            self.target_os,
        ))
    }

    /// Resolve the captured linker invocation that produced this
    /// crate's library. The key is the basename of the captured
    /// `-o`; for a typical cargo build the file is something like
    /// `lib<crate>-<hash>.dylib`, so we match by the `lib<crate>`
    /// prefix and the right extension. If multiple match (e.g.
    /// rebuilds across cargo cache states), the most-recent
    /// timestamp wins.
    fn lookup_captured_linker(&self) -> Option<&CapturedLinkerInvocation> {
        let stem_lib = format!("lib{}", self.package.replace('-', "_"));
        let stem_bin = self.package.replace('-', "_");
        let exts: &[&str] = match self.target_os {
            LinkerOs::Macos => &[".dylib"],
            LinkerOs::Linux => &[".so"],
            LinkerOs::Other => &[".dll"],
        };
        let mut best: Option<&CapturedLinkerInvocation> = None;
        for inv in self.captured_linker_args.values() {
            let Some(out) = inv.output.as_deref() else {
                continue;
            };
            let Some(name) = Path::new(out).file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let matches_ext = exts.iter().any(|ext| name.ends_with(ext));
            if !matches_ext {
                continue;
            }
            // `lib<crate>` (Unix shared) or `<crate>` (Windows DLL or
            // Apple bin output) — both are valid stems for the user
            // crate's link output.
            let matches_stem = name.starts_with(&stem_lib) || name.starts_with(&stem_bin);
            if !matches_stem {
                continue;
            }
            best = match best {
                Some(prev) if prev.timestamp_micros >= inv.timestamp_micros => Some(prev),
                _ => Some(inv),
            };
        }
        best
    }
}

/// Current rustc (matches cargo's default resolution): `RUSTC` env
/// wins, otherwise `rustc` on PATH.
fn current_rustc() -> PathBuf {
    PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
}

/// Return the static virtual address of `whisker_aslr_anchor` in
/// `path` (Mach-O's underscore-prefixed `_whisker_aslr_anchor` also
/// accepted). This goes into `JumpTable::new_base_address`; our
/// vendored subsecond's `apply_patch` then computes
///
/// ```ignore
/// new_offset = dlsym(patch, "whisker_aslr_anchor")  // runtime
///            - table.new_base_address               // static
///            = patch image base.
/// ```
///
/// Using `relative_address_base()` here (always 0 for an ELF PIE
/// dylib) sent `new_offset = patch_runtime_anchor`, leaving the
/// JumpTable's values shifted by the runtime address of the anchor
/// rather than by the image base — every patched function would land
/// somewhere meaningless. Symmetric to the host-side anchor lookup
/// in [`crate::hotpatch::cache::HotpatchModuleCache::from_path`].
fn read_image_base(path: &Path) -> Result<u64> {
    let table = parse_symbol_table(path).with_context(|| format!("parse {}", path.display()))?;
    // Same fallback semantics as the host cache: 0 when the anchor
    // symbol is absent. Lets test fixtures that don't carry
    // `#[whisker::main]` still build a patch plan; only the runtime
    // `apply_patch` math gets skewed.
    Ok(table
        .by_name
        .get("whisker_aslr_anchor")
        .or_else(|| table.by_name.get("_whisker_aslr_anchor"))
        .map(|s| s.address)
        .unwrap_or(0))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotpatch::SymbolTable;

    fn empty_cache() -> HotpatchModuleCache {
        HotpatchModuleCache {
            lib: PathBuf::from("/orig.dylib"),
            symbols: SymbolTable::default(),
            aslr_reference: 0x1_0000_0000,
        }
    }

    fn linker_inv(output: &str, ts: u128) -> CapturedLinkerInvocation {
        CapturedLinkerInvocation {
            output: Some(output.into()),
            args: vec!["-shared".into()],
            timestamp_micros: ts,
        }
    }

    #[test]
    fn new_holds_onto_its_inputs() {
        let p = Patcher::new(
            "demo".into(),
            PathBuf::from("/usr/local/bin/rustc"),
            PathBuf::from("/usr/bin/clang"),
            PathBuf::from("/tmp/cwd"),
            PathBuf::from("/tmp/patches"),
            LinkerOs::Macos,
            empty_cache(),
            HashMap::new(),
            HashMap::new(),
        );
        assert_eq!(p.package, "demo");
        assert_eq!(
            p.expected_patch_path(),
            PathBuf::from("/tmp/patches")
                .join(thin_build::library_filename_for_os("demo", LinkerOs::Macos,)),
        );
    }

    // ----- lookup_captured_linker --------------------------------------

    fn patcher_with_linker_map(
        target_os: LinkerOs,
        package: &str,
        linker: HashMap<String, CapturedLinkerInvocation>,
    ) -> Patcher {
        Patcher::new(
            package.into(),
            "/rustc".into(),
            "/cc".into(),
            "/cwd".into(),
            "/patches".into(),
            target_os,
            empty_cache(),
            HashMap::new(),
            linker,
        )
    }

    #[test]
    fn lookup_finds_macos_dylib_with_lib_prefix() {
        let mut m = HashMap::new();
        m.insert(
            "libdemo-abc123.dylib".into(),
            linker_inv("/cargo/target/debug/deps/libdemo-abc123.dylib", 100),
        );
        let p = patcher_with_linker_map(LinkerOs::Macos, "demo", m);
        let inv = p.lookup_captured_linker().expect("found");
        assert_eq!(inv.timestamp_micros, 100);
    }

    #[test]
    fn lookup_finds_linux_so_with_underscored_crate_name() {
        let mut m = HashMap::new();
        m.insert(
            "libhello_world.so".into(),
            linker_inv("/cargo/target/debug/deps/libhello_world.so", 50),
        );
        let p = patcher_with_linker_map(LinkerOs::Linux, "hello-world", m);
        let inv = p.lookup_captured_linker().expect("found");
        assert_eq!(inv.timestamp_micros, 50);
    }

    #[test]
    fn lookup_returns_most_recent_when_multiple_match() {
        let mut m = HashMap::new();
        m.insert(
            "libdemo.dylib".into(),
            linker_inv("/path/libdemo.dylib", 100),
        );
        m.insert(
            "libdemo-abc.dylib".into(),
            linker_inv("/path/libdemo-abc.dylib", 200),
        );
        let p = patcher_with_linker_map(LinkerOs::Macos, "demo", m);
        let inv = p.lookup_captured_linker().expect("found");
        assert_eq!(inv.timestamp_micros, 200);
    }

    #[test]
    fn lookup_returns_none_when_no_extension_matches() {
        let mut m = HashMap::new();
        m.insert("libdemo.so".into(), linker_inv("/path/libdemo.so", 100));
        // Looking for macOS .dylib in a map of .so → no match.
        let p = patcher_with_linker_map(LinkerOs::Macos, "demo", m);
        assert!(p.lookup_captured_linker().is_none());
    }

    #[test]
    fn lookup_returns_none_when_crate_name_doesnt_match() {
        let mut m = HashMap::new();
        m.insert(
            "libother.dylib".into(),
            linker_inv("/path/libother.dylib", 100),
        );
        let p = patcher_with_linker_map(LinkerOs::Macos, "demo", m);
        assert!(p.lookup_captured_linker().is_none());
    }

    #[tokio::test]
    async fn build_patch_errors_when_captured_rustc_args_missing() {
        let p = Patcher::new(
            "package-not-in-cache".into(),
            "/rustc".into(),
            "/cc".into(),
            "/cwd".into(),
            "/patches".into(),
            LinkerOs::Macos,
            empty_cache(),
            HashMap::new(), // empty rustc map
            HashMap::new(),
        );
        // aslr_reference value is irrelevant for this error path —
        // build_patch bails before touching it.
        let err = p.build_patch(0).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no captured rustc invocation"), "{msg}");
    }
}
