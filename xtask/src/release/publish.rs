use super::*;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Instant, SystemTime};

pub(super) fn run(root: &Path) -> Result<()> {
    ensure!(
        !git(
            root,
            &["diff", "--name-only", "HEAD^", "HEAD", "--", PLAN_PATH]
        )?
        .trim()
        .is_empty(),
        "publish must run at the release-plan merge commit; rerun the original Actions job"
    );
    let plan = ReleasePlan::read(root)?;
    plan.validate_checkout(root)?;
    plan.validate_selected_streams(root)?;
    ensure!(
        git(root, &["status", "--porcelain", "--untracked-files=no"])?
            .trim()
            .is_empty(),
        "publishing requires a clean checkout"
    );
    ensure_tag(root, &plan.tag(), false)?;
    // Reused SDKs also have to be reachable: a tag or a green native build alone
    // doesn't guarantee that a downstream app can resolve its dependencies.
    for stream in ["sdk", "gradle"] {
        native::verify(stream, &native::pin(root, stream)?)?;
    }
    let swift_tag = format!("v{}", native::pin(root, "ios")?);
    ensure!(
        remote_tag(root, &swift_tag)?.is_some(),
        "SwiftPM tag {swift_tag} is missing"
    );

    let agent = http();
    let started = Instant::now();
    for attempt in 1..=80 {
        let mut pending = Vec::new();
        for (name, value) in &plan.crates {
            if !is_published(&agent, name, value)? {
                pending.push(name.clone());
            }
        }
        if pending.is_empty() {
            ensure_tag(root, &plan.tag(), true)?;
            if github_json(
                root,
                &format!("repos/{REPOSITORY}/releases/tags/{}", plan.tag()),
                false,
            )?
            .is_none()
            {
                gh(
                    root,
                    &[
                        "release",
                        "create",
                        &plan.tag(),
                        "--verify-tag",
                        "--title",
                        &format!("Whisker {}", plan.version),
                        "--notes-file",
                        &plan.notes_path(),
                    ],
                )?;
            }
            println!(
                "All {} crates verified; one GitHub Release: {}",
                plan.crates.len(),
                plan.tag()
            );
            return Ok(());
        }
        ensure!(
            started.elapsed() < Duration::from_secs(90 * 60),
            "publish recovery exceeded 90 minutes; re-run this job to resume"
        );
        println!(
            "Publish attempt {attempt}: {} unpublished crates",
            pending.len()
        );
        let mut command = Command::new(crate::cargo());
        command
            .current_dir(root)
            .args(["publish", "--locked", "--registry", "crates-io"]);
        for name in &pending {
            command.args(["-p", name]);
        }
        let (success, diagnostics) = cargo_publish(&mut command)?;
        if success {
            // Cargo waits for index propagation, but recheck the registry before
            // declaring the whole release complete (including resumed attempts).
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        let delay = retry_delay(&diagnostics, SystemTime::now())
            .context("cargo publish failed (not a crates.io rate limit)")?;
        ensure!(
            attempt < 80 && started.elapsed() + delay < Duration::from_secs(90 * 60),
            "crates.io rate limit recovery exhausted; re-run this job to resume"
        );
        println!(
            "crates.io rate limit; retrying unpublished crates after {}s",
            delay.as_secs()
        );
        std::thread::sleep(delay);
    }
    bail!("release verification did not complete; re-run this job to resume")
}

pub(super) fn cargo_publish(command: &mut Command) -> Result<(bool, String)> {
    let mut child = command
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut diagnostics = String::new();
    for line in BufReader::new(child.stderr.take().context("cargo stderr")?).lines() {
        let line = line?;
        eprintln!("{line}");
        diagnostics.push_str(&line);
        diagnostics.push('\n');
    }
    Ok((child.wait()?.success(), diagnostics))
}

pub(super) fn index_path(name: &str) -> Result<String> {
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "invalid crate name {name:?}"
    );
    let name = name.to_ascii_lowercase();
    Ok(match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    })
}

fn is_published(agent: &ureq::Agent, name: &str, value: &str) -> Result<bool> {
    let url = format!("https://index.crates.io/{}", index_path(name)?);
    match agent.get(&url).call() {
        Ok(response) => index_contains(&response.into_string()?, value),
        Err(ureq::Error::Status(404, _)) => Ok(false),
        Err(error) => Err(error).with_context(|| {
            format!(
                "check published version of {name}; registry failure is not an unpublished crate"
            )
        }),
    }
}

pub(super) fn index_contains(index: &str, value: &str) -> Result<bool> {
    for line in index.lines().filter(|line| !line.is_empty()) {
        let record: serde_json::Value = serde_json::from_str(line)?;
        if record["vers"] == value {
            ensure!(
                record["yanked"] == false,
                "{value} is yanked; choose a new version"
            );
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn retry_delay(diagnostics: &str, now: SystemTime) -> Option<Duration> {
    diagnostics
        .lines()
        .filter(|line| {
            line.contains("status 429 Too Many Requests")
                && line.contains("https://crates.io/docs/rate-limits")
        })
        .map(|line| {
            let reset = line
                .split_once("Please try again after ")
                .and_then(|(_, rest)| rest.split_once(" GMT"))
                .and_then(|(date, _)| httpdate::parse_http_date(&format!("{date} GMT")).ok());
            reset
                .and_then(|time| time.duration_since(now).ok())
                .filter(|delay| !delay.is_zero())
                .map(|delay| delay + Duration::from_secs(10))
                .unwrap_or(Duration::from_secs(120))
        })
        .max()
}
