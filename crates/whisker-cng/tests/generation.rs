#![cfg(feature = "generate")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use whisker_cng::{GenerationReport, GenerationTarget as Target, generate};

struct App(PathBuf);

impl App {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("whisker-generation-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        Self(root.canonicalize().unwrap())
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }

    fn manifest(&self) -> PathBuf {
        self.0.join("Cargo.toml")
    }

    fn cargo(&self, arguments: &[&str]) -> Output {
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(arguments)
            .current_dir(&self.0)
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap()
    }

    fn report(&self) -> GenerationReport {
        serde_json::from_slice(
            &std::fs::read(self.0.join("target/.whisker/generation.json")).unwrap(),
        )
        .unwrap()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn succeeded(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn direct_and_isolated_execution_generate_projects_with_plugins_and_feature_gating() {
    let app = App::new();
    let cng = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = cng.with_file_name("whisker-config");
    app.write(
        "Cargo.toml",
        &format!(
            r#"[package]
name = "generation-test-app"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
whisker-config = {{ path = {:?} }}
"#,
            config
        ),
    );
    app.write(
        "src/lib.rs",
        "compile_error!(\"the application must not be compiled\");",
    );
    app.write(
        "whisker.rs",
        r#"pub fn configure(app: &mut whisker_config::Config) {
    app.name("GenerationTest").bundle_id("test.generation");
}"#,
    );
    let legacy = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(
        legacy.projects[&Target::Ios]
            .gen_dir
            .join("GenerationTest.xcodeproj/project.pbxproj")
            .is_file()
    );
    assert!(!app.0.join("gen/android").exists());

    app.write(
        "Cargo.toml",
        &format!(
            r#"[package]
name = "generation-test-app"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
whisker-cng = {{ path = {:?}, default-features = false }}
[[bin]]
name = "whisker-config"
path = "./whisker.rs"
required-features = ["whisker-config"]
test = false
bench = false
[features]
whisker-config = ["whisker-cng/generate"]
"#,
            cng
        ),
    );
    app.write(
        "whisker.rs",
        r#"//! Project generation entry point.
fn main() {
    println!("configuration diagnostics need not be JSON");
    whisker_cng::run(|app| {
        app.name("GenerationTest").bundle_id("test.generation");
        app.android(|android| { android.min_sdk(24); });
    });
}"#,
    );
    let isolated = generate(&app.manifest(), &[]).unwrap();
    assert_eq!(isolated.projects.len(), 4);
    assert_eq!(isolated.config.android.min_sdk, Some(24));
    assert!(app.0.join("gen/android/settings.gradle.kts").is_file());
    assert!(app.0.join("gen/macos/Cargo.toml").is_file());
    assert!(app.0.join("gen/web/Cargo.toml").is_file());
    let project = app
        .0
        .join("gen/ios/GenerationTest.xcodeproj/project.pbxproj");
    let expected_project = std::fs::read(&project).unwrap();
    std::fs::remove_dir_all(app.0.join("gen/ios")).unwrap();
    let regenerated = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(regenerated.projects[&Target::Ios].regenerated);
    assert_eq!(std::fs::read(&project).unwrap(), expected_project);
    let reused = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(!reused.projects[&Target::Ios].regenerated);

    let gated = app.cargo(&["run", "--bin", "whisker-config"]);
    assert!(!gated.status.success());
    assert!(String::from_utf8_lossy(&gated.stderr).contains("requires the features"));
    let tree = succeeded(app.cargo(&["tree", "-e", "normal"]));
    let tree = String::from_utf8(tree.stdout).unwrap();
    assert!(!tree.contains("cargo_metadata v"), "{tree}");
    assert!(!tree.contains("image v"), "{tree}");

    app.write("src/lib.rs", "pub fn application() {}");
    let direct = succeeded(app.cargo(&[
        "run",
        "--quiet",
        "--bin",
        "whisker-config",
        "--features",
        "whisker-config",
        "--",
        "ios",
    ]));
    assert!(
        String::from_utf8(direct.stdout)
            .unwrap()
            .contains("configuration diagnostics")
    );
    assert_eq!(std::fs::read(&project).unwrap(), expected_project);
    assert_eq!(
        serde_json::to_value(&app.report().config).unwrap(),
        serde_json::to_value(&isolated.config).unwrap()
    );

    let asset = cng
        .join("../../packages/whisker-asset")
        .canonicalize()
        .unwrap();
    succeeded(app.cargo(&["add", "whisker-asset", "--path", asset.to_str().unwrap()]));
    app.write("assets/message.txt", "bundled by CNG");
    app.write(
        "whisker.rs",
        r#"fn main() {
    whisker_cng::run(|app| {
        app.name("GenerationTest").bundle_id("test.generation");
        app.plugin::<whisker_asset::WhiskerAsset>(|assets| { assets.dir("assets"); });
    });
}"#,
    );
    app.write(
        "src/lib.rs",
        "compile_error!(\"application must stay isolated\");",
    );
    app.write(
        ".cargo/config.toml",
        "[build]\ntarget = \"wasm32-unknown-unknown\"\n",
    );
    let assets = generate(&app.manifest(), &[Target::Android, Target::Ios]).unwrap();
    assert_eq!(assets.config.plugins["whisker-asset"]["dirs"][0], "assets");
    let ios = std::fs::read_to_string(&project).unwrap();
    assert!(ios.contains("whisker_assets"), "{ios}");
    assert_eq!(
        std::fs::read_to_string(app.0.join("gen/ios/whisker_assets/message.txt")).unwrap(),
        "bundled by CNG"
    );
    assert_eq!(
        std::fs::read_to_string(
            app.0
                .join("gen/android/app/src/main/assets/whisker/message.txt")
        )
        .unwrap(),
        "bundled by CNG"
    );

    app.write(
        "whisker.rs",
        r#"fn main() {
    whisker_cng::run(|app| {
        app.name("GenerationTest").bundle_id("test.generation");
        app.plugin::<whisker_asset::WhiskerAsset>(|assets| { assets.dir("missing-assets"); });
    });
}"#,
    );
    assert!(generate(&app.manifest(), &[Target::Ios]).is_err());
    assert!(!app.0.join("target/.whisker/generation.json").exists());

    std::fs::remove_file(app.0.join(".cargo/config.toml")).unwrap();
    app.write("src/lib.rs", "pub fn application() {}");
    app.write(
        "whisker.rs",
        "compile_error!(\"generator must be skipped\"); fn main() {}",
    );
    succeeded(app.cargo(&["build", "--quiet"]));
    succeeded(app.cargo(&[
        "rustc",
        "--quiet",
        "--crate-type",
        "cdylib",
        "--",
        "-C",
        "debuginfo=0",
    ]));
}
