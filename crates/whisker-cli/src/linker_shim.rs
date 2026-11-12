//! `whisker-linker-shim` — `-C linker=<shim>` target.
//!
//! rustc, when told `-C linker=whisker-linker-shim`, invokes us as
//!
//! ```text
//! whisker-linker-shim <linker-driver-args...>
//! ```
//!
//! There is **no real-linker-path prefix** in argv (rustc treats the
//! shim itself as the linker). To forward the call, we read the real
//! linker path from the `WHISKER_REAL_LINKER` env var. The dev-server
//! sets this when it spawns the build with the shim active; if it
//! isn't set the shim aborts (so a stray shim left in the toolchain
//! configuration after a crashed `whisker run` doesn't silently break
//! ordinary `cargo build` — it fails fast with a clear message).
//!
//! What we capture in JSON:
//!
//! ```text
//! { output, args, timestamp_micros, linker, envs }
//! ```
//!
//! `output` is the value following `-o` (or MSVC's `/OUT:`) in argv.
//! The dev-server keys captured invocations by output filename so the
//! right one can be replayed for the right crate during thin rebuild.
//! `args` has `@response-file` arguments expanded, because rustc
//! deletes those files after the link.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One captured linker invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CapturedLinkerInvocation {
    /// Value passed to `-o` or `/OUT:`, if any. Used as the index key.
    pub output: Option<String>,
    /// Full argv passed to the linker driver, response files expanded —
    /// what we re-invoke during thin rebuild.
    pub args: Vec<String>,
    /// Microseconds since UNIX epoch.
    pub timestamp_micros: u128,
    /// MSVC only: the real linker as found on the `PATH` rustc gave us.
    #[serde(default)]
    pub linker: Option<String>,
    /// MSVC only: the toolchain environment rustc set up for the linker,
    /// which a later direct `link.exe` spawn has no other way to get.
    #[serde(default)]
    pub envs: BTreeMap<String, String>,
}

/// Environment variables `link.exe` needs to find its libraries and tools.
const MSVC_LINK_ENV: &[&str] = &["LIB", "LIBPATH", "PATH"];

/// Entry point called from `src/bin/whisker_linker_shim.rs`.
pub fn run() -> Result<()> {
    let mut argv: Vec<String> = std::env::args().collect();
    if argv.is_empty() {
        anyhow::bail!("whisker-linker-shim: empty argv");
    }
    let _shim_path = argv.remove(0);
    let linker_args = argv;

    if let Some(cache_dir) = std::env::var_os("WHISKER_LINKER_CACHE_DIR") {
        let cache_dir = PathBuf::from(cache_dir);
        let mut invocation = capture(&linker_args)?;
        if cfg!(windows) {
            invocation.linker = std::env::var("WHISKER_REAL_LINKER")
                .ok()
                .and_then(|linker| find_on_path(&linker, std::env::var_os("PATH")))
                .map(|path| path.to_string_lossy().into_owned());
            invocation.envs = MSVC_LINK_ENV
                .iter()
                .filter_map(|key| Some((key.to_string(), std::env::var(key).ok()?)))
                .collect();
        }
        save_invocation(&cache_dir, &invocation)
            .with_context(|| format!("save to {}", cache_dir.display()))?;
    }

    let real_linker = std::env::var("WHISKER_REAL_LINKER").context(
        "WHISKER_REAL_LINKER not set; whisker-linker-shim has nothing to forward to. \
         Did you mean to install the shim in your toolchain config?",
    )?;
    let status = std::process::Command::new(&real_linker)
        .args(&linker_args)
        .status()
        .with_context(|| format!("spawn {real_linker}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

// ----- Pure helpers ---------------------------------------------------------

pub fn capture(linker_args: &[String]) -> Result<CapturedLinkerInvocation> {
    let args = expand_response_files(linker_args);
    Ok(CapturedLinkerInvocation {
        output: extract_output(&args),
        args,
        timestamp_micros: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0),
        linker: None,
        envs: BTreeMap::new(),
    })
}

/// Replace each readable `@file` argument with the arguments it holds.
pub fn expand_response_files(args: &[String]) -> Vec<String> {
    let mut expanded = Vec::with_capacity(args.len());
    for arg in args {
        match arg.strip_prefix('@').map(std::fs::read) {
            Some(Ok(bytes)) => expanded.extend(parse_response_file(&bytes)),
            _ => expanded.push(arg.clone()),
        }
    }
    expanded
}

/// Decode a response file as rustc writes it: one argument per line,
/// UTF-16 with a byte-order mark for MSVC, UTF-8 otherwise.
pub fn parse_response_file(bytes: &[u8]) -> Vec<String> {
    let text = match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(unescape_response_argument)
        .collect()
}

/// MSVC lines are wrapped in quotes with `\"` for a quote; GNU lines
/// escape single characters with a backslash.
fn unescape_response_argument(line: &str) -> String {
    match line
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(quoted) => quoted.replace("\\\"", "\""),
        None => {
            let mut out = String::with_capacity(line.len());
            let mut chars = line.chars();
            while let Some(c) = chars.next() {
                out.push(if c == '\\' {
                    chars.next().unwrap_or(c)
                } else {
                    c
                });
            }
            out
        }
    }
}

/// Resolve `program` against `path` the way the OS would for a spawn.
fn find_on_path(program: &str, path: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.components().count() > 1 {
        return Some(candidate.to_path_buf());
    }
    let with_extension = if candidate.extension().is_none() && cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    };
    std::env::split_paths(&path?)
        .map(|dir| dir.join(&with_extension))
        .find(|full| full.is_file())
}

