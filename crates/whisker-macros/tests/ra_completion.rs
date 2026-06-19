//! Integration tests that drive rust-analyzer end-to-end and
//! assert on the completion items it returns at marked cursor
//! positions in `render!`-using source code.
//!
//! Each `#[test]` writes a source snippet (with `|` marking the
//! cursor) into a fixture cargo project under `target/whisker-
//! macros-ra-tests/<test-name>/`, spawns rust-analyzer, sends
//! enough of the LSP protocol to request completion at the
//! marker, and asserts on the returned labels.
//!
//! ## Why this exists
//!
//! Unit tests on the macro's emitted token stream prove the
//! shape of the expansion but say nothing about whether RA can
//! thread its completion through it. That gap let two real
//! regressions slip past macro unit-tests:
//!
//! 1. `view(sty|)` partial-kwarg completion broke when the macro
//!    fell back to an `ident_ref` side block because RA injects
//!    a sentinel suffix at the cursor and the prefix-match
//!    heuristic stopped matching.
//! 2. (open) `vie|` tag-name completion at child position
//!    doesn't surface any candidates because the macro emits a
//!    `UserComponent`-shaped call that RA can't resolve.
//!
//! These tests reproduce both directly. Adding a test before
//! changing the macro / runtime keeps us from re-breaking them.
//!
//! ## Running
//!
//! ```sh
//! cargo test -p whisker-macros --test ra_completion
//! ```
//!
//! `RA_BINARY` (env var) overrides the rust-analyzer binary
//! path. If unset, the test downloads a pinned release binary
//! into `target/whisker-macros-ra-tests/ra-binary/<VERSION>/`
//! and reuses it across runs. Bump `RA_VERSION` below to update.

#![cfg(test)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

/// Pinned rust-analyzer release tag. Bump to update.
///
/// Releases live at <https://github.com/rust-lang/rust-analyzer/releases>
/// and use date-stamped tags (the maintainers cut one most
/// Mondays). When bumping, delete
/// `target/whisker-macros-ra-tests/ra-binary/` to force a
/// fresh download — the cache key is the version string.
const RA_VERSION: &str = "2026-05-18";

// ----- Test cases ---------------------------------------------------------

/// Whether RA surfaced a completion for builder method `name`.
///
/// The built-in builder methods (`style`, `class`, `on_tap`, …) live
/// on the `ElementBuilder` trait now, and RA labels trait-provided
/// methods as `name(as ElementBuilder)` to disambiguate them from
/// inherent ones — selecting either inserts `name`. So accept the
/// bare name or the trait-qualified form.
fn surfaces_method(labels: &[String], name: &str) -> bool {
    labels
        .iter()
        .any(|l| l == name || l.starts_with(&format!("{name}(")))
}

#[test]
fn partial_kwarg_in_render_completes_builder_methods() {
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

fn probe() -> Element {
    render! {
        view(sty|)
    }
}
"#;
    let labels = run_probe("partial_kwarg_in_render", source);
    assert!(
        surfaces_method(&labels, "style"),
        "expected `style` in completions; got {labels:?}"
    );
    assert!(
        surfaces_method(&labels, "class"),
        "expected `class` in completions; got {labels:?}"
    );
}

#[test]
fn partial_kwarg_inside_component_body_completes_builder_methods() {
    // The non-trivial case: the render! is inside a #[component]
    // body, which the proc-macro rewrites into nested closures.
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
fn probe() -> Element {
    render! {
        view(sty|)
    }
}
"#;
    let labels = run_probe("partial_kwarg_in_component", source);
    assert!(
        surfaces_method(&labels, "style"),
        "kwarg completion must work inside #[component]; got {labels:?}"
    );
}

#[test]
fn partial_tag_name_at_root_completes_to_builtin_view() {
    // Regression test for tag-name completion. `vie|` at the
    // root of a render! should surface `view` (built-in tag).
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

fn probe() -> Element {
    render! {
        vie|
    }
}
"#;
    let labels = run_probe("partial_tag_root", source);
    assert!(
        labels.iter().any(|l| l == "view"),
        "expected `view` in completions for partial `vie|`; got {labels:?}"
    );
}

