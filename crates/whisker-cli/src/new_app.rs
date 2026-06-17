//! `whisker new <name>` — scaffold a new Whisker app crate.
//!
//! Creates a directory matching the supplied crate name with the
//! minimum-viable Whisker app skeleton: a single-crate workspace
//! `Cargo.toml`, a tiny `src/lib.rs` with `#[whisker::main]`, the
//! `whisker.rs` `Config` probe, a `.gitignore`, a `rust-analyzer.toml`
//! (format-on-save via `whisker fmt`), and a `README.md`.
//! The result compiles standalone — the user runs `whisker run
//! --target host` (or `--target ios` / `android` if their machine
//! passes `whisker doctor`) and sees an interactive counter.
//!
//! ## Why a single-crate workspace?
//!
//! `whisker run` walks up from the crate's `Cargo.toml` looking for a
//! `[workspace]` table — it uses the workspace root for Lynx cache
//! paths, the rustc-shim cache dir, etc. A standalone app crate needs
//! to advertise itself as both `[package]` and `[workspace]` so the
//! single directory satisfies both lookups; otherwise `whisker run`
//! errors out with "no [workspace] Cargo.toml at or above …".
//!
//! ## Naming
//!
//! - **Crate name** (the `<name>` arg): kebab-case, must be a valid
//!   cargo package name. Example: `my-app`, `awesome-thing`.
//! - **Display name** (derived from crate name): title-cased,
//!   spaces between words. Example: `My App`, `Awesome Thing`.
//!   Override with `--display-name`.
//! - **Bundle ID** (derived from crate name): `rs.example.<ns>`
//!   where `<ns>` is `_`-joined snake_case. Override with
//!   `--bundle-id`.

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};

/// `whisker new` CLI arguments.
#[derive(Args, Debug)]
pub struct NewAppArgs {
    /// The cargo crate name. kebab-case (`my-app`, `awesome-thing`).
    /// Must be a valid cargo package name — letters / digits / `-` /
    /// `_`, must start with a letter.
    pub name: String,

    /// Optional parent directory. Defaults to the current working
    /// directory. The new crate lands at `<parent>/<name>/`.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Override the iOS bundle id / Android applicationId.
    /// Defaults to `rs.example.<snake_case_name>`.
    #[arg(long)]
    pub bundle_id: Option<String>,

    /// Override the human-readable app display name. Defaults to the
    /// crate name with `-` swapped for spaces and each word
    /// title-cased (`my-app` → `My App`).
    #[arg(long)]
    pub display_name: Option<String>,
}

pub fn run(args: NewAppArgs) -> Result<()> {
    validate_crate_name(&args.name)?;
    let parent = args.path.unwrap_or_else(|| PathBuf::from("."));
    let target_dir = parent.join(&args.name);
    if target_dir.exists() {
        bail!(
            "{}: directory already exists. Pick a different name or remove it.",
            target_dir.display(),
        );
    }

    let ns = args.name.replace('-', "_");
    let display_name = args
        .display_name
        .clone()
        .unwrap_or_else(|| derive_display_name(&args.name));
    let bundle_id = args
        .bundle_id
        .clone()
        .unwrap_or_else(|| format!("rs.example.{ns}"));

    let v = Vars {
        crate_name: &args.name,
        display_name: &display_name,
        bundle_id: &bundle_id,
    };

    std::fs::create_dir_all(target_dir.join("src"))
        .with_context(|| format!("create {}/src", target_dir.display()))?;

    write(&target_dir, "Cargo.toml", &cargo_toml(&v))?;
    write(&target_dir, "src/lib.rs", &lib_rs(&v))?;
    write(&target_dir, "whisker.rs", &whisker_rs(&v))?;
    write(&target_dir, ".gitignore", GITIGNORE)?;
    write(&target_dir, "rust-analyzer.toml", RUST_ANALYZER_TOML)?;
    write(&target_dir, "README.md", &readme(&v))?;

    eprintln!(
        "Created Whisker app at {}\n\
         \n\
         Next steps:\n  \
         1. cd {}\n  \
         2. whisker run ios      # requires Xcode + iOS simulator\n  \
         3. whisker run android  # requires Android SDK + emulator\n  \
         \n\
         Run `whisker doctor` first to verify your toolchain.",
        target_dir.display(),
        target_dir.display(),
    );
    Ok(())
}

