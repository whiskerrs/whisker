//! Programmatic completion tester driving rust-analyzer over LSP.
//!
//! Spawns the same `rust-analyzer` binary the VS Code extension
//! uses, drives the LSP `textDocument/completion` request at a
//! cursor position marked with `|` in a source file, and prints
//! the returned completion items.
//!
//! Usage:
//!
//!     cargo run -p ra-completion-probe -- \
//!         --project <path-to-cargo-project> \
//!         --file <path-to-source-file-relative-to-project> \
//!         --marker '|'
//!
//! The marker character (default `|`) is searched for in the source
//! file; its position becomes the LSP cursor position. The marker
//! is stripped from the in-memory copy of the file we send to RA
//! via `textDocument/didOpen` — the on-disk file isn't touched, so
//! you can edit-and-rerun without cleanup.
//!
//! Defaults to using the rust-analyzer shipped with the VS Code
//! extension. Override with `--ra-binary <path>`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_RA_BINARY: &str =
    "/Users/itome/.vscode/extensions/rust-lang.rust-analyzer-0.3.2896-darwin-arm64/server/rust-analyzer";

#[derive(Debug)]
struct Args {
    project: PathBuf,
    file: PathBuf,
    marker: char,
    ra_binary: PathBuf,
    timeout: Duration,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut project: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut marker = '|';
    let mut ra_binary = PathBuf::from(DEFAULT_RA_BINARY);
    let mut timeout = Duration::from_secs(180);
    let mut verbose = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => {
                project = Some(PathBuf::from(args.next().ok_or("--project needs a value")?));
            }
            "--file" => {
                file = Some(PathBuf::from(args.next().ok_or("--file needs a value")?));
            }
            "--marker" => {
                let v = args.next().ok_or("--marker needs a value")?;
                if v.chars().count() != 1 {
                    return Err(format!("--marker must be one character, got `{v}`"));
                }
                marker = v.chars().next().unwrap();
            }
            "--ra-binary" => {
                ra_binary = PathBuf::from(args.next().ok_or("--ra-binary needs a value")?);
            }
            "--timeout-secs" => {
                let v = args.next().ok_or("--timeout-secs needs a value")?;
                timeout =
                    Duration::from_secs(v.parse().map_err(|e| format!("bad --timeout-secs: {e}"))?);
            }
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }

    Ok(Args {
        project: project.ok_or("--project is required")?,
        file: file.ok_or("--file is required")?,
        marker,
        ra_binary,
        timeout,
        verbose,
    })
}

fn print_help() {
    eprintln!(
        "ra-completion-probe — drive rust-analyzer to test completion programmatically\n\
         \n\
         USAGE:\n    \
            ra-completion-probe --project <PATH> --file <PATH> [--marker CHAR] [--ra-binary PATH]\n\
         \n\
         The file at PATH (relative to PROJECT or absolute) must contain exactly one\n\
         occurrence of MARKER (default `|`). That position becomes the LSP cursor.\n\
         The on-disk file is NOT modified — the marker is stripped only from the\n\
         in-memory copy sent to RA.\n"
    );
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            print_help();
            std::process::exit(2);
        }
    };

    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    // Resolve file path (allow either absolute or relative to project).
    let file_path = if args.file.is_absolute() {
        args.file.clone()
    } else {
        args.project.join(&args.file)
    };
    let file_path = file_path
        .canonicalize()
        .map_err(|e| format!("can't canonicalize {}: {e}", file_path.display()))?;
    let project = args
        .project
        .canonicalize()
        .map_err(|e| format!("can't canonicalize {}: {e}", args.project.display()))?;

    // Read file, locate marker.
    let raw =
        std::fs::read_to_string(&file_path).map_err(|e| format!("read {}: {e}", file_path.display()))?;
    let (text, line, character) = locate_marker(&raw, args.marker)?;
    if args.verbose {
        eprintln!(
            "[probe] cursor at {}:{} (0-indexed) in {}",
            line,
            character,
            file_path.display()
        );
    }

    // Spawn rust-analyzer.
    let mut child = Command::new(&args.ra_binary)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn rust-analyzer at {}: {e}", args.ra_binary.display()))?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);

    let mut next_id = 1_i64;

    // 1. initialize — pass the same procMacro / buildScripts
    // toggles the VS Code extension sets by default, otherwise
    // RA happily indexes the project but skips proc-macro
    // expansion entirely and `render!` becomes opaque (completion
    // would silently return nothing — which is exactly what we
    // saw on first run before adding these options).
    let init_id = next_id;
    next_id += 1;
    let root_uri = path_to_uri(&project);
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": init_id,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "completion": {
                            "completionItem": { "snippetSupport": false }
                        },
                        "synchronization": {}
                    },
                    "window": { "workDoneProgress": true },
                    "experimental": {
                        "serverStatusNotification": true
                    }
                },
                "initializationOptions": {
                    "cargo": {
                        "buildScripts": { "enable": true }
                    },
                    "procMacro": { "enable": true },
                    "diagnostics": { "enable": false }
                },
                "workspaceFolders": [{
                    "uri": root_uri,
                    "name": project.file_name().and_then(|s| s.to_str()).unwrap_or("project")
                }]
            }
        }),
    )?;
    wait_for_response(&mut reader, init_id, args.timeout, args.verbose)?;

    // 2. initialized notification
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )?;

    // 3. didOpen — send the stripped file content
    let file_uri = path_to_uri(&file_path);
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": file_uri,
                    "languageId": "rust",
                    "version": 1,
                    "text": text
                }
            }
        }),
    )?;

    // 4. Wait for RA's `experimental/serverStatus` to flip to
    // "ready" (it sends `quiescent: true, health: "ok"`). Without
    // this, RA can respond to completion before proc-macro
    // expansion is wired up and return an empty list.
    if args.verbose {
        eprintln!("[probe] waiting for serverStatus ready...");
    }
    wait_for_server_ready(&mut reader, args.timeout, args.verbose)?;

    // 5. Retry the completion request up to 5 times with 1s
    // pauses. RA can return empty completions for the first
    // request after quiescent=true if macro expansion is still
    // catching up — empirically a couple of retries pays off
    // for large crates like whisker.
    let mut response = Value::Null;
    for attempt in 1..=5 {
        let compl_id = next_id;
        next_id += 1;
        write_msg(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": compl_id,
                "method": "textDocument/completion",
                "params": {
                    "textDocument": { "uri": file_uri },
                    "position": { "line": line, "character": character },
                    "context": { "triggerKind": 1 }
                }
            }),
        )?;
        if args.verbose {
            eprintln!("[probe] completion attempt {attempt}...");
        }
        response = wait_for_response(&mut reader, compl_id, args.timeout, args.verbose)?;
        // If we got items, stop. Else wait a sec and retry.
        let n = extract_completion_items(&response)
            .map(|v| v.len())
            .unwrap_or(0);
        if args.verbose {
            eprintln!("[probe] attempt {attempt} returned {n} item(s)");
        }
        if n > 0 || attempt == 5 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }

    // 6. Shutdown + exit (best-effort, we don't care about errors here).
    let _ = write_msg(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "id": next_id + 1, "method": "shutdown"}),
    );
    let _ = write_msg(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "exit"}),
    );
    let _ = child.wait();

    // 7. Print completions.
    let items = extract_completion_items(&response)?;
    if items.is_empty() {
        println!("(no completions)");
    } else {
        for item in items {
            print!("{}", item.label);
            if let Some(detail) = item.detail.as_deref() {
                print!("\t{detail}");
            }
            println!();
        }
    }

    Ok(())
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

