use super::*;

const MAVEN: &str = "https://whiskerrs.github.io/whisker/maven";

fn definition(stream: &str) -> Result<(&'static str, &'static str, &'static str)> {
    match stream {
        "sdk" => Ok((CLI_PATH, "WHISKER_SDK_VERSION", "sdk-v")),
        "gradle" => Ok((CLI_PATH, "WHISKER_GRADLE_PLUGIN_VERSION", "gradle-plugin-v")),
        "ios" => Ok((IOS_PATH, "WHISKER_IOS_SPM_VERSION", "v")),
        _ => bail!("unknown native stream {stream:?}"),
    }
}

pub(super) fn pin(root: &Path, stream: &str) -> Result<String> {
    let (path, name, _) = definition(stream)?;
    let source = fs::read_to_string(root.join(path))?;
    let marker = format!("{name}: &str = \"");
    Ok(source
        .split_once(&marker)
        .and_then(|(_, tail)| tail.split_once('"'))
        .context("native version constant")?
        .0
        .to_owned())
}

pub(super) fn update_pin(root: &Path, stream: &str, next: &str) -> Result<()> {
    let previous = pin(root, stream)?;
    let (path, name, _) = definition(stream)?;
    replace_once(
        &root.join(path),
        &format!("{name}: &str = \"{previous}\""),
        &format!("{name}: &str = \"{next}\""),
    )?;
    if stream == "ios" {
        check_swift_pins(root, &previous)?;
        for path in swift_manifests(root)? {
            replace_once(
                &path,
                &format!("whiskerrs/whisker.git\", exact: \"{previous}\""),
                &format!("whiskerrs/whisker.git\", exact: \"{next}\""),
            )?;
        }
    }
    Ok(())
}

fn replace_once(path: &Path, from: &str, to: &str) -> Result<()> {
    let text = fs::read_to_string(path)?;
    ensure!(
        text.matches(from).count() == 1,
        "expected one {from:?} in {}",
        path.display()
    );
    fs::write(path, text.replacen(from, to, 1))?;
    Ok(())
}

fn swift_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root.join("packages"))? {
        let path = entry?.path().join("Package.swift");
        if path.is_file() && fs::read_to_string(&path)?.contains("whiskerrs/whisker.git") {
            paths.push(path);
        }
    }
    ensure!(!paths.is_empty(), "no Swift module manifests found");
    Ok(paths)
}

