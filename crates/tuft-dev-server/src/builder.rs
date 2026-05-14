//! Tier 2 cold rebuild: spawn cargo / xtask to produce a fresh
//! artifact for the active [`Target`].
//!
//! No subsecond patch construction here; that's I4g. This module's
//! job is to take a [`Change`] and run the right shell-out so the
//! installer (next module over) has something fresh to push.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::Target;

/// Builds the artifact appropriate for `target`. For host targets
/// that's a plain `cargo build -p`; for device targets we lean on
/// the existing `xtask` orchestration so we don't duplicate the
/// NDK / xcframework dance.
pub struct Builder {
    workspace_root: PathBuf,
    package: String,
    target: Target,
    /// Cargo features forwarded to whichever compilation step
    /// actually compiles the user crate. For Tier 2 development
    /// builds the dev loop turns on `tuft/hot-reload`.
    features: Vec<String>,
}

impl Builder {
    pub fn new(workspace_root: PathBuf, package: String, target: Target) -> Self {
        Self {
            workspace_root,
            package,
            target,
            features: Vec::new(),
        }
    }

    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }

    /// Run the build. Inherits stdout/stderr so cargo's own progress
    /// output is visible in the dev server's terminal.
    pub async fn build(&self) -> Result<()> {
        let plan = self.plan();
        let mut cmd = Command::new(&plan.program);
        cmd.args(&plan.args).current_dir(&self.workspace_root);
        let status = cmd
            .status()
            .await
            .with_context(|| format!("spawn {}", plan.program))?;
        if !status.success() {
            anyhow::bail!("{} exited {}", plan.program, status);
        }
        Ok(())
    }

    /// Pure-function side of [`build`]: derive the (program, args)
    /// pair from `target` + `package` + `features`. Factored out so
    /// unit tests don't have to actually run cargo.
    pub fn plan(&self) -> BuildPlan {
        plan_for(&self.package, self.target, &self.features)
    }
}

/// What command the dev loop will spawn to produce a fresh artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    pub program: String,
    pub args: Vec<String>,
}

fn plan_for(package: &str, target: Target, features: &[String]) -> BuildPlan {
    match target {
        Target::Host => {
            let mut args = vec!["build".into(), "-p".into(), package.to_string()];
            push_features(&mut args, features);
            BuildPlan { program: "cargo".into(), args }
        }
        Target::Android => {
            // Reuse the existing xtask orchestration:
            //   cargo xtask android build-example -p <package>
            // Feature plumbing into the cdylib is I4f; for now we
            // just smuggle the requested features into a flag the
            // xtask is going to learn to consume.
            let mut args = vec![
                "xtask".into(),
                "android".into(),
                "build-example".into(),
                "-p".into(),
                package.to_string(),
            ];
            push_features(&mut args, features);
            BuildPlan { program: "cargo".into(), args }
        }
        Target::IosSimulator => {
            // Likewise:
            //   cargo xtask ios build-xcframework -p <package>
            // The actual Simulator install + launch happens in the
            // installer; we only need a fresh xcframework here.
            let mut args = vec![
                "xtask".into(),
                "ios".into(),
                "build-xcframework".into(),
                "-p".into(),
                package.to_string(),
            ];
            push_features(&mut args, features);
            BuildPlan { program: "cargo".into(), args }
        }
    }
}

fn push_features(args: &mut Vec<String>, features: &[String]) {
    for f in features {
        args.push("--features".into());
        args.push(f.clone());
    }
}

// Cheap helper so the installer module doesn't need its own
// path-resolution logic.
pub(crate) fn android_apk_path(workspace_root: &Path, package: &str) -> PathBuf {
    workspace_root
        .join("examples")
        .join(package)
        .join("android/app/build/outputs/apk/debug/app-debug.apk")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn b(target: Target) -> Builder {
        Builder::new(
            PathBuf::from("/tmp/ws"),
            "hello-world".into(),
            target,
        )
    }

    #[test]
    fn host_plan_calls_cargo_build_for_the_package() {
        let p = b(Target::Host).plan();
        assert_eq!(p.program, "cargo");
        assert_eq!(p.args, ["build", "-p", "hello-world"]);
    }

    #[test]
    fn android_plan_invokes_xtask_build_example() {
        let p = b(Target::Android).plan();
        assert_eq!(p.program, "cargo");
        assert_eq!(
            p.args,
            ["xtask", "android", "build-example", "-p", "hello-world"],
        );
    }

    #[test]
    fn ios_plan_invokes_xtask_build_xcframework() {
        let p = b(Target::IosSimulator).plan();
        assert_eq!(p.program, "cargo");
        assert_eq!(
            p.args,
            ["xtask", "ios", "build-xcframework", "-p", "hello-world"],
        );
    }

    #[test]
    fn features_are_forwarded_repeated_per_feature() {
        let p = b(Target::Host)
            .with_features(vec!["tuft/hot-reload".into(), "extra".into()])
            .plan();
        assert_eq!(
            p.args,
            [
                "build",
                "-p",
                "hello-world",
                "--features",
                "tuft/hot-reload",
                "--features",
                "extra",
            ],
        );
    }

    #[test]
    fn android_apk_path_is_under_examples_with_debug_suffix() {
        let p = android_apk_path(Path::new("/tmp/ws"), "hello-world");
        assert!(p.to_string_lossy().ends_with(
            "/examples/hello-world/android/app/build/outputs/apk/debug/app-debug.apk"
        ));
    }
}