fn path_to_uri(p: &Path) -> String {
    // rust-analyzer accepts file:// URIs. Percent-encoding gets
    // sketchy with non-ASCII; the paths we run with are ASCII so
    // we go with the minimal form.
    format!("file://{}", p.display())
}

// ---- LSP framing --------------------------------------------------------

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

fn wait_for_server_ready<R: Read>(
    reader: &mut BufReader<R>,
    timeout: Duration,
    verbose: bool,
) -> Result<(), String> {
    let start = Instant::now();
    let mut last_log = Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err("timeout waiting for serverStatus ready".to_string());
        }
        let msg = read_msg(reader)?;
        if msg.get("method").and_then(|m| m.as_str()) == Some("experimental/serverStatus") {
            let params = msg.get("params");
            let quiescent = params
                .and_then(|p| p.get("quiescent"))
                .and_then(|q| q.as_bool())
                .unwrap_or(false);
            let health = params
                .and_then(|p| p.get("health"))
                .and_then(|h| h.as_str())
                .unwrap_or("");
            if verbose {
                eprintln!(
                    "[probe] serverStatus: quiescent={quiescent} health={health}"
                );
            }
            if quiescent {
                return Ok(());
            }
        } else if verbose && last_log.elapsed() > Duration::from_secs(3) {
            let method = msg
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("(no method)");
            eprintln!(
                "[probe] still indexing ({:.1}s) — last msg: {method}",
                start.elapsed().as_secs_f32()
            );
            last_log = Instant::now();
        }
    }
}

fn wait_for_response<R: Read>(
    reader: &mut BufReader<R>,
    id: i64,
    timeout: Duration,
    verbose: bool,
) -> Result<Value, String> {
    let start = Instant::now();
    let mut last_log = Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err(format!("timeout waiting for response id={id}"));
        }
        let msg = read_msg(reader)?;
        if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
            return Ok(msg);
        }
        // Notification (incl. `$/progress`) — log periodically in
        // verbose mode so we can tell RA is still alive without
        // drowning stderr.
        if verbose && last_log.elapsed() > Duration::from_secs(3) {
            let method = msg
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("(no method)");
            eprintln!(
                "[probe] still waiting (id={id}, {:.1}s) — last msg: {method}",
                start.elapsed().as_secs_f32()
            );
            last_log = Instant::now();
        }
    }
}


// ---- Completion response parsing ----------------------------------------

#[derive(Debug, Deserialize)]
struct CompletionItem {
    label: String,
    #[serde(default)]
    detail: Option<String>,
}

fn extract_completion_items(response: &Value) -> Result<Vec<CompletionItem>, String> {
    let result = response
        .get("result")
        .ok_or("completion response has no `result`")?;
    // Result is either CompletionItem[] or { items: CompletionItem[] }.
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
