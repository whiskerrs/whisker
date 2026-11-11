//! Process-wide cleanup for interactive and non-interactive CLI workflows.

use std::sync::OnceLock;

fn cleanup() {
    crate::tui::reset_terminal();
    whisker_build::child_guard::kill_all();
    if whisker_credentials::cleanup_staging().is_err() {
        whisker_build::ui::error("could not remove plaintext credential staging");
    }
}

pub(crate) fn exit(code: i32) -> ! {
    cleanup();
    std::process::exit(code)
}

pub(crate) fn install() -> anyhow::Result<()> {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            ctrlc::set_handler(|| exit(130)).map_err(|error| error.to_string())?;
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                cleanup();
                previous(info);
            }));
            Ok(())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!("install CLI cleanup handler: {error}"))?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "subprocess helper for signal cleanup tests"]
    fn cleanup_child() {
        let Ok(marker) = std::env::var("WHISKER_CLEANUP_TEST_MARKER") else {
            return;
        };
        install().unwrap();
        let staging = whisker_credentials::MaterializedDir::new().unwrap();
        let secret = staging.write("test.p8", b"test secret").unwrap();
        std::fs::write(marker, secret.to_str().unwrap()).unwrap();
        if std::env::var("WHISKER_CLEANUP_TEST_MODE").as_deref() == Ok("panic") {
            panic!("exercise panic cleanup");
        }
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn signals_and_panics_remove_plaintext_in_noninteractive_processes() {
        for mode in ["INT", "TERM", "panic"] {
            let temp = std::env::temp_dir().join(format!(
                "whisker-cleanup-test-{}-{mode}",
                std::process::id()
            ));
            std::fs::create_dir_all(&temp).unwrap();
            let marker = temp.join("ready");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "shutdown::tests::cleanup_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("WHISKER_CLEANUP_TEST_MARKER", &marker)
                .env("WHISKER_CLEANUP_TEST_MODE", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while !marker.exists() && Instant::now() < deadline {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if !marker.exists() {
                let _ = child.kill();
                let _ = child.wait();
                panic!("cleanup child did not start");
            }
            let path = std::path::PathBuf::from(std::fs::read_to_string(&marker).unwrap());
            if mode != "panic" {
                assert!(
                    Command::new("kill")
                        .args([format!("-{mode}"), child.id().to_string()])
                        .status()
                        .unwrap()
                        .success()
                );
            }
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if child.try_wait().unwrap().is_none() {
                let _ = child.kill();
                let _ = child.wait();
                panic!("cleanup child did not exit");
            }
            assert!(!path.exists(), "{mode} left plaintext credentials");
            assert!(
                !path.parent().unwrap().exists(),
                "{mode} left staging directory"
            );
            std::fs::remove_dir_all(temp).unwrap();
        }
    }
}
