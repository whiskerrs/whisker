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
use std::sync::Mutex;

use super::{
    build_jump_table, build_link_plan, load_captured_args, load_captured_linker_args,
    parse_symbol_table, run_link_plan, run_obj_plan, thin_build, validate_environment,
    CapturedLinkerInvocation, CapturedRustcInvocation, HotpatchModuleCache, LinkerOs, PatchPlan,
};

/// Single-slot in-session cache of the stub `.o` we synthesize for the
/// patch dylib. Most edits only change a function *body*; the set of
/// undefined symbols the resulting `.o` references doesn't move, and
/// the device's `aslr_reference` is fixed for the session — so the
/// stub bytes are identical to the previous patch's. Reusing them
/// saves the per-patch object-builder pass.
struct StubCache {
    /// FNV-1a hash of the sorted `needed` symbol list. Cheap, and
    /// good enough for an "is this the same set?" check — collisions
    /// would just mean rebuilding the stub once.
    needed_hash: u64,
    aslr_reference: u64,
    target_os: LinkerOs,
    bytes: Vec<u8>,
}

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
    stub_cache: Mutex<Option<StubCache>>,
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
            stub_cache: Mutex::new(None),
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
    ///
    /// `crate_key` is the **rustc-form** name of the crate that owns
    /// the change. `None` defaults to the user crate. Sub-crate
    /// patches (#103) pass the changed crate's name so the thin
    /// build picks up that crate's captured rustc args (a fresh `.o`
    /// of the changed sub-crate), then links it into a patch dylib
    /// using the user crate's linker invocation as template. The
    /// original user dylib already exports the sub-crate's symbols
    /// (rustc linked its rlib in fat-build time); subsecond's
    /// JumpTable redirects them onto the patch dylib's new bodies.
    pub async fn build_patch(
        &self,
        aslr_reference: u64,
        crate_key: Option<&str>,
    ) -> Result<PatchPlan> {
        let user_key = self.package.replace('-', "_");
        let crate_key = crate_key
            .map(str::to_owned)
            .unwrap_or_else(|| user_key.clone());
        let captured_rustc = self.captured_rustc_args.get(&crate_key).with_context(|| {
            format!(
                "no captured rustc invocation for crate `{crate_key}`; \
                 was the fat build run?",
            )
        })?;
        // When patching a sub-crate, also compile the user crate
        // alongside it. The user crate carries the
        // `whisker_aslr_anchor` / `whisker_app_main` / `whisker_tick`
        // symbols (emitted by `#[whisker::main]`) — without them,
        // `subsecond::apply_patch`'s `dlsym(patch, "whisker_aslr_anchor")`
        // returns NULL and the runtime computes a junk slide that
        // SIGBUSes on the next call into a patched function. Linking
        // the user crate's `.o` puts those symbols into the patch
        // dylib's `.dynsym` so subsecond's math works.
        let user_rustc = if crate_key != user_key {
            Some(self.captured_rustc_args.get(&user_key).with_context(|| {
                format!(
                    "sub-crate patch ({crate_key}) needs user crate `{user_key}` in the link \
                         (for the `whisker_aslr_anchor` symbol), but no captured rustc invocation \
                         is available for it",
                )
            })?)
        } else {
            None
        };

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

        // Stage 1: rustc the changed crate to a `.o` file.
        let obj_plan = thin_build::build_obj_plan(captured_rustc, &self.patch_out_dir);
        let object = run_obj_plan(&obj_plan, &self.rustc_path, &self.cwd)
            .await
            .context("rustc --emit=obj for thin patch")?;

        // Stage 1b: for sub-crate patches, also rebuild the user
        // crate's `.o` so the patch dylib carries the anchor symbols
        // subsecond needs (see Stage 1's `user_rustc` doc above).
        // Skipped on user-crate patches because `object` already
        // covers it.
        let user_object_for_link: Option<PathBuf> = if let Some(user_rustc) = user_rustc {
            let user_obj_plan = thin_build::build_obj_plan(user_rustc, &self.patch_out_dir);
            let obj = run_obj_plan(&user_obj_plan, &self.rustc_path, &self.cwd)
                .await
                .context("rustc --emit=obj for user crate (sub-crate patch anchor source)")?;
            Some(obj)
        } else {
            None
        };

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
            // Test-fixture path: even without a stub we still need to
            // pass the user crate's `.o` for sub-crate patches so
            // the anchor symbol exists.
            user_object_for_link.iter().cloned().collect()
        } else {
            let stub_path = self.patch_out_dir.join("aslr-stub.o");
            // Feed BOTH input objects into the UND-symbol scan when
            // building the stub. The sub-crate `.o` references
            // whisker / kit symbols; the user crate `.o` references
            // sub-crate symbols + framework symbols. Missing the
            // latter set would leave UNDs unresolved at link time.
            let stub_bytes = self
                .stub_bytes_for_objects(&object, user_object_for_link.as_deref(), aslr_reference)
                .context("synthesize stub object")?;
            std::fs::write(&stub_path, &stub_bytes)
                .with_context(|| format!("write stub object to {}", stub_path.display()))?;
            let mut e = vec![stub_path];
            // Pull in the user crate's `.o` alongside the sub-crate's
            // (no-op for user-crate patches where this is None).
            if let Some(uo) = user_object_for_link.as_ref() {
                e.push(uo.clone());
            }
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
        // Required exports for the patch dylib's `.dynsym`. See
        // `build_link_plan` doc for the `whisker_aslr_anchor`
        // rationale (subsecond panics on `dlsym(patch, ...).unwrap()`
        // if it's missing). Only meaningful on Mach-O; the Linux
        // / Android branch of build_link_plan ignores
        // `extra_exports`.
        let extra_exports: &[&str] = match self.target_os {
            LinkerOs::Macos => &["_whisker_aslr_anchor", "_whisker_app_main", "_whisker_tick"],
            _ => &[],
        };
        let link_plan = build_link_plan(
            &captured_linker.args,
            &object,
            &output_dylib,
            self.target_os,
            &extras,
            extra_exports,
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

    /// Return the stub object bytes for the link inputs +
    /// `aslr_reference`. Reuses an in-session cached copy when the
    /// patch's UND symbol set matches the previous build's and
    /// `aslr_reference` / `target_os` are unchanged — the common case
    /// when an edit only touches a function body.
    ///
    /// Pass `extra` for sub-crate patches (#103): the union of UND
    /// symbols across both objects, minus their union of defined,
    /// gives the right "still unresolved" set.
    fn stub_bytes_for_objects(
        &self,
        object: &Path,
        extra: Option<&Path>,
        aslr_reference: u64,
    ) -> Result<Vec<u8>> {
        let mut paths: Vec<&Path> = vec![object];
        if let Some(p) = extra {
            paths.push(p);
        }
        let needed =
            super::compute_needed_symbols_multi(&paths).context("compute_needed_symbols_multi")?;
        let needed_hash = hash_needed(&needed);
        if let Ok(guard) = self.stub_cache.lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.needed_hash == needed_hash
                    && cached.aslr_reference == aslr_reference
                    && cached.target_os == self.target_os
                {
                    return Ok(cached.bytes.clone());
                }
            }
        }
        let bytes = super::build_stub_for_needed(
            &needed,
            &self.original_cache,
            self.target_os,
            aslr_reference,
        )
        .context("build_stub_for_needed")?;
        if let Ok(mut guard) = self.stub_cache.lock() {
            *guard = Some(StubCache {
                needed_hash,
                aslr_reference,
                target_os: self.target_os,
                bytes: bytes.clone(),
            });
        }
        Ok(bytes)
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

/// Hash a sorted symbol-name list into a u64 for the stub cache key.
/// FNV-1a — small, fast, and we only need "did this set change?"
/// granularity (a hash collision just means we rebuild the stub
/// once, no correctness impact).
fn hash_needed(needed: &[String]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h = FNV_OFFSET;
    for name in needed {
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        // Separator so ["ab","c"] and ["a","bc"] hash differently.
        h ^= 0xff;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
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
        let err = p.build_patch(0, None).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no captured rustc invocation"), "{msg}");
    }

    #[tokio::test]
    async fn build_patch_errors_when_explicit_crate_key_missing() {
        let p = Patcher::new(
            "demo".into(),
            "/rustc".into(),
            "/cc".into(),
            "/cwd".into(),
            "/patches".into(),
            LinkerOs::Macos,
            empty_cache(),
            HashMap::new(),
            HashMap::new(),
        );
        let err = p.build_patch(0, Some("not_a_crate")).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("not_a_crate"), "{msg}");
    }
}