#[test]
fn props_builder_helper_types_are_hidden_from_completion() {
    // typed-builder generates a handful of marker types per Props
    // (`<Name>PropsBuilder<((),(),()...)>` and friends). They're
    // technically `pub` but pure plumbing — RA shouldn't surface
    // them at user call sites because they pollute the candidate
    // list with `ArtTilePropsBuilder_*` entries on every keystroke.
    // The `#[component]` macro tucks the Props struct behind a
    // `#[doc(hidden)] pub mod __<name>_props_internal` to gate
    // those helpers off; only `ArtTileProps` itself re-exports
    // outward.
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
fn art_tile(label: &'static str) -> Element {
    render! { text(value: label) }
}

fn probe() -> Element {
    render! {
        view() {
            ArtTile|
        }
    }
}
"#;
    let labels = run_probe("hidden_builder_helpers", source);
    eprintln!(
        "[hidden_builder_helpers] full label set ({} items): {labels:?}",
        labels.len()
    );
    let art_prefixed: Vec<String> = labels
        .iter()
        .filter(|l| l.to_lowercase().starts_with("art"))
        .cloned()
        .collect();
    eprintln!("[hidden_builder_helpers] Art* candidates: {art_prefixed:?}");

    assert!(
        !art_prefixed.iter().any(|l| l == "art_tile"),
        "snake_case `art_tile` should be hidden inside the inner module; \
         got: {art_prefixed:?}",
    );
    let leaked: Vec<&String> = art_prefixed
        .iter()
        .filter(|l| l.starts_with("ArtTilePropsBuilder"))
        .collect();
    assert!(
        leaked.is_empty(),
        "ArtTilePropsBuilder* types should be hidden inside the \
         internal props module; saw: {leaked:?}",
    );
}

#[test]
fn longer_prefix_does_not_surface_builder_via_autoimport() {
    // RA's auto-import path may surface `pub` items inside private
    // modules when the user has typed enough characters to uniquely
    // identify the target. Probe at `ArtTilePropsBu|` — if Builder
    // is properly `#[doc(hidden)]` and inside a `#[doc(hidden)]`
    // private mod, RA must not offer to auto-import it.
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
fn art_tile(label: &'static str) -> Element {
    render! { text(value: label) }
}

fn probe() -> Element {
    render! {
        view() {
            ArtTilePropsBu|
        }
    }
}
"#;
    let labels = run_probe("longer_prefix_builder_leak", source);
    eprintln!(
        "[longer_prefix_builder_leak] {} candidates: {labels:?}",
        labels.len()
    );
    let leaked: Vec<&String> = labels
        .iter()
        .filter(|l| l.contains("ArtTilePropsBuilder") || l.contains("PropsBuilder"))
        .collect();
    assert!(
        leaked.is_empty(),
        "Builder must stay hidden even at long matching prefixes; \
         leaked: {leaked:?}",
    );
}

