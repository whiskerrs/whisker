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
            // Panics only restore the terminal. The hook also runs for panics the
            // process survives, such as Tokio catching a panicking task, where
            // stopping builds or deleting staging would break work in flight.
            // Main-thread panics unwind, so `MaterializedDir` drops clean up.
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                crate::tui::reset_terminal();
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
        let build = KillOnDrop(Command::new("sleep").arg("60").spawn().unwrap());
        let _tracked = whisker_build::child_guard::track(build.0.id());
        let mode = std::env::var("WHISKER_CLEANUP_TEST_MODE").unwrap_or_default();
        if mode == "thread-panic" {
            let _ = std::thread::spawn(|| panic!("exercise background panic")).join();
        }
        let ready = std::path::Path::new(&marker).with_extension("tmp");
        std::fs::write(
            &ready,
            format!("{}\n{}", secret.to_str().unwrap(), build.0.id()),
        )
        .unwrap();
        std::fs::rename(ready, marker).unwrap();
        if mode == "panic" {
            panic!("exercise panic cleanup");
        }
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stops a test process that is still running when this handle drops.
    struct KillOnDrop(std::process::Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    fn process_exists(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    fn signal(pid: u32, name: &str) {
        assert!(
            Command::new("kill")
                .args([format!("-{name}"), pid.to_string()])
                .status()
                .unwrap()
                .success()
        );
    }

    fn wait_until(deadline: Instant, mut done: impl FnMut() -> bool) -> bool {
        while !done() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    #[test]
    fn exits_remove_plaintext_but_survived_panics_keep_builds() {
        for mode in ["INT", "TERM", "panic", "thread-panic"] {
            let temp = std::env::temp_dir().join(format!(
                "whisker-cleanup-test-{}-{mode}",
                std::process::id()
            ));
            std::fs::create_dir_all(&temp).unwrap();
            let marker = temp.join("ready");
            let mut child = KillOnDrop(
                Command::new(std::env::current_exe().unwrap())
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
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(20);
            wait_until(deadline, || {
                marker.exists() || child.0.try_wait().unwrap().is_some()
            });
            assert!(marker.exists(), "{mode} cleanup child did not start");
            let ready = std::fs::read_to_string(&marker).unwrap();
            let (path, build) = ready.split_once('\n').unwrap();
            let path = std::path::PathBuf::from(path);
            let build: u32 = build.parse().unwrap();
            if mode == "thread-panic" {
                // The process survived a caught panic, so the build and its
                // staging must still be in use.
                std::thread::sleep(Duration::from_millis(200));
                assert!(child.0.try_wait().unwrap().is_none(), "{mode} exited");
                assert!(path.exists(), "{mode} removed live plaintext credentials");
                assert!(process_exists(build), "{mode} stopped a live build child");
                signal(child.0.id(), "TERM");
            } else if mode != "panic" {
                signal(child.0.id(), mode);
            }
            assert!(
                wait_until(deadline, || child.0.try_wait().unwrap().is_some()),
                "{mode} cleanup child did not exit"
            );
            assert!(!path.exists(), "{mode} left plaintext credentials");
            assert!(
                !path.parent().unwrap().exists(),
                "{mode} left staging directory"
            );
            assert!(
                wait_until(deadline, || !process_exists(build)),
                "{mode} left the tracked build child running"
            );
            std::fs::remove_dir_all(temp).unwrap();
        }
    }
}
