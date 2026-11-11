#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn xcode_script_never_forwards_development_features_in_release() {
    let root = std::env::temp_dir().join(format!(
        "whisker-release-environment-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let cli = root.join("whisker-test");
    std::fs::write(&cli, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$WHISKER_TEST_CAPTURE\"\nprintf '%s' \"${WHISKER_FEATURES-unset}\" > \"$WHISKER_TEST_CAPTURE.env\"\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    let script = root.join("build-rust.sh");
    std::fs::write(
        &script,
        include_str!("../src/templates/ios/build-rust.sh")
            .replace("{{whisker_workspace_root}}", root.to_str().unwrap())
            .replace("{{whisker_user_package}}", "example"),
    )
    .unwrap();
    for configuration in ["Release", "Debug"] {
        let capture = root.join(configuration);
        assert!(
            Command::new("bash")
                .arg(&script)
                .env("WHISKER_CLI", &cli)
                .env("WHISKER_TEST_CAPTURE", &capture)
                .env("WHISKER_FEATURES", "whisker/hot-reload")
                .env("CONFIGURATION", configuration)
                .env("PLATFORM_NAME", "iphonesimulator")
                .env("ARCHS", "arm64")
                .env("BUILT_PRODUCTS_DIR", &root)
                .status()
                .unwrap()
                .success()
        );
        let args = std::fs::read_to_string(&capture).unwrap();
        let environment = std::fs::read_to_string(capture.with_extension("env")).unwrap();
        if configuration == "Release" {
            assert!(!args.contains("--features"));
            assert_eq!(environment, "unset");
        } else {
            assert!(args.contains("--features\nwhisker/hot-reload"));
            assert_eq!(environment, "whisker/hot-reload");
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