// ============================================================================
// Template variables + rendering
// ============================================================================

struct Vars<'a> {
    /// Cargo crate name, e.g. `my-app`.
    crate_name: &'a str,
    /// Human-readable display name shown in the app launcher.
    display_name: &'a str,
    /// Reverse-DNS bundle id / applicationId.
    bundle_id: &'a str,
}

fn write(root: &Path, rel: &str, content: &str) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn cargo_toml(v: &Vars) -> String {
    format!(
        r#"# `{name}` — a Whisker app. See README.md for usage.
#
# Single-crate workspace: the same `Cargo.toml` carries `[package]`
# for cargo's package resolution and `[workspace]` so `whisker run`
# can find a workspace root. Add sibling crates by listing them in
# `workspace.members`.

[workspace]
members = ["."]
resolver = "2"

[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["rlib"]

[dependencies]
whisker = "{whisker_version}"
"#,
        name = v.crate_name,
        whisker_version = whisker_dep_version(),
    )
}

/// The `whisker` version a freshly-scaffolded app should depend on: the
/// CLI's own `major.minor`. release-plz bumps every workspace crate in
/// lockstep, so the CLI version always matches the published `whisker`
/// crate — this keeps `whisker new` from pinning a stale version (e.g. a
/// `0.1` scaffold while crates.io is already on `0.2`).
fn whisker_dep_version() -> String {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("1");
    format!("{major}.{minor}")
}

fn lib_rs(v: &Vars) -> String {
    let display = v.display_name;
    format!(
        r##"//! {display} — a Whisker app.

use whisker::css::{{AlignItems, Color, Display, FlexDirection, FontWeight, JustifyContent}};
use whisker::prelude::*;
use whisker::runtime::view::Element;

// `#[whisker::main]` is the app entry point. Keep it thin — it just
// mounts the root component. Whisker provides the root `page` element
// for you, so `app()` (and your components) just return a `view`.
//
// Splitting your UI into `#[component]`s (rather than writing everything
// here) is what lets `whisker run` hot-reload your edits in well under a
// second: edit `Root` below, hit save, and the running app updates live
// with its state preserved.
#[whisker::main]
fn app() -> Element {{
    render! {{
        Root
    }}
}}

/// The root of your app — put your state and layout here. Whisker wraps
/// whatever `Root` returns in the full-screen root `page`, so this `view`
/// just needs `flex_grow: 1.0` to fill it. Editing `Root` hot-reloads.
#[component]
fn root() -> Element {{
    // `RwSignal` holds reactive state. Anything that reads it (like the
    // `computed` below) re-runs and repaints when it changes.
    let count = RwSignal::new(0);

    render! {{
        // Styles use the typed `css!` macro: field names map to CSS
        // properties and values are checked at compile time. Reach for
        // `.raw("prop", "value")` for anything the typed builder doesn't
        // cover yet (here: `gap` and the gradient `background`).
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: px(24),
            background_color: Color::hex(0x0b0b0f),
        )) {{
            // A card: column layout, padding, rounded corners, plus a
            // `gap` and a linear-gradient background via `.raw(...)`.
            view(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: px(32),
                border_radius: px(20),
            )
            .raw("gap", "6px")
            .raw("background", "linear-gradient(135deg, #7c5cff 0%, #4e9bff 100%)")) {{
                text(
                    value: "{display}",
                    style: css!(
                        color: Color::hex(0xffffff),
                        font_size: px(22),
                        font_weight: FontWeight::Bold,
                    )
                    .raw("letter-spacing", "0.5px"),
                )
                text(
                    value: "Edit `Root` and save — hot reload in under a second",
                    style: css!(color: Color::rgba(255, 255, 255, 0.85), font_size: px(13)),
                )
                text(
                    value: computed(move || format!("{{}}", count.get())),
                    style: css!(
                        color: Color::hex(0xffffff),
                        font_size: px(56),
                        font_weight: FontWeight::Numeric(800),
                        margin_top: px(12),
                    ),
                )
                // A horizontal row; `gap` separates the two buttons.
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    margin_top: px(16),
                )
                .raw("gap", "12px")) {{
                    Button(label: "-1", delta: -1, count: count)
                    Button(label: "+1", delta: 1, count: count)
                }}
            }}
        }}
    }}
}}