#[test]
fn short_prefix_user_component_does_not_leak_builder_helpers() {
    // VS Code users typically start typing with just a few chars
    // (`Art|`). RA's fuzzy-match may include items the strict
    // prefix path filters out — let's verify the leak set stays
    // empty at 3 chars too.
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
fn art_tile(label: &'static str) -> Element {
    render! { text(value: label) }
}

fn probe() -> Element {
    render! {
        view() {
            Art|
        }
    }
}
"#;
    let labels = run_probe("short_prefix_leak", source);
    eprintln!(
        "[short_prefix_leak] {} candidates returned: {labels:?}",
        labels.len()
    );
    // The critical items that MUST stay hidden:
    //
    // 1. snake_case fn `art_tile` — user calls via PascalCase alias.
    // 2. builder struct `ArtTilePropsBuilder` — reached only via
    //    `ArtTileProps::builder()`, never by name.
    // 3. typed-builder-style markers (`ArtTilePropsBuilder_Error_*`,
    //    `ArtTilePropsBuilder_Repeated_*`) — must stay completely
    //    gone after the hand-rolled builder migration.
    //
    // The inner module *paths* (`__art_tile_inner::`,
    // `__art_tile_props_internal::`) DO still appear as path
    // completion entries because they're the names of (private)
    // submodules of the user's crate. The items inside them are
    // properly unreachable as bare identifiers.
    let leaked: Vec<&String> = labels
        .iter()
        .filter(|l| {
            l.as_str() == "art_tile"
                || l.as_str() == "ArtTilePropsBuilder"
                || l.starts_with("ArtTilePropsBuilder_")
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "user-facing builder + snake_case fn + typed-builder helpers \
         must stay hidden even at short prefixes; leaked: {leaked:?}",
    );
}

#[test]
fn partial_user_component_completes_to_pascal_case_alias() {
    // `Art|` in a render! children block should surface the
    // `ArtTile` PascalCase alias the `#[component]` macro emits
    // for `fn art_tile`. Without that alias, only `art_tile` is
    // in scope and `Art…` matches nothing.
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
fn art_tile(label: &'static str) -> Element {
    render! { text(value: label) }
}

fn probe() -> Element {
    render! {
        view() {
            Art|
        }
    }
}
"#;
    let labels = run_probe("partial_user_component", source);
    assert!(
        labels.iter().any(|l| l == "ArtTile"),
        "expected `ArtTile` PascalCase alias for `fn art_tile`; got {labels:?}"
    );
}

#[test]
fn partial_tag_name_in_children_block_completes_to_builtin_view() {
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::Element;

fn probe() -> Element {
    render! {
        view() {
            vie|
        }
    }
}
"#;
    let labels = run_probe("partial_tag_child", source);
    assert!(
        labels.iter().any(|l| l == "view"),
        "expected `view` in child-position completions for `vie|`; got {labels:?}"
    );
}

// ----- rust-analyzer binary provisioning ---------------------------------

/// Return the path to a usable `rust-analyzer` binary. Preference
/// order:
///
/// 1. `$RA_BINARY` — explicit override (CI, local hacking).
/// 2. Cached pinned download under
///    `target/whisker-macros-ra-tests/ra-binary/<RA_VERSION>/`.
/// 3. Fresh download from GitHub releases, gunzipped to (2).
///
/// Downloads use `curl` + `gzip` to avoid pulling HTTP and
/// compression crates into the test build. Windows isn't
/// supported (RA releases are `.zip` there; the user can set
/// `$RA_BINARY` explicitly).
fn ensure_ra_binary() -> Result<PathBuf, String> {
    // Memoise across tests in the same process — cargo test runs
    // each test on a thread, all four would otherwise race and
    // step on each other (one curl finishes, another gzip fails
    // because the .gz disappeared mid-decompress; or an exec
    // races a still-writing rename and gets "Text file busy").
    // Serialising via OnceLock means the first thread provisions
    // and the rest reuse the cached PathBuf without touching the
    // filesystem.
    static CACHED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    CACHED.get_or_init(provision_ra_binary).clone()
}

fn provision_ra_binary() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("RA_BINARY") {
        return Ok(PathBuf::from(p));
    }

    let cache_dir = workspace_root()
        .join("target")
        .join("whisker-macros-ra-tests")
        .join("ra-binary")
        .join(RA_VERSION);
    let binary_path = cache_dir.join("rust-analyzer");

    if binary_path.is_file() {
        return Ok(binary_path);
    }

    let target_triple = ra_release_triple()?;
    let url = format!(
        "https://github.com/rust-lang/rust-analyzer/releases/download/\
         {RA_VERSION}/rust-analyzer-{target_triple}.gz",
    );

    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("create {}: {e}", cache_dir.display()))?;

    // Download to a temp filename, gunzip it, then atomically
    // rename into place. If a concurrent run (different process,
    // or some other tool) wins the rename race, the second
    // rename returns AlreadyExists and we just reuse what's
    // there. This makes the function safe to call from multiple
    // processes hitting the same `target/` cache.
    let pid = std::process::id();
    let gz_path = cache_dir.join(format!("rust-analyzer.{pid}.gz"));
    let staged_path = cache_dir.join(format!("rust-analyzer.{pid}"));

    eprintln!("[ra_completion] downloading {url}");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(&gz_path)
        .arg(&url)
        .status()
        .map_err(|e| format!("spawn curl: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&gz_path);
        return Err(format!(
            "curl failed (exit {status:?}) — check that `{RA_VERSION}` is \
             a real rust-analyzer release tag and your network is reachable"
        ));
    }

    // `gzip -d` writes the decompressed file next to the .gz
    // (stripping `.gz`) and removes the original. We named the
    // input `rust-analyzer.<pid>.gz` so the output is
    // `rust-analyzer.<pid>` (`staged_path`) — keeps concurrent
    // provisioning from clobbering each other.
    let status = Command::new("gzip")
        .arg("-df")
        .arg(&gz_path)
        .status()
        .map_err(|e| format!("spawn gzip: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&gz_path);
        let _ = std::fs::remove_file(&staged_path);
        return Err(format!("gzip -d failed (exit {status:?})"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&staged_path)
            .map_err(|e| format!("stat {}: {e}", staged_path.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&staged_path, perms)
            .map_err(|e| format!("chmod {}: {e}", staged_path.display()))?;
    }

    // Atomic publish. If another process already published a
    // binary, treat that as success and drop our staged copy.
    match std::fs::rename(&staged_path, &binary_path) {
        Ok(()) => Ok(binary_path),
        Err(_) if binary_path.is_file() => {
            let _ = std::fs::remove_file(&staged_path);
            Ok(binary_path)
        }
        Err(e) => Err(format!(
            "rename {} → {}: {e}",
            staged_path.display(),
            binary_path.display()
        )),
    }
}

/// Map the host's (arch, os) to the rust-analyzer release asset
/// triple. Asset names look like `rust-analyzer-<triple>.gz`.
fn ra_release_triple() -> Result<&'static str, String> {
    Ok(match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        (arch, os) => {
            return Err(format!(
                "unsupported host (arch={arch}, os={os}) for automated \
                 rust-analyzer download — set $RA_BINARY to an existing \
                 binary instead"
            ));
        }
    })
}

// ----- Fixture project bootstrap -----------------------------------------

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `crates/whisker-macros`; the workspace
    // root is two levels above.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn fixture_dir(test_name: &str) -> PathBuf {
    workspace_root()
        .join("target")
        .join("whisker-macros-ra-tests")
        .join(test_name)
}

