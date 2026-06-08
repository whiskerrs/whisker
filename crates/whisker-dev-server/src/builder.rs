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

    /// Read-only view of the features currently configured. The dev
    /// loop reads this when constructing the [`Installer`] so the iOS
    /// xcodebuild env var (`WHISKER_FEATURES`) stays in sync with what
    /// the Builder would have passed to a direct cargo invocation.
    pub fn features(&self) -> &[String] {
        &self.features
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
        // Dev loop only stages module Kotlin sources, then drives
        // gradle. Gradle's own `whiskerBuildDebugArm64V8a` task runs
        // `whisker-build android` (which runs cargo + stages the .so +
        // libc++_shared.so into the generated jniLibs source dir AGP
        // mergeJniLibFolders picks up), so a *second* pre-cargo build
        // here would just produce the same `.so` twice and leak its
        // output across the curated dev-loop UI.
        //
        // Mirrors what iOS already does: cargo runs only inside
        // xcodebuild's Build Phase; the dev-server's `build_ios_simulator`
        // is module-source-staging only. Aligning Android to the same
        // shape halves the wall-clock of every Tier 2 rebuild on a
        // cache-warm cargo and removes one race against the TUI viewport.
        let ws = self.workspace_root.clone();
        let crate_dir = self.crate_dir.clone();
        let pkg = self.package.clone();
        let features = self.features.clone();
        let capture = self.capture.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let gen_android = crate_dir.join("gen/android");
            // Stage discovered Whisker modules' Android Kotlin
            // sources before gradle runs. Empty when no module
            // declares android.kotlin_sources.
            let modules = whisker_build::modules::discover(&ws.join("Cargo.toml"), &pkg)?;
            whisker_build::android::stage_module_kotlin_sources(&gen_android, &modules)?;
            whisker_build::android::run_gradle_assemble(
                &gen_android,
                whisker_build::Profile::Debug,
                &features,
                capture.as_ref(),
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking Android build")?
    }

    async fn build_ios_simulator(&self) -> Result<()> {
        // Step 7: this method's only remaining job is to stage the
        // module Swift sources for SwiftPM. The actual `.app` build —
        // and the cargo cross-compile that produces
        // `WhiskerDriver.framework` — happens during xcodebuild in
        // `installer.rs::ios_install_and_launch`, via the cng-generated
        // pbxproj's "Whisker Generate" Run Script Build Phase.
        //
        // Pre-Step-7 this method also ran `build_xcframework_with` to
        // produce `target/whisker-driver/WhiskerDriver.xcframework`
        // and to prime the Tier 1 capture shims. The xcframework is
        // no longer referenced by anything (Step 7 dropped the SPM
        // binaryTarget) so its output was wasted; the capture wiring
        // moved to `installer.rs` where it gets applied as env vars
        // on the xcodebuild Command.
        let ws = self.workspace_root.clone();
        let crate_dir = self.crate_dir.clone();
        let pkg = self.package.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            // Stage Whisker modules' iOS Swift sources before
            // xcodebuild runs so the pbxproj's WhiskerModules SwiftPM
            // ref resolves cleanly. Empty when no module declares
            // `[ios].swift_sources` — the staging step still writes a
            // no-op Package.swift + WhiskerModuleBehaviors.swift so
            // AppDelegate's `import WhiskerModules` doesn't fail to
            // resolve.
            let modules = whisker_build::modules::discover(&ws.join("Cargo.toml"), &pkg)?;
            let gen_ios = crate_dir.join("gen/ios");
            let whisker_runtime_path = ws.join("platforms/ios");
            let whisker_ios_macros_path = ws.join("platforms/ios/macros");
            whisker_build::ios::stage_module_swift_sources(
                &gen_ios,
                &whisker_runtime_path,
                &whisker_ios_macros_path,
                &modules,
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking iOS module-source stage")?
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