/// Find the value passed to `-o`, or to MSVC's `/OUT:`. Linker drivers
/// (clang/gcc) always use the **separated** form (`-o /path/lib.so`);
/// the attached form (`-o/path/lib.so`) is technically valid for `ld`
/// but isn't what rustc emits. We deliberately only handle the
/// separated form so we don't false-positive on lookalike flags such
/// as `-output-format=…`.
pub fn extract_output(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-o" {
            return iter.next().cloned();
        }
        if let Some(prefix) = arg.get(..5)
            && prefix.eq_ignore_ascii_case("/OUT:")
        {
            return Some(arg[5..].to_string());
        }
    }
    None
}

/// Best-effort filename slug for a captured invocation: the basename
/// of the `-o` argument minus extension, with non-ascii-alphanumeric
/// characters replaced with `_`. Falls back to `_unknown`.
pub fn invocation_filename(invocation: &CapturedLinkerInvocation) -> String {
    let stem_for_path = invocation
        .output
        .as_deref()
        .and_then(|s| Path::new(s).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("_unknown");
    let safe: String = stem_for_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}-{}.json", safe, invocation.timestamp_micros)
}

pub fn save_invocation(cache_dir: &Path, invocation: &CapturedLinkerInvocation) -> Result<()> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("create {}", cache_dir.display()))?;
    let path = cache_dir.join(invocation_filename(invocation));
    let json = serde_json::to_string_pretty(invocation).context("serialize")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-linker-shim-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // ----- extract_output ---------------------------------------------

    #[test]
    fn extract_output_from_separated_form() {
        let args = s(&["-O3", "-o", "/tmp/libfoo.dylib", "obj.o"]);
        assert_eq!(extract_output(&args).as_deref(), Some("/tmp/libfoo.dylib"));
    }

    #[test]
    fn extract_output_ignores_attached_form() {
        // Deliberately not handled — see extract_output's rationale.
        // If a real driver ever uses attached form we'll hear about it
        // when -o ends up None and the cache file is named "_unknown".
        let args = s(&["-o/tmp/libfoo.dylib", "obj.o"]);
        assert_eq!(extract_output(&args), None);
    }

    #[test]
    fn extract_output_returns_none_when_absent() {
        let args = s(&["obj.o", "-shared"]);
        assert_eq!(extract_output(&args), None);
    }

    #[test]
    fn extract_output_does_not_grab_lookalike_long_flags() {
        // `-output-format=...` shouldn't be treated as `-o`.
        let args = s(&["-output-format=binary", "-o", "/tmp/real.so"]);
        assert_eq!(extract_output(&args).as_deref(), Some("/tmp/real.so"));
    }

    // ----- capture ----------------------------------------------------

    #[test]
    fn capture_preserves_full_argv() {
        let args = s(&[
            "-O3",
            "-shared",
            "-o",
            "/tmp/libfoo.dylib",
            "-Wl,-undefined,dynamic_lookup",
            "/tmp/foo.o",
        ]);
        let inv = capture(&args).unwrap();
        assert_eq!(inv.args, args);
        assert_eq!(inv.output.as_deref(), Some("/tmp/libfoo.dylib"));
        assert!(inv.timestamp_micros > 0);
    }

    #[test]
    fn capture_with_no_output_leaves_field_none() {
        let inv = capture(&s(&["-shared", "obj.o"])).unwrap();
        assert_eq!(inv.output, None);
    }

    // ----- invocation_filename ---------------------------------------

    #[test]
    fn invocation_filename_uses_output_basename_and_timestamp() {
        let inv = CapturedLinkerInvocation {
            output: Some("/tmp/build/libfoo.dylib".into()),
            args: vec![],
            timestamp_micros: 42,
            linker: None,
            envs: Default::default(),
        };
        assert_eq!(invocation_filename(&inv), "libfoo.dylib-42.json");
    }

    #[test]
    fn invocation_filename_handles_anonymous_invocation() {
        let inv = CapturedLinkerInvocation {
            output: None,
            args: vec![],
            timestamp_micros: 7,
            linker: None,
            envs: Default::default(),
        };
        assert_eq!(invocation_filename(&inv), "_unknown-7.json");
    }

    #[test]
    fn invocation_filename_sanitises_weird_characters() {
        // Defensive — actual rustc-produced output paths don't have
        // these, but it's cheap to be tolerant.
        let inv = CapturedLinkerInvocation {
            output: Some("/tmp/foo bar/lib weird?name.so".into()),
            args: vec![],
            timestamp_micros: 1,
            linker: None,
            envs: Default::default(),
        };
        assert_eq!(invocation_filename(&inv), "lib_weird_name.so-1.json");
    }

    // ----- save_invocation --------------------------------------------

    #[test]
    fn save_invocation_writes_and_round_trips() {
        let dir = unique_tempdir();
        let inv = CapturedLinkerInvocation {
            output: Some("/tmp/libfoo.dylib".into()),
            args: s(&["-shared", "-o", "/tmp/libfoo.dylib", "foo.o"]),
            timestamp_micros: 12345,
            linker: None,
            envs: Default::default(),
        };
        save_invocation(&dir, &inv).expect("save");

        let path = dir.join(invocation_filename(&inv));
        assert!(path.is_file());
        let body = std::fs::read_to_string(&path).unwrap();
        let parsed: CapturedLinkerInvocation = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed, inv);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_invocation_creates_the_cache_dir_if_missing() {
        let dir = unique_tempdir().join("nested/path");
        assert!(!dir.exists());
        let inv = CapturedLinkerInvocation {
            output: Some("/tmp/lib.dylib".into()),
            args: vec![],
            timestamp_micros: 1,
            linker: None,
            envs: Default::default(),
        };
        save_invocation(&dir, &inv).expect("save");
        assert!(dir.is_dir());

        // best-effort cleanup
        let mut to_remove = dir;
        for _ in 0..3 {
            to_remove.pop();
        }
        let _ = std::fs::remove_dir_all(&to_remove);
    }

    // ----- MSVC ----------------------------------------------------------

    #[test]
    fn extract_output_reads_the_msvc_out_option() {
        let args = s(&["/NOLOGO", r"/OUT:C:\t\deps\app.exe", "/DEBUG"]);
        assert_eq!(extract_output(&args).as_deref(), Some(r"C:\t\deps\app.exe"));
        let lower = s(&[r"/out:C:\t\app.exe"]);
        assert_eq!(extract_output(&lower).as_deref(), Some(r"C:\t\app.exe"));
    }

    #[test]
    fn response_file_in_utf16_with_quoted_lines_is_decoded() {
        let text = "\"/NOLOGO\"\r\n\"C:\\a b\\x.o\"\r\n\"/OUT:C:\\t\\app.exe\"\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(
            parse_response_file(&bytes),
            s(&["/NOLOGO", r"C:\a b\x.o", r"/OUT:C:\t\app.exe"]),
        );
    }

    #[test]
    fn response_file_in_utf8_with_backslash_escapes_is_decoded() {
        assert_eq!(
            parse_response_file(b"-o\n/tmp/out\\ dir/lib.so\n"),
            s(&["-o", "/tmp/out dir/lib.so"]),
        );
    }

    #[test]
    fn capture_expands_response_files_and_keys_on_their_output() {
        let dir = unique_tempdir();
        let rsp = dir.join("linker-arguments");
        std::fs::write(&rsp, "\"/OUT:C:\\t\\app.exe\"\n\"kernel32.lib\"\n").unwrap();
        let invocation = capture(&[format!("@{}", rsp.display())]).unwrap();
        assert_eq!(invocation.output.as_deref(), Some(r"C:\t\app.exe"));
        assert_eq!(invocation.args, s(&[r"/OUT:C:\t\app.exe", "kernel32.lib"]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_keeps_an_unreadable_response_argument_as_is() {
        let invocation = capture(&s(&["@/nope/missing.rsp", "-o", "out"])).unwrap();
        assert_eq!(invocation.args, s(&["@/nope/missing.rsp", "-o", "out"]));
    }
}