/// Create the minimal cargo project the test source lives in.
/// Path-deps point at whisker in the surrounding workspace so the
/// fixture doesn't have to vendor anything.
fn write_fixture_project(test_name: &str, source: &str) -> PathBuf {
    let dir = fixture_dir(test_name);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create fixture src dir");

    let whisker_path = workspace_root().join("crates").join("whisker");
    let cargo_toml = format!(
        "[package]\nname = \"probe-{test_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\
         [lib]\npath = \"src/lib.rs\"\n\
         [dependencies]\nwhisker = {{ path = \"{whisker}\" }}\n\
         [workspace]\n",
        whisker = whisker_path.display(),
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    std::fs::write(src.join("lib.rs"), source).expect("write lib.rs");
    dir
}

/// Full pipeline for one assertion: build the fixture, drive RA
/// over LSP, request completion at the `|` marker, return the
/// item labels.
fn run_probe(test_name: &str, source_with_marker: &str) -> Vec<String> {
    let project = write_fixture_project(test_name, source_with_marker);
    let lib_rs = project.join("src").join("lib.rs");
    let (text, line, character) =
        locate_marker(source_with_marker, '|').expect("source must contain one `|` marker");
    // Overwrite the on-disk file with the marker stripped so it
    // type-checks (the marker would be a parse error otherwise).
    std::fs::write(&lib_rs, &text).expect("rewrite lib.rs without marker");

    let ra_binary = ensure_ra_binary().expect("provision rust-analyzer");
    let ra_binary = ra_binary.to_string_lossy().into_owned();

    let mut driver = LspDriver::start(&ra_binary, &project).expect("start rust-analyzer");
    driver.initialize().expect("initialize");
    driver.did_open(&lib_rs, &text).expect("didOpen");
    driver
        .wait_for_server_ready()
        .expect("server failed to reach quiescent=true");
    let items = driver
        .completion_with_retry(&lib_rs, line, character, 5)
        .expect("completion");
    driver.shutdown();
    items.into_iter().map(|i| i.label).collect()
}

// ----- LSP driver --------------------------------------------------------

struct LspDriver {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: AtomicI64,
    project_uri: String,
    timeout: Duration,
}

impl LspDriver {
    fn start(ra_binary: &str, project: &Path) -> Result<Self, String> {
        let mut child = Command::new(ra_binary)
            .current_dir(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn {ra_binary}: {e}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: AtomicI64::new(1),
            project_uri: path_to_uri(project),
            timeout: Duration::from_secs(180),
        })
    }

    fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn initialize(&mut self) -> Result<(), String> {
        let id = self.next_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "initialize",
                "params": {
                    "processId": std::process::id(),
                    "rootUri": self.project_uri,
                    "capabilities": {
                        "textDocument": {
                            "completion": {
                                "completionItem": { "snippetSupport": false }
                            }
                        },
                        "window": { "workDoneProgress": true },
                        "experimental": { "serverStatusNotification": true }
                    },
                    "initializationOptions": {
                        "cargo": { "buildScripts": { "enable": true } },
                        "procMacro": { "enable": true },
                        "diagnostics": { "enable": false }
                    },
                    "workspaceFolders": [{
                        "uri": self.project_uri,
                        "name": "probe-fixture"
                    }]
                }
            }),
        )?;
        self.wait_for_response(id)?;
        write_msg(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        )?;
        Ok(())
    }

    fn did_open(&mut self, file: &Path, text: &str) -> Result<(), String> {
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": path_to_uri(file),
                        "languageId": "rust",
                        "version": 1,
                        "text": text
                    }
                }
            }),
        )
    }

    fn wait_for_server_ready(&mut self) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if start.elapsed() > self.timeout {
                return Err("timeout waiting for serverStatus ready".to_string());
            }
            let msg = read_msg(&mut self.stdout)?;
            if msg.get("method").and_then(|m| m.as_str()) == Some("experimental/serverStatus") {
                let quiescent = msg
                    .get("params")
                    .and_then(|p| p.get("quiescent"))
                    .and_then(|q| q.as_bool())
                    .unwrap_or(false);
                if quiescent {
                    return Ok(());
                }
            }
        }
    }

    fn completion_with_retry(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
        attempts: u32,
    ) -> Result<Vec<CompletionItem>, String> {
        let mut last = Vec::new();
        for attempt in 1..=attempts {
            let id = self.next_id();
            write_msg(
                &mut self.stdin,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "textDocument/completion",
                    "params": {
                        "textDocument": { "uri": path_to_uri(file) },
                        "position": { "line": line, "character": character },
                        "context": { "triggerKind": 1 }
                    }
                }),
            )?;
            let response = self.wait_for_response(id)?;
            last = extract_completion_items(&response)?;
            if !last.is_empty() || attempt == attempts {
                return Ok(last);
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
        Ok(last)
    }

    fn wait_for_response(&mut self, id: i64) -> Result<Value, String> {
        let start = Instant::now();
        loop {
            if start.elapsed() > self.timeout {
                return Err(format!("timeout waiting for response id={id}"));
            }
            let msg = read_msg(&mut self.stdout)?;
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                return Ok(msg);
            }
        }
    }

    fn shutdown(mut self) {
        let _ = write_msg(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "id": 9999, "method": "shutdown"}),
        );
        let _ = write_msg(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "method": "exit"}),
        );
        let _ = self.child.wait();
    }
}