pub(super) fn check_swift_pins(root: &Path, expected: &str) -> Result<()> {
    for path in swift_manifests(root)? {
        ensure!(
            fs::read_to_string(&path)?
                .contains(&format!("whiskerrs/whisker.git\", exact: \"{expected}\"")),
            "{} does not pin SwiftPM {expected}",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn check_selection(root: &Path, stream: &str, selected: Option<&str>) -> Result<()> {
    let current = pin(root, stream)?;
    let (_, _, prefix) = definition(stream)?;
    if let Some(next) = selected {
        ensure!(
            version(next)? > version(&current)?,
            "{stream} version must be newer than {current}; leave it blank to reuse {current}"
        );
        ensure!(
            remote_tag(root, &format!("{prefix}{next}"))?.is_none(),
            "{stream} {next} is already tagged; choose a new version"
        );
    } else {
        let previous = format!("{prefix}{current}");
        ensure!(
            remote_tag(root, &previous)?.is_some(),
            "current {stream} tag {previous} is missing"
        );
        let mut args = vec!["diff", "--name-only", &previous, "HEAD", "--"];
        args.extend(match stream {
            "sdk" => vec![
                "platforms/android/runtime",
                "platforms/android/module",
                "platforms/android/ksp",
                "platforms/android/build.gradle.kts",
                "platforms/android/settings.gradle.kts",
            ],
            "gradle" => vec!["platforms/android/gradle-plugin"],
            "ios" => vec!["Package.swift", "platforms/ios"],
            _ => unreachable!(),
        });
        let changed = git(root, &args)?;
        ensure!(
            changed.trim().is_empty(),
            "{stream} has changes since {previous}; specify a new {stream} version:\n{changed}"
        );
    }
    Ok(())
}

pub(super) fn check(root: &Path, stream: &str, value: &str) -> Result<()> {
    version(value)?;
    ensure!(
        pin(root, stream)? == value,
        "{stream} source pin does not match {value}"
    );
    if stream == "ios" {
        check_swift_pins(root, value)?;
    }
    let (_, _, prefix) = definition(stream)?;
    let tag = format!("{prefix}{value}");
    ensure_tag(root, &tag, false)?;
    let published = if stream == "ios" {
        remote_tag(root, &tag)?.is_some()
    } else {
        let endpoint = format!(
            "repos/{REPOSITORY}/contents/maven/whisker-releases/{stream}-{value}.json?ref=gh-pages"
        );
        if let Some(receipt) = github_json(root, &endpoint, true)? {
            let head = git(root, &["rev-parse", "HEAD"])?;
            ensure!(
                receipt["commit"] == head.trim(),
                "Maven version {value} was published from another commit"
            );
            ensure!(
                remote_tag(root, &tag)?.is_some(),
                "published Maven receipt has no source tag"
            );
            true
        } else {
            false
        }
    };
    output("published", if published { "true" } else { "false" })
}

pub(super) fn stamp(root: &Path, stream: &str, value: &str) -> Result<()> {
    // Smoke workflows use the development version; publish callers validate
    // stable versions and source pins with native-check before this step.
    if value != "0.0.0-dev" {
        version(value)?;
    }
    let paths = match stream {
        "sdk" => vec![
            "platforms/android/module/build.gradle.kts",
            "platforms/android/runtime/build.gradle.kts",
            "platforms/android/ksp/ksp/build.gradle.kts",
        ],
        "gradle" => vec![
            "platforms/android/gradle-plugin/whisker-settings-plugin/build.gradle.kts",
            "platforms/android/gradle-plugin/whisker-gradle-plugin/build.gradle.kts",
        ],
        _ => bail!("only Maven builds need version stamping"),
    };
    for path in paths {
        let path = root.join(path);
        let text = fs::read_to_string(&path)?;
        let assignments: Vec<_> = text
            .lines()
            .filter(|line| line.starts_with("version = \""))
            .collect();
        ensure!(
            assignments.len() == 1,
            "expected one version assignment in {}",
            path.display()
        );
        replace_once(&path, assignments[0], &format!("version = \"{value}\""))?;
    }
    Ok(())
}

pub(super) fn tag(root: &Path, stream: &str, value: &str) -> Result<()> {
    version(value)?;
    ensure!(
        pin(root, stream)? == value,
        "native version pin differs from tag"
    );
    if stream == "ios" {
        check_swift_pins(root, value)?;
    }
    let (_, _, prefix) = definition(stream)?;
    ensure_tag(root, &format!("{prefix}{value}"), true)?;
    if stream != "ios" {
        // Published atomically alongside Maven files. On retry this receipt lets
        // us skip the upload even while Pages/CDN propagation is still pending.
        let receipts = root.join("maven-out/whisker-releases");
        fs::create_dir_all(&receipts)?;
        let head = git(root, &["rev-parse", "HEAD"])?;
        fs::write(
            receipts.join(format!("{stream}-{value}.json")),
            serde_json::to_string(&serde_json::json!({"commit": head.trim()}))?,
        )?;
    }
    Ok(())
}

pub(super) fn artifact_urls(stream: &str, value: &str) -> Result<Vec<String>> {
    version(value)?;
    let artifacts = match stream {
        "sdk" => vec![
            ("rs/whisker", "whisker-module-android", "aar"),
            ("rs/whisker", "whisker-runtime-android", "aar"),
            ("rs/whisker", "ksp", "jar"),
        ],
        "gradle" => vec![
            ("rs/whisker", "whisker-settings-plugin", "jar"),
            ("rs/whisker", "whisker-gradle-plugin", "jar"),
            ("rs/whisker", "rs.whisker.gradle.plugin", "pom"),
            (
                "rs/whisker/gradle",
                "rs.whisker.gradle.gradle.plugin",
                "pom",
            ),
        ],
        _ => bail!("{stream} has no Maven artifacts"),
    };
    let mut urls = Vec::new();
    for (group, artifact, extension) in artifacts {
        let base = format!("{MAVEN}/{group}/{artifact}/{value}/{artifact}-{value}");
        urls.push(format!("{base}.{extension}"));
        if extension != "pom" {
            urls.push(format!("{base}.pom"));
        }
    }
    Ok(urls)
}

pub(super) fn verify(stream: &str, value: &str) -> Result<()> {
    let agent = http();
    let mut missing = artifact_urls(stream, value)?;
    for attempt in 0..60 {
        let mut pending = Vec::new();
        for url in missing {
            match agent.head(&url).call() {
                Ok(response) if response.status() == 200 => println!("Available: {url}"),
                Ok(response) => bail!("unexpected status {} for {url}", response.status()),
                Err(ureq::Error::Status(404 | 429 | 500..=599, _))
                | Err(ureq::Error::Transport(_)) => pending.push(url),
                Err(error) => return Err(error.into()),
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        missing = pending;
        if attempt < 59 {
            std::thread::sleep(Duration::from_secs(10));
        }
    }
    bail!(
        "Maven artifacts are not reachable after waiting: {}",
        missing.join(", ")
    )
}
