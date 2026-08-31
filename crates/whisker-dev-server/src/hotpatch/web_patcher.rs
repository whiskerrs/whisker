//! State-preserving WebAssembly patch compiler.
//!
//! Cargo/rustc remain the dependency resolver and compiler. This adapter
//! replays the captured rustc invocation for the changed crate as PIC, links
//! the resulting object as a wasm side module, then delegates relocation and
//! indirect-table mapping to [`super::wasm_patch`].

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{
    CapturedLinkerInvocation, CapturedRustcInvocation, PatchPlan, WasmHotpatchModuleCache,
    create_wasm_jump_table, load_captured_args, load_captured_linker_args, run_obj_plan,
    thin_build,
};

pub struct WebPatcher {
    package: String,
    rustc_path: PathBuf,
    linker_path: PathBuf,
    cwd: PathBuf,
    patch_out_dir: PathBuf,
    original_cache: WasmHotpatchModuleCache,
    rustc_invocations: HashMap<String, CapturedRustcInvocation>,
    linker_invocations: HashMap<String, CapturedLinkerInvocation>,
}

impl WebPatcher {
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        workspace_root: &Path,
        package: String,
        rustc_cache_dir: &Path,
        linker_cache_dir: &Path,
        linker_path: &Path,
        base_wasm: &Path,
    ) -> Result<Self> {
        let rustc_invocations = load_captured_args(rustc_cache_dir, Some("wasm32-unknown-unknown"))
            .with_context(|| format!("load {}", rustc_cache_dir.display()))?;
        let linker_invocations = load_captured_linker_args(linker_cache_dir)
            .with_context(|| format!("load {}", linker_cache_dir.display()))?;
        let original_cache = WasmHotpatchModuleCache::new(
            base_wasm,
            &"wasm32-unknown-unknown"
                .parse()
                .expect("static target triple is valid"),
        )
        .with_context(|| format!("parse Web base module {}", base_wasm.display()))?;
        Ok(Self {
            package,
            rustc_path: current_rustc(),
            linker_path: linker_path.to_path_buf(),
            cwd: workspace_root.to_path_buf(),
            patch_out_dir: workspace_root.join("target/.whisker/web-patches"),
            original_cache,
            rustc_invocations,
            linker_invocations,
        })
    }

    pub async fn build_patch(&self, crate_key: Option<&str>) -> Result<PatchPlan> {
        let key = crate_key
            .map(str::to_owned)
            .unwrap_or_else(|| self.package.replace('-', "_"));
        let captured = self.rustc_invocations.get(&key).with_context(|| {
            format!("no captured wasm rustc invocation for changed crate `{key}`")
        })?;
        let mut object_plan = thin_build::build_obj_plan(captured, &self.patch_out_dir);
        set_pic(&mut object_plan.args);
        let object = run_obj_plan(&object_plan, &self.rustc_path, &self.cwd)
            .await
            .context("compile Web Hot Reload object")?;

        let patch = self.patch_out_dir.join(format!("{key}.patch.wasm"));
        let linker_args = self
            .linker_invocations
            .values()
            .max_by_key(|invocation| invocation.timestamp_micros)
            .context("no captured wasm linker invocation")?;
        let args = side_module_args(&linker_args.args, &object, &patch);
        std::fs::create_dir_all(&self.patch_out_dir)
            .with_context(|| format!("create {}", self.patch_out_dir.display()))?;
        let output = tokio::process::Command::new(&self.linker_path)
            .args(&args)
            .current_dir(&self.cwd)
            .output()
            .await
            .with_context(|| format!("spawn {}", self.linker_path.display()))?;
        if !output.status.success() {
            anyhow::bail!(
                "wasm-ld side-module link failed ({})\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let table = create_wasm_jump_table(&patch, &self.original_cache)
            .context("construct WebAssembly jump table")?;
        Ok(PatchPlan {
            table,
            report: Default::default(),
        })
    }
}

fn set_pic(args: &mut Vec<String>) {
    args.retain(|argument| !argument.starts_with("-Crelocation-model="));
    args.push("-Crelocation-model=pic".into());
}

fn side_module_args(original: &[String], object: &Path, output: &Path) -> Vec<String> {
    let mut args = vec![
        "-flavor".into(),
        "wasm".into(),
        "--fatal-warnings".into(),
        "--import-memory".into(),
        "--import-table".into(),
        "--growable-table".into(),
        "--allow-undefined".into(),
        // The changed crate is normally an rlib, not the generated Web entry
        // binary. Retain its address-taken hot functions even though there is
        // no `main` root in this side link; the jump-table transformer needs
        // their element segment to map old and new function pointers.
        "--no-gc-sections".into(),
        "--no-demangle".into(),
        "--no-entry".into(),
        "--pie".into(),
        "--experimental-pic".into(),
    ];
    // Preserve only explicit exports that the thin object itself defines.
    // wasm-ld otherwise receives the fat build's object/archive graph.
    for pair in original.windows(2) {
        if pair[0] == "--export" && !pair[1].starts_with("__wbindgen") {
            args.extend([pair[0].clone(), pair[1].clone()]);
        }
    }
    args.push(object.display().to_string());
    args.push("-o".into());
    args.push(output.display().to_string());
    args
}

fn current_rustc() -> PathBuf {
    std::env::var_os("RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_module_plan_drops_fat_inputs_and_keeps_output_explicit() {
        let args = side_module_args(
            &["old.o".into(), "--export".into(), "start".into()],
            Path::new("new.o"),
            Path::new("patch.wasm"),
        );
        assert!(!args.iter().any(|arg| arg == "old.o"));
        assert!(args.windows(2).any(|pair| pair == ["-o", "patch.wasm"]));
        assert!(args.iter().any(|arg| arg == "--no-gc-sections"));
        assert!(!args.iter().any(|arg| arg == "-pie"));
        assert!(args.iter().any(|arg| arg == "--pie"));
    }
}
