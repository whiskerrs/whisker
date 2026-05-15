//! Tier 2 cold rebuild: produce a fresh artifact + (re)install it on
//! the active [`Target`].
//!
//! Delegates the cargo + gradle / xcodebuild orchestration to
//! `whisker-build`, which is shared with `whisker-cli`'s `whisker
//! build` subcommand. When `with_capture` is set, the cargo step
//! doubles as a **fat build** that fills the rustc + linker capture
//! caches the Tier 1 hot-patch pipeline replays later.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

use crate::Target;
use whisker_build::CaptureShims;

/// Builder for cold (Tier 2) rebuilds. Tier 1 hot-patches live in
/// [`crate::hotpatch::Patcher`] — Builder is only invoked for
/// dependency-shaped changes (Cargo.toml edits) and as a fallback
/// when Tier 1 errors.
pub struct Builder {
    workspace_root: PathBuf,
    /// User crate dir (= `Cargo.toml` parent). Needed to find
    /// `gen/android/` for gradle invocation.
    crate_dir: PathBuf,
    package: String,
    target: Target,
    /// Cargo features forwarded to whichever step compiles the user
    /// crate. The dev loop turns on `whisker/hot-reload` here.
    features: Vec<String>,
    /// `Some` → fat build (Tier 1 capture caches get populated).
    /// `None` → plain Tier 2.
    capture: Option<CaptureShims>,
}

impl Builder {
    pub fn new(
        workspace_root: PathBuf,
        crate_dir: PathBuf,
        package: String,
        target: Target,
    ) -> Self {
        Self {
            workspace_root,
            crate_dir,
            package,
            target,
            features: Vec::new(),
            capture: None,
        }
    }

    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }

    /// Elevate the next build into a fat build. The cache dirs and
    /// shim binaries from `capture` get folded into the cargo
    /// invocation via env vars — see
    /// [`whisker_build::capture_env_vars`] for the exact set.
    pub fn with_capture(mut self, capture: CaptureShims) -> Self {
        self.capture = Some(capture);
        self
    }

    /// Run the build for the current target. Inherits stdout/stderr.
    pub async fn build(&self) -> Result<()> {
        match self.target {
            Target::Host => self.build_host().await,
            Target::Android => self.build_android().await,
            Target::IosSimulator => self.build_ios_simulator().await,
        }
    }

    /// Whether this builder is configured for a fat build.
    pub fn captures_shims(&self) -> bool {
        self.capture.is_some()
    }

    // ----- per-target build paths ------------------------------------------

    async fn build_host(&self) -> Result<()> {
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "-p", &self.package]);
        for f in &self.features {
            cmd.args(["--features", f]);
        }
        cmd.current_dir(&self.workspace_root);
        let s = cmd.status().await.context("spawn cargo")?;
        if !s.success() {
            return Err(anyhow!("cargo build failed ({s})"));
        }
        Ok(())
    }

    async fn build_android(&self) -> Result<()> {
        // Dev loop uses a single ABI; the gen template's compileSdk
        // can differ from the NDK API level — the NDK 24 default has
        // worked across Whisker's supported devices.
        let abi = "arm64-v8a".to_string();
        let api: u32 = 24;
        let ws = self.workspace_root.clone();
        let crate_dir = self.crate_dir.clone();
        let pkg = self.package.clone();
        let features = self.features.clone();
        let capture = self.capture.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let tc = whisker_build::android::resolve_toolchain(&abi, api)?;
            let so = whisker_build::android::cargo_build_dylib(
                &whisker_build::android::CargoBuild {
                    workspace_root: &ws,
                    package: &pkg,
                    toolchain: &tc,
                    profile: whisker_build::Profile::Debug,
                    features: &features,
                    capture: capture.as_ref(),
                },
            )?;
            let gen_android = crate_dir.join("gen/android");
            whisker_build::android::stage_jni_libs(&gen_android, &abi, &so, &tc)?;
            whisker_build::android::run_gradle_assemble(
                &gen_android,
                whisker_build::Profile::Debug,
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking Android build")?
    }

    async fn build_ios_simulator(&self) -> Result<()> {
        // For iOS the dev loop produces a WhiskerDriver.xcframework
        // that the WhiskerRuntime SPM package references. The actual
        // `.app` build (xcodebuild) lives in `installer.rs::ios_install_and_launch` —
        // that step needs the bundle id + scheme from `IosParams`
        // and runs after this returns.
        let ws = self.workspace_root.clone();
        let pkg = self.package.clone();
        let features = self.features.clone();
        let capture = self.capture.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            whisker_build::ios::build_xcframework(&ws, &pkg, &features, capture.as_ref())?;
            Ok(())
        })
        .await
        .context("spawn_blocking iOS xcframework build")?
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_can_be_constructed_for_each_target() {
        for t in [Target::Host, Target::Android, Target::IosSimulator] {
            let b = Builder::new(
                PathBuf::from("/tmp/ws"),
                PathBuf::from("/tmp/ws/examples/x"),
                "x".into(),
                t,
            );
            assert!(!b.captures_shims());
            assert!(b.features.is_empty());
        }
    }

    #[test]
    fn with_features_replaces_the_feature_list() {
        let b = Builder::new(
            PathBuf::from("/tmp/ws"),
            PathBuf::from("/tmp/ws/examples/x"),
            "x".into(),
            Target::Host,
        )
        .with_features(vec!["whisker/hot-reload".into(), "extra".into()]);
        assert_eq!(b.features, vec!["whisker/hot-reload", "extra"]);
    }

    #[test]
    fn with_capture_flips_captures_shims() {
        let shims = CaptureShims {
            rustc_shim: PathBuf::from("/tmp/rs"),
            linker_shim: PathBuf::from("/tmp/ls"),
            rustc_cache_dir: PathBuf::from("/tmp/rc"),
            linker_cache_dir: PathBuf::from("/tmp/lc"),
            real_linker: PathBuf::from("/usr/bin/cc"),
            target_triple: Some("aarch64-linux-android".into()),
        };
        let b = Builder::new(
            PathBuf::from("/tmp/ws"),
            PathBuf::from("/tmp/ws/examples/x"),
            "x".into(),
            Target::Android,
        )
        .with_capture(shims);
        assert!(b.captures_shims());
    }
}
