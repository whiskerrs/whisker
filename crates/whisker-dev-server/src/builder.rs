//! full reload: produce a fresh artifact + (re)install it on
//! the active [`Target`].
//!
//! Delegates the cargo + gradle / xcodebuild orchestration to
//! `whisker-build`, which is shared with `whisker-cli`'s `whisker
//! build` subcommand. When `with_capture` is set, the cargo step
//! doubles as a **fat build** that fills the rustc + linker capture
//! caches the hot-reload patch pipeline replays later.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::{MacosParams, Target, WebParams};
use whisker_build::CaptureShims;

/// Builder for cold (full reload) rebuilds. hot-reload patches live in
/// [`crate::hotpatch::Patcher`] — Builder is only invoked for
/// dependency-shaped changes (Cargo.toml edits) and as a fallback
/// when hot reload errors.
pub struct Builder {
    workspace_root: PathBuf,
    /// User crate dir (= `Cargo.toml` parent). Needed to find
    /// `gen/android/` for gradle invocation.
    crate_dir: PathBuf,
    package: String,
    target: Target,
    /// Cargo features forwarded to whichever step compiles the user
    /// crate. The dev loop turns on `whisker/hot-reload` for mobile and
    /// the generated Host's `hot-reload` feature for macOS.
    features: Vec<String>,
    /// `Some` → fat build (hot reload capture caches get populated).
    /// `None` → plain full reload.
    capture: Option<CaptureShims>,
    macos: Option<MacosParams>,
    web: Option<WebParams>,
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
            macos: None,
            web: None,
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

    /// Supplies the generated native macOS project when this builder targets
    /// [`Target::Macos`].
    pub fn with_macos(mut self, macos: Option<MacosParams>) -> Self {
        self.macos = macos;
        self
    }

    pub fn with_web(mut self, web: Option<WebParams>) -> Self {
        self.web = web;
        self
    }

    /// Run the build for the current target. Inherits stdout/stderr.
    pub async fn build(&self) -> Result<()> {
        match self.target {
            Target::Android => self.build_android().await,
            Target::IosSimulator => self.build_ios_simulator().await,
            Target::Macos => self.build_macos().await,
            Target::Web => self.build_web().await,
        }
    }

    /// Whether this builder is configured for a fat build.
    pub fn captures_shims(&self) -> bool {
        self.capture.is_some()
    }

    // ----- per-target build paths ------------------------------------------

    async fn build_android(&self) -> Result<()> {
        let workspace_root = self.workspace_root.clone();
        let crate_dir = self.crate_dir.clone();
        let package = self.package.clone();
        let features = self.features.clone();
        let capture = self.capture.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            const ABI: &str = "arm64-v8a";
            let toolchain = whisker_build::android::resolve_toolchain(ABI, 24)
                .context("resolve Android NDK toolchain")?;
            let dylib =
                whisker_build::android::cargo_build_dylib(&whisker_build::android::CargoBuild {
                    workspace_root: &workspace_root,
                    package: &package,
                    toolchain: &toolchain,
                    profile: whisker_build::Profile::Debug,
                    features: &features,
                    capture: capture.as_ref(),
                })?;
            let gen_android = crate_dir.join("gen/android");
            whisker_build::android::stage_so_files(
                &gen_android.join("app/src/main/jniLibs").join(ABI),
                &dylib,
                &toolchain,
                ABI,
            )?;
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

    async fn build_web(&self) -> Result<()> {
        let web = self
            .web
            .clone()
            .context("target=Web but Config.web is missing")?;
        let features = self.features.clone();
        let capture = self.capture.clone();
        tokio::task::spawn_blocking(move || {
            let config = whisker_build::web::WebBuild {
                project_dir: web.project_dir,
                target_dir: web.target_dir,
                dist_dir: web.dist_dir,
                package: web.generated_package,
                profile: whisker_build::Profile::Debug,
                features,
                capture,
                development: true,
            };
            let raw_wasm = whisker_build::web::compile(&config)?;
            if config.capture.is_some() {
                let bytes = std::fs::read(&raw_wasm)
                    .with_context(|| format!("read {}", raw_wasm.display()))?;
                let prepared = crate::hotpatch::prepare_wasm_base_module(&bytes)
                    .context("prepare WebAssembly base module for Hot Reload")?;
                std::fs::write(&raw_wasm, prepared)
                    .with_context(|| format!("write {}", raw_wasm.display()))?;
            }
            whisker_build::web::bindgen(&config, &raw_wasm)?;
            Ok(())
        })
        .await
        .context("join Web build task")?
    }

    async fn build_ios_simulator(&self) -> Result<()> {
        let workspace_root = self.workspace_root.clone();
        let package = self.package.clone();
        let features = self.features.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let built_products = workspace_root
                .join("target/.whisker/ios-derived")
                .join(&package)
                .join("Build/Products/Debug-iphonesimulator");
            whisker_build::ios::build_framework_for_xcode_run_script(
                &whisker_build::ios::XcodeRunScriptInputs {
                    workspace_root: &workspace_root,
                    package: &package,
                    platform: "iphonesimulator",
                    // The generated Xcode project uses a generic simulator
                    // destination, which asks for both slices even on an
                    // Apple Silicon development machine.
                    archs: &["arm64", "x86_64"],
                    features: &features,
                },
                &built_products,
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking iOS Rust framework build")?
    }

    async fn build_macos(&self) -> Result<()> {
        let params = self
            .macos
            .clone()
            .context("target=Macos but no MacosParams were provided")?;
        let features = self.features.clone();
        let capture = self.capture.clone();
        tokio::task::spawn_blocking(move || {
            whisker_build::macos::build_app(&whisker_build::macos::MacosBuild {
                project_dir: &params.project_dir,
                target_dir: &params.target_dir,
                app_name: &params.app_name,
                binary_name: &params.binary_name,
                profile: whisker_build::Profile::Debug,
                features: &features,
                capture: capture.as_ref(),
            })
            .map(|_| ())
        })
        .await
        .context("spawn_blocking macOS Host build")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_can_be_constructed_for_each_target() {
        for t in [
            Target::Android,
            Target::IosSimulator,
            Target::Macos,
            Target::Web,
        ] {
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
            Target::Android,
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
