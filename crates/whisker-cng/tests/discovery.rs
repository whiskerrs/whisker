//! End-to-end check on `discover_plugins`.
//!
//! Builds a throwaway 3-crate workspace under a tempdir (app +
//! plugin + plugin-with-multiple-entries) and verifies cargo
//! metadata + the parser produce the expected
//! [`DiscoveredPlugin`] list.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_cng::discover_plugins;

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-discovery-test-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Write a tiny path-deps workspace: one app crate consumes the
/// plugin crate(s). Returns the path to the workspace root
/// (which holds the workspace `Cargo.toml`).
struct WorkspaceFixture {
    root: PathBuf,
}

impl WorkspaceFixture {
    fn new() -> Self {
        let root = unique_tempdir();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"
                [workspace]
                resolver = "2"
                members = ["app", "plugin-a", "plugin-multi"]
            "#,
        )
        .unwrap();
        Self { root }
    }

    fn add_app(&self, deps: &[(&str, &str)]) {
        let dir = self.root.join("app");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let deps_block = deps
            .iter()
            .map(|(name, path)| format!("{name} = {{ path = \"{path}\" }}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                r#"
                    [package]
                    name = "test-app"
                    version = "0.0.1"
                    edition = "2021"

                    [dependencies]
                    {deps_block}
                "#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    }

    fn add_plugin(&self, dir_name: &str, package_name: &str, plugins_toml: &str) {
        let dir = self.root.join(dir_name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                r#"
                    [package]
                    name = "{package_name}"
                    version = "0.0.1"
                    edition = "2021"

                    {plugins_toml}
                "#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    }

    fn app_manifest(&self) -> PathBuf {
        self.root.join("app/Cargo.toml")
    }
}

#[test]
fn discovers_a_single_plugin_in_a_dep_crate() {
    let fx = WorkspaceFixture::new();
    fx.add_app(&[
        ("plugin-a", "../plugin-a"),
        ("plugin-multi", "../plugin-multi"),
    ]);
    fx.add_plugin(
        "plugin-a",
        "plugin-a",
        r#"
            [package.metadata.whisker.plugins.alpha-plugin]
            bin = "alpha-plugin-cng"
        "#,
    );
    fx.add_plugin(
        "plugin-multi",
        "plugin-multi",
        r#"
            [package.metadata.whisker.plugins.beta-plugin]
            bin = "beta-plugin-cng"
            after = ["alpha-plugin"]

            [package.metadata.whisker.plugins.gamma-plugin]
            bin = "gamma-plugin-cng"
            before = ["beta-plugin"]
        "#,
    );

    let mut plugins = discover_plugins(&fx.app_manifest(), "test-app").unwrap();
    plugins.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(plugins.len(), 3);

    assert_eq!(plugins[0].name, "alpha-plugin");
    assert_eq!(plugins[0].source_crate, "plugin-a");
    assert_eq!(plugins[0].bin_target_name, "alpha-plugin-cng");
    assert!(plugins[0].after.is_empty());

    assert_eq!(plugins[1].name, "beta-plugin");
    assert_eq!(plugins[1].source_crate, "plugin-multi");
    assert_eq!(plugins[1].bin_target_name, "beta-plugin-cng");
    assert_eq!(plugins[1].after, vec!["alpha-plugin"]);

    assert_eq!(plugins[2].name, "gamma-plugin");
    assert_eq!(plugins[2].source_crate, "plugin-multi");
    assert_eq!(plugins[2].bin_target_name, "gamma-plugin-cng");
    assert_eq!(plugins[2].before, vec!["beta-plugin"]);
}

#[test]
fn returns_empty_when_no_dep_declares_plugins() {
    let fx = WorkspaceFixture::new();
    fx.add_app(&[
        ("plugin-a", "../plugin-a"),
        ("plugin-multi", "../plugin-multi"),
    ]);
    fx.add_plugin("plugin-a", "plugin-a", "");
    // Even an unrelated `[package.metadata.whisker.ios]` block (the
    // module-discovery surface) should not be picked up as a
    // plugin.
    fx.add_plugin(
        "plugin-multi",
        "plugin-multi",
        r#"
            [package.metadata.whisker.ios]
            swift_sources = []
        "#,
    );

    let plugins = discover_plugins(&fx.app_manifest(), "test-app").unwrap();
    assert!(plugins.is_empty(), "{plugins:?}");
}

#[test]
fn duplicate_plugin_name_across_crates_is_rejected() {
    let fx = WorkspaceFixture::new();
    fx.add_app(&[
        ("plugin-a", "../plugin-a"),
        ("plugin-multi", "../plugin-multi"),
    ]);
    fx.add_plugin(
        "plugin-a",
        "plugin-a",
        r#"
            [package.metadata.whisker.plugins.collision]
            bin = "from-plugin-a"
        "#,
    );
    fx.add_plugin(
        "plugin-multi",
        "plugin-multi",
        r#"
            [package.metadata.whisker.plugins.collision]
            bin = "from-plugin-multi"
        "#,
    );

    let err = discover_plugins(&fx.app_manifest(), "test-app").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("collision"), "{msg}");
    assert!(msg.contains("plugin-a"), "{msg}");
    assert!(msg.contains("plugin-multi"), "{msg}");
}

#[test]
fn typoed_plugin_entry_field_is_rejected_with_crate_path() {
    let fx = WorkspaceFixture::new();
    fx.add_app(&[
        ("plugin-a", "../plugin-a"),
        ("plugin-multi", "../plugin-multi"),
    ]);
    fx.add_plugin(
        "plugin-a",
        "plugin-a",
        r#"
            [package.metadata.whisker.plugins.bad]
            bin = "ok-bin"
            aftr = ["typo-of-after"]
        "#,
    );
    fx.add_plugin("plugin-multi", "plugin-multi", "");

    let err = discover_plugins(&fx.app_manifest(), "test-app").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("aftr"), "{msg}");
    // Crate-path attribution should be present so the user knows
    // which Cargo.toml to fix.
    assert!(msg.contains("plugin-a"), "{msg}");
}