/// A small reusable button. Passing the `count` signal in as a prop lets
/// it update state owned by `Root`, and keeps `Root` readable. Each
/// `#[component]` also hot-reloads on its own.
#[component]
fn button(label: &'static str, delta: i32, count: RwSignal<i32>) -> Element {{
    render! {{
        view(
            style: css!(
                border_radius: px(12),
                background_color: Color::rgba(255, 255, 255, 0.18),
            )
            .raw("padding", "12px 22px"),
            on_tap: move |_| count.set(count.get() + delta),
        ) {{
            text(
                value: label,
                style: css!(
                    color: Color::hex(0xffffff),
                    font_size: px(18),
                    font_weight: FontWeight::Numeric(600),
                ),
            )
        }}
    }}
}}
"##
    )
}

fn whisker_rs(v: &Vars) -> String {
    format!(
        r#"// `whisker.rs` — Whisker app configuration.
//
// `whisker run` compiles this file as a tiny probe binary that
// serializes the resulting `Config` to JSON; the CLI reads that
// JSON and projects it into the dev-server's flat `Config`.

pub fn configure(app: &mut whisker_config::Config) {{
    app.name("{display}")
        .bundle_id("{bundle_id}")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {{
        a.package("{bundle_id}")
            .application_id("{bundle_id}")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    }});

    app.ios(|i| {{
        i.bundle_id("{bundle_id}")
            .scheme("{display}")
            .deployment_target("13.0");
    }});
}}
"#,
        display = v.display_name,
        bundle_id = v.bundle_id,
    )
}

const GITIGNORE: &str = "\
# Cargo build artifacts.
target/

