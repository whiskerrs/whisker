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
//! path. The default points at the VS Code extension's bundled
//! server.

#![cfg(test)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_RA_BINARY: &str =
    "/Users/itome/.vscode/extensions/rust-lang.rust-analyzer-0.3.2896-darwin-arm64/server/rust-analyzer";

// ----- Test cases ---------------------------------------------------------

#[test]
fn partial_kwarg_in_render_completes_builder_methods() {
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::ElementHandle;

fn probe() -> ElementHandle {
    render! {
        view(sty|)
    }
}
"#;
    let labels = run_probe("partial_kwarg_in_render", source);
    assert!(
        labels.contains(&"style".to_string()),
        "expected `style` in completions; got {labels:?}"
    );
    assert!(
        labels.contains(&"class".to_string()),
        "expected `class` in completions; got {labels:?}"
    );
}

#[test]
fn partial_kwarg_inside_component_body_completes_builder_methods() {
    // The non-trivial case: the render! is inside a #[component]
    // body, which the proc-macro rewrites into nested closures.
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::ElementHandle;

#[component]
fn probe() -> ElementHandle {
    render! {
        view(sty|)
    }
}
"#;
    let labels = run_probe("partial_kwarg_in_component", source);
    assert!(
        labels.contains(&"style".to_string()),
        "kwarg completion must work inside #[component]; got {labels:?}"
    );
}

#[test]
fn partial_tag_name_at_root_completes_to_builtin_view() {
    // Regression test for tag-name completion. `vie|` at the
    // root of a render! should surface `view` (built-in tag).
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::ElementHandle;

fn probe() -> ElementHandle {
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
fn partial_tag_name_in_children_block_completes_to_builtin_view() {
    let source = r#"
use whisker::prelude::*;
use whisker::runtime::view::ElementHandle;

fn probe() -> ElementHandle {
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

    let ra_binary = std::env::var("RA_BINARY").unwrap_or_else(|_| DEFAULT_RA_BINARY.to_string());

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