// ----- LSP framing -------------------------------------------------------

fn write_msg<W: Write>(out: &mut W, msg: &Value) -> Result<(), String> {
    let body = serde_json::to_string(msg).map_err(|e| format!("serialize: {e}"))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    out.write_all(header.as_bytes())
        .map_err(|e| format!("write header: {e}"))?;
    out.write_all(body.as_bytes())
        .map_err(|e| format!("write body: {e}"))?;
    out.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

fn read_msg<R: Read>(reader: &mut BufReader<R>) -> Result<Value, String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("read header: {e}"))?;
        if n == 0 {
            return Err("RA closed stdout".to_string());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                rest.trim()
                    .parse()
                    .map_err(|e| format!("bad Content-Length: {e}"))?,
            );
        }
    }
    let n = content_length.ok_or("missing Content-Length header")?;
    let mut body = vec![0u8; n];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("read body: {e}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("parse body: {e}"))
}

// ----- Helpers -----------------------------------------------------------

fn path_to_uri(p: &Path) -> String {
    format!("file://{}", p.display())
}

fn locate_marker(raw: &str, marker: char) -> Result<(String, u32, u32), String> {
    let mut found = None;
    let mut text = String::with_capacity(raw.len());
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    for ch in raw.chars() {
        if ch == marker {
            if found.is_some() {
                return Err(format!("marker `{marker}` appears more than once"));
            }
            found = Some((line, col));
            continue;
        }
        text.push(ch);
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    let (line, character) = found.ok_or(format!("marker `{marker}` not found"))?;
    Ok((text, line, character))
}

#[derive(Debug, Deserialize)]
struct CompletionItem {
    label: String,
}

fn extract_completion_items(response: &Value) -> Result<Vec<CompletionItem>, String> {
    let result = response
        .get("result")
        .ok_or("completion response has no `result`")?;
    let items_value = if result.is_array() {
        result.clone()
    } else if let Some(items) = result.get("items") {
        items.clone()
    } else if result.is_null() {
        Value::Array(Vec::new())
    } else {
        return Err(format!("unexpected completion result shape: {result}"));
    };
    let items: Vec<CompletionItem> =
        serde_json::from_value(items_value).map_err(|e| format!("parse items: {e}"))?;
    Ok(items)
}