# rustfmt backup files (older toolchains left these behind on a failed
# format pass; harmless to keep ignored).
**/*.rs.bk

# Whisker-generated host projects — refreshed on every `whisker run`.
# Includes gradle's `.gradle/` + `build/` caches and xcodebuild's
# `xcuserdata/` / `*.xcuserstate` under here, so no need to list those
# separately.
gen/

# Environment / secrets. Copy the pattern (e.g. `.env.example`) when
# you need to share a template across the team without committing the
# real values.
.env
.env.local
.env.*.local

# IDE / editor noise.
.idea/
.vscode/
*.iml
.vs/

# OS noise.
.DS_Store
Thumbs.db

# NOTE: `Cargo.lock` is deliberately NOT ignored. A Whisker user crate
# is shaped like an application (compiled into the device-side dylib),
# so the lock file is what guarantees every CI / teammate / production
# build resolves to the same dependency tree. Commit it.
";

/// Editor-agnostic rust-analyzer project config. Any rust-analyzer
/// host (VS Code, Zed, neovim, Helix, …) reads `rust-analyzer.toml`, so
/// format-on-save routes through `whisker fmt` for everyone — a rustfmt
/// drop-in that also formats the `render!` / `css!` macro bodies rustfmt
/// leaves untouched (and still honors your `rustfmt.toml`).
const RUST_ANALYZER_TOML: &str = "\
# rust-analyzer project configuration (committed: shared by the whole
# team, editor-agnostic). Formats on save with `whisker fmt`, a rustfmt
# drop-in that ALSO formats `render!` / `css!` macro bodies — which plain
# rustfmt skips — while still honoring your `rustfmt.toml`.
#
# Requires the `whisker` CLI on PATH (`cargo install whisker-cli`).
[rustfmt]
overrideCommand = [\"whisker\", \"fmt\", \"--stdin\"]
";

fn readme(v: &Vars) -> String {
    format!(
        r##"# {display}

A [Whisker](https://github.com/whiskerrs/whisker) app.

## Develop

```sh
# On an iOS Simulator (macOS only).
whisker run ios

# On an Android device or emulator.
whisker run android
```

Run `whisker doctor` first to verify your toolchain is set up for each
target.

## Edit

The UI lives in [`src/lib.rs`](src/lib.rs). Save any change and
`whisker run` hot-patches the running app in under a second — no
restart, no state loss.

App-level metadata (bundle id, app name, Android / iOS deployment
settings) lives in [`whisker.rs`](whisker.rs). Edits there require
a full `whisker run` restart since they shape the generated native
project.

## Build for release

Whisker doesn't wrap release builds — drive xcodebuild / gradle the
same way CI does:

```sh
# Android release APK
( cd gen/android && ./gradlew :app:assembleRelease )

# iOS Simulator .app (Release configuration)
xcodebuild -project gen/ios/<Scheme>.xcodeproj \
  -scheme <Scheme> -configuration Release \
  -destination 'generic/platform=iOS Simulator' build
```

The `gen/` tree is refreshed automatically on every `whisker run`;
delete it whenever you want a clean re-generate.
"##,
        display = v.display_name,
    )
}

// ============================================================================
// Name validation + derivations
// ============================================================================

/// Reject crate names cargo wouldn't accept. Whisker doesn't add any
/// constraints on top — the goal is to fail fast with a helpful
/// message rather than letting `cargo build` print the same complaint
/// after the scaffold landed on disk.
fn validate_crate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("crate name is empty"));
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err(anyhow!(
            "crate name must start with an ASCII letter (got `{first}`)"
        ));
    }
    for c in name.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(anyhow!(
                "crate name contains illegal character `{c}` — allowed: ASCII letters, digits, `-`, `_`"
            ));
        }
    }
    Ok(())
}

/// Title-case the crate name for the display surface. `my-app` →
/// `My App`. Underscore behaves like a dash.
fn derive_display_name(crate_name: &str) -> String {
    crate_name
        .split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-process monotonic counter for tempdir suffixes. Keeps
    /// concurrent test runs from racing on the same `<name>` dir.
    fn test_seq() -> u64 {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        SEQ.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn validate_accepts_simple_kebab_name() {
        validate_crate_name("my-app").unwrap();
        validate_crate_name("a").unwrap();
        validate_crate_name("foo_bar").unwrap();
        validate_crate_name("v2").unwrap();
    }

    #[test]
    fn validate_rejects_empty_and_non_letter_lead() {
        assert!(validate_crate_name("").is_err());
        assert!(validate_crate_name("1app").is_err());
        assert!(validate_crate_name("-app").is_err());
        assert!(validate_crate_name("_app").is_err());
    }

    #[test]
    fn validate_rejects_illegal_chars() {
        assert!(validate_crate_name("my app").is_err()); // space
        assert!(validate_crate_name("my.app").is_err()); // dot
        assert!(validate_crate_name("my/app").is_err()); // slash
        assert!(validate_crate_name("café").is_err()); // non-ASCII
    }

    #[test]
    fn display_name_title_cases_kebab_segments() {
        assert_eq!(derive_display_name("my-app"), "My App");
        assert_eq!(
            derive_display_name("awesome-thing-pro"),
            "Awesome Thing Pro"
        );
        assert_eq!(derive_display_name("hello_world"), "Hello World");
        assert_eq!(derive_display_name("single"), "Single");
    }

    #[test]
    fn display_name_skips_empty_segments() {
        // Double-dash or trailing dash shouldn't produce a doubled
        // space — `split` with a predicate filters to non-empty.
        assert_eq!(derive_display_name("a--b"), "A B");
        assert_eq!(derive_display_name("a-"), "A");
    }

    #[test]
    fn scaffold_creates_expected_files() {
        let tmp = std::env::temp_dir().join(format!(
            "whisker-new-test-{}-{}",
            std::process::id(),
            // No `Instant::now` in cfg(test) constraints; a thread-id
            // nibble is enough entropy for sequential test runs.
            test_seq()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let args = NewAppArgs {
            name: "demo-app".into(),
            path: Some(tmp.clone()),
            bundle_id: None,
            display_name: None,
        };
        run(args).unwrap();

        let root = tmp.join("demo-app");
        assert!(root.join("Cargo.toml").is_file());
        assert!(root.join("src/lib.rs").is_file());
        assert!(root.join("whisker.rs").is_file());
        assert!(root.join(".gitignore").is_file());
        assert!(root.join("README.md").is_file());
        assert!(root.join("rust-analyzer.toml").is_file());

        // The scaffolded rust-analyzer config routes format-on-save
        // through `whisker fmt` so render!/css! get formatted.
        let ra = std::fs::read_to_string(root.join("rust-analyzer.toml")).unwrap();
        assert!(ra.contains("[rustfmt]"));
        assert!(ra.contains(r#"overrideCommand = ["whisker", "fmt", "--stdin"]"#));

        let cargo = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"demo-app\""));
        assert!(cargo.contains("[workspace]"));
        // Tracks the CLI's own major.minor (release-plz bumps in lockstep),
        // so this stays correct across version bumps.
        assert!(cargo.contains(&format!("whisker = \"{}\"", super::whisker_dep_version())));

        let whisker_rs = std::fs::read_to_string(root.join("whisker.rs")).unwrap();
        // Default display name + bundle id are derived.
        assert!(whisker_rs.contains("Demo App"));
        assert!(whisker_rs.contains("rs.example.demo_app"));

        let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        // Sanity-check the load-bearing entries — losing either of
        // these would surface as committed `target/` artifacts or a
        // committed `gen/` tree on the user's first push.
        assert!(
            gitignore.contains("target/"),
            "missing target/ in .gitignore",
        );
        assert!(gitignore.contains("gen/"), "missing gen/ in .gitignore");
        // `Cargo.lock` must NOT be ignored — a Whisker app's lock
        // file pins the dep tree the CI / production build resolves.
        assert!(
            !gitignore.lines().any(|l| l.trim() == "Cargo.lock"),
            ".gitignore must NOT exclude Cargo.lock",
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn scaffold_respects_overrides() {
        let tmp = std::env::temp_dir().join(format!(
            "whisker-new-overrides-{}-{}",
            std::process::id(),
            test_seq()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let args = NewAppArgs {
            name: "custom".into(),
            path: Some(tmp.clone()),
            bundle_id: Some("com.example.custom".into()),
            display_name: Some("Custom Display".into()),
        };
        run(args).unwrap();

        let whisker_rs = std::fs::read_to_string(tmp.join("custom/whisker.rs")).unwrap();
        assert!(whisker_rs.contains("Custom Display"));
        assert!(whisker_rs.contains("com.example.custom"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn scaffold_refuses_to_clobber_existing_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "whisker-new-clobber-{}-{}",
            std::process::id(),
            test_seq()
        ));
        std::fs::create_dir_all(tmp.join("existing")).unwrap();
        let args = NewAppArgs {
            name: "existing".into(),
            path: Some(tmp.clone()),
            bundle_id: None,
            display_name: None,
        };
        let err = run(args).unwrap_err();
        assert!(err.to_string().contains("already exists"));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
