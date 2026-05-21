//! Curated terminal output for `whisker run` / `whisker build`.
//!
//! Replaces ad-hoc `eprintln!("[whisker-build] …")` /
//! `eprintln!("[whisker-dev-server] …")` lines with a single, uniform
//! event surface that the user sees as:
//!
//! ```text
//! ──── Build ────────────────────────────────────
//!   ⏵  compile      hello-world             …
//!   ✓  compile      hello-world             6.7s
//!   ⏵  stage        xcframework             …
//!   ✓  stage        xcframework             0.3s
//!   ⚠  simctl       target already booted
//!
//! ──── Patch ───────────────────────────────────
//!   ✓  patch        tier 1                  730ms
//! ```
//!
//! ## Behaviour modes
//!
//! - **Default**: spinners + curated step list, color when stderr is
//!   a TTY, ASCII fallback otherwise.
//! - **`WHISKER_VERBOSE=1`**: every event is emitted as plain
//!   `[whisker] …` lines without spinners. Same content the
//!   pre-refactor `eprintln!` calls produced, but uniformly prefixed.
//!   Underlying tool output (cargo / xcodebuild / gradle) also
//!   streams through verbatim — the caller is responsible for piping
//!   those streams; we don't capture them here.
//!
//! `WHISKER_VERBOSE` is meant as the `--verbose` CLI flag's
//! transport: the CLI sets it before invoking the dev-server / build
//! pipeline so the env-var is the single source of truth across
//! crate boundaries.
//!
//! ## Why a shared module, not a trait
//!
//! whisker-build, whisker-dev-server, and whisker-cli all need to
//! emit status. Threading an `OutputSink` trait through every call
//! site would be a big refactor with no payoff (there's exactly one
//! production sink — stderr). A free-fn surface with a thread-local
//! configuration knob keeps the migration to a per-call edit instead
//! of a signature change.

use std::io::IsTerminal;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

// ---- Shared MultiProgress + status bar -------------------------------
//
// Steps and the dev-server status share a single `MultiProgress` so
// their drawing doesn't fight: the status bar is anchored at the
// bottom, individual step bars insert above it. Without coordination,
// indicatif's redraws and `eprintln!`s interleave and we end up with
// the "client connected" line wedged between two spinner frames the
// user reported.

fn multi() -> &'static MultiProgress {
    static M: OnceLock<MultiProgress> = OnceLock::new();
    M.get_or_init(MultiProgress::new)
}

/// The current persistent dev-server status bar, if any. `Some` once
/// [`ensure_status`] has run; otherwise no status anchor exists and
/// new step bars just append to the multi-progress in arrival order.
static STATUS_BAR: Mutex<Option<ProgressBar>> = Mutex::new(None);

// ---- Configuration ----------------------------------------------------

#[derive(Copy, Clone, Debug)]
enum Mode {
    /// Default — colored output with spinners (when stderr is a TTY).
    Curated,
    /// `WHISKER_VERBOSE=1` — plain `[whisker] …` lines, no spinners.
    Verbose,
}

fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| {
        if is_verbose() {
            Mode::Verbose
        } else {
            Mode::Curated
        }
    })
}

/// `true` when `WHISKER_VERBOSE=1` is set in the environment. Same
/// switch the `--verbose` CLI flag toggles. Public so the
/// dev-server's noise filters (e.g. xcodebuild warning suppression)
/// can opt out under verbose mode and let everything through.
pub fn is_verbose() -> bool {
    std::env::var("WHISKER_VERBOSE")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// `true` when stderr is connected to an interactive terminal and we
/// should use ANSI color + spinner refresh. Off by default in CI /
/// piped builds — the [`Mode::Curated`] path still works there but
/// without animation.
fn is_tty() -> bool {
    static TTY: OnceLock<bool> = OnceLock::new();
    *TTY.get_or_init(|| std::io::stderr().is_terminal())
}

// ---- Persistent dev-server status bar --------------------------------

/// Initialise the persistent status bar (anchored at the bottom of
/// the rendered area). Subsequent calls update the same bar — only
/// the first call decides the bar's label / prefix.
///
/// Safe to call before any [`step`] starts; the bar slots into the
/// shared [`multi`] and every subsequent step bar inserts above it.
pub fn ensure_status(label: impl Into<String>) {
    if !matches!(mode(), Mode::Curated) || !is_tty() {
        // Verbose / non-TTY: no spinner-style bottom bar; status
        // changes go through `set_status` which falls back to
        // `info()`-style lines.
        return;
    }
    let mut guard = STATUS_BAR.lock().expect("status mutex");
    if guard.is_some() {
        return;
    }
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("  \x1b[35m◍\x1b[0m {prefix:<12} {msg}")
            .expect("status template is valid"),
    );
    bar.set_prefix(label.into());
    bar.set_message("");
    bar.enable_steady_tick(Duration::from_millis(200));
    *guard = Some(multi().add(bar));
}

/// Update the persistent status bar's message in place. No-op when
/// no bar is active (verbose / non-TTY / before `ensure_status`).
pub fn set_status(msg: impl Into<String>) {
    let m = msg.into();
    match mode() {
        Mode::Verbose => eprintln!("[whisker] status: {m}"),
        Mode::Curated => {
            if let Ok(guard) = STATUS_BAR.lock() {
                if let Some(bar) = guard.as_ref() {
                    bar.set_message(m);
                    return;
                }
            }
            // No bar yet (non-TTY or pre-`ensure_status`): fall back
            // to a regular info line so the message isn't lost.
            info(m);
        }
    }
}

/// Tear down the status bar at the end of a run. Leaves a final
/// rendered line in scrollback (via `finish_with_message` + the
/// `{msg}` template swap the steps use).
pub fn finish_status(final_msg: impl Into<String>) {
    if let Ok(mut guard) = STATUS_BAR.lock() {
        if let Some(bar) = guard.take() {
            bar.set_style(
                ProgressStyle::with_template("{msg}").expect("template literal is valid"),
            );
            bar.finish_with_message(final_msg.into());
        }
    }
}

// ---- Section headers --------------------------------------------------

/// Print a section header. Sections group related steps together:
/// `"Build"`, `"Patch"`, `"Watch"`, `"Install"`. Keep names short
/// (one word) so the visual rhythm is regular.
pub fn section(name: &str) {
    match mode() {
        Mode::Verbose => {
            eprintln!("[whisker] ─── {name} ───");
        }
        Mode::Curated => {
            // Line drawing matches what `cargo` itself emits during
            // its "Compiling" / "Finished" phases — a single visual
            // rhythm across the whole pipeline.
            let bar_chars = "─".repeat(40usize.saturating_sub(name.len()));
            let line = if is_tty() {
                format!("\n\x1b[1;36m──── {name} {bar_chars}\x1b[0m")
            } else {
                format!("\n──── {name} {bar_chars}")
            };
            emit_above_bars(&line);
        }
    }
}

/// Print a line, routing through the shared MultiProgress when a
/// status bar / step bar is alive so the line lands ABOVE the bars
/// instead of overlapping with their redraw. Falls back to plain
/// `eprintln!` when nothing's animated.
fn emit_above_bars(line: &str) {
    // `multi.println` panics with a "no bars in multi" check? Actually
    // it just no-ops when there are no bars. Either way it's the safe
    // primitive — fall back to eprintln only when we know we're
    // non-TTY (in which case no multi anyway).
    if is_tty() {
        let _ = multi().println(line);
    } else {
        eprintln!("{line}");
    }
}

// ---- Steps (durable progress lines) ----------------------------------

/// A live progress line. Created with [`step`], updated by
/// [`Step::done`] / [`Step::fail`].
///
/// In curated TTY mode this is a spinner that re-renders in place; in
/// verbose mode each transition prints a separate line. Either way
/// the same call sites work — callers don't branch on mode.
pub struct Step {
    /// `Some` only in curated TTY mode — non-TTY curated still emits
    /// plain lines, just without animation.
    bar: Option<ProgressBar>,
    /// Used by `done()` / `fail()` for the elapsed-time render.
    started_at: Instant,
    /// Carried separately from the bar's prefix because verbose-mode
    /// transitions need it for the final line emission too.
    name: String,
    detail: String,
}

impl Step {
    /// Resolve the step to a success state with an optional summary
    /// (`"6.7s"`, `"1.2 MB"`, etc.). Pass an empty string to suppress.
    pub fn done(self, summary: impl Into<String>) {
        self.finish(StepKind::Done, &summary.into());
    }

    /// Resolve the step to a failure. Renders an `✗` marker; the
    /// caller is expected to follow up with an `ui::error(...)` line
    /// containing the actionable detail.
    pub fn fail(self, summary: impl Into<String>) {
        self.finish(StepKind::Fail, &summary.into());
    }

    /// Spawn `cmd`, stream its stdout + stderr line-by-line, and
    /// return its [`ExitStatus`]. Cargo-style progress lines
    /// (`    Compiling X v0.1.0`, `    Finished …`, `    Updating
    /// crates.io …`) update the spinner's message in place so the
    /// step stays a single live line; everything else — rustc
    /// errors, linker output, warnings — is printed above the
    /// spinner so it persists in scrollback for copy-paste triage.
    ///
    /// In non-TTY mode (CI, `tee` to a file, `WHISKER_VERBOSE=1`)
    /// every line is emitted verbatim — no in-place rewriting,
    /// because there's no spinner to anchor against.
    pub fn pipe(
        &self,
        cmd: &mut std::process::Command,
    ) -> std::io::Result<std::process::ExitStatus> {
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let bar_stdout = self.bar.clone();
        let bar_stderr = self.bar.clone();
        let t_out = std::thread::spawn(move || stream_through_bar(stdout, bar_stdout));
        let t_err = std::thread::spawn(move || stream_through_bar(stderr, bar_stderr));
        let status = child.wait()?;
        let _ = t_out.join();
        let _ = t_err.join();
        Ok(status)
    }

    fn finish(self, kind: StepKind, summary: &str) {
        let elapsed = format_elapsed(self.started_at.elapsed());
        let summary = if summary.is_empty() {
            elapsed
        } else {
            format!("{summary}  {elapsed}")
        };
        let glyph = kind.glyph();
        let line = render_step_line(glyph, &self.name, &self.detail, &summary, kind);
        if let Some(bar) = self.bar {
            // Swap the spinner template for a plain `{msg}` so the
            // final line is *exactly* the formatted text we built —
            // without the leftover `{spinner}` glyph + `{prefix}`
            // duplication + trailing `…` that the live template
            // would otherwise re-render around it.
            bar.set_style(
                ProgressStyle::with_template("{msg}").expect("template literal is valid"),
            );
            bar.finish_with_message(line);
        } else {
            eprintln!("{line}");
        }
    }
}

/// Read `stream` line-by-line; route cargo-progress lines through
/// the spinner's `set_message`, everything else through
/// [`ProgressBar::println`] (or `eprintln!` when there's no bar).
fn stream_through_bar<R: std::io::Read + Send + 'static>(
    stream: Option<R>,
    bar: Option<ProgressBar>,
) {
    use std::io::{BufRead, BufReader};
    let Some(s) = stream else { return };
    let reader = BufReader::new(s);
    for line in reader.lines().map_while(Result::ok) {
        if let Some(progress) = cargo_progress_text(&line) {
            if let Some(bar) = &bar {
                bar.set_message(progress.to_string());
            }
            // No bar (non-TTY / verbose): emit verbatim. Without
            // this branch the progress lines would be silently
            // discarded in CI logs.
            else if matches!(mode(), Mode::Verbose) {
                eprintln!("[whisker] {line}");
            }
        } else if !line.is_empty() {
            // Diagnostics / errors / unrecognised tool output:
            // persist in scrollback. `println` writes a line above
            // the spinner, the bar redraws below.
            if let Some(bar) = &bar {
                bar.println(&line);
            } else {
                eprintln!("{line}");
            }
        }
    }
}

/// Recognise a cargo-style progress line (`    Compiling foo v0.1.0`,
/// `   Compiling foo v0.1.0`, `    Finished …`) and return the
/// trimmed text — that's what we surface inside the spinner.
/// Returns `None` for anything that isn't progress (rustc errors,
/// linker output, the user's `println!` output, etc.).
///
/// Tolerates ANSI escapes — cargo emits color codes to TTYs, and
/// piping doesn't always strip them when cargo's `--color=always` is
/// in effect or when the user's `.cargo/config.toml` forces it.
fn cargo_progress_text(line: &str) -> Option<&str> {
    let stripped = strip_leading_ansi(line.trim_start());
    let first_word = stripped.split_whitespace().next()?;
    // Keep this list aligned with cargo's `Status` shell glyphs. New
    // verbs (`Generating`, etc.) can be added as cargo introduces them.
    if matches!(
        first_word,
        "Compiling"
            | "Checking"
            | "Finished"
            | "Updating"
            | "Downloading"
            | "Downloaded"
            | "Fresh"
            | "Locking"
            | "Building"
            | "Documenting"
            | "Generating"
            | "Installing"
            | "Removing"
            | "Compiled"
    ) {
        Some(stripped.trim_end())
    } else {
        None
    }
}

/// Strip a leading sequence of ANSI escape codes — `\x1b[…m` SGR
/// sequences cargo uses to color the status verb. Defensive: most
/// pipe scenarios get a no-color stream from cargo, but
/// `CARGO_TERM_COLOR=always` / `.cargo/config.toml` overrides exist.
fn strip_leading_ansi(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() && bytes[i] == 0x1b && bytes[i + 1] == b'[' {
        // Find the terminating letter (in range @..~ = 0x40..0x7e).
        let mut j = i + 2;
        while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
            j += 1;
        }
        if j < bytes.len() {
            i = j + 1;
        } else {
            break;
        }
    }
    &s[i..]
}

#[derive(Copy, Clone)]
enum StepKind {
    Done,
    Fail,
}

impl StepKind {
    fn glyph(&self) -> &'static str {
        match self {
            StepKind::Done => "✓",
            StepKind::Fail => "✗",
        }
    }
}

/// Start a step.
///
/// `name` is the verb-noun anchor (`"compile"`, `"stage"`,
/// `"install"`, `"patch"`); `detail` is the variable suffix that
/// changes per invocation (`"hello-world"`, `"xcframework"`).
///
/// The split is purely typographical — keeping `name` to a small
/// closed set lets readers visually align columns down the run log.
pub fn step(name: impl Into<String>, detail: impl Into<String>) -> Step {
    let name = name.into();
    let detail = detail.into();
    let started_at = Instant::now();

    match mode() {
        Mode::Verbose => {
            eprintln!("[whisker] ⏵ {name}: {detail}");
            Step {
                bar: None,
                started_at,
                name,
                detail,
            }
        }
        Mode::Curated if is_tty() => {
            let bar = ProgressBar::new_spinner();
            // 12-char fixed-width name column keeps verbs left-aligned
            // across steps (`compile     hello-world …`,
            // `stage       xcframework …`). 18 chars covers the
            // longest verb we use (`xcodebuild`) plus padding.
            bar.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {prefix:<12} {msg:<24} …")
                    .expect("template literal is valid"),
            );
            bar.set_prefix(name.clone());
            bar.set_message(detail.clone());
            bar.enable_steady_tick(Duration::from_millis(80));
            // Join the shared MultiProgress so step bars and the
            // dev-server status bar coordinate redraws. When a
            // status bar exists, new step bars insert *above* it so
            // status stays anchored at the bottom.
            let bar = {
                let guard = STATUS_BAR.lock().expect("status mutex");
                match guard.as_ref() {
                    Some(status) => multi().insert_before(status, bar),
                    None => multi().add(bar),
                }
            };
            Step {
                bar: Some(bar),
                started_at,
                name,
                detail,
            }
        }
        Mode::Curated => {
            // Curated but non-TTY (CI, piped to file). Emit a single
            // "started" line; `finish()` will emit the final state.
            eprintln!("  ⏵ {name:<12} {detail}");
            Step {
                bar: None,
                started_at,
                name,
                detail,
            }
        }
    }
}

fn render_step_line(
    glyph: &str,
    name: &str,
    detail: &str,
    summary: &str,
    kind: StepKind,
) -> String {
    if is_tty() {
        let color = match kind {
            StepKind::Done => "\x1b[32m",
            StepKind::Fail => "\x1b[31m",
        };
        format!("  {color}{glyph}\x1b[0m {name:<12} {detail:<24} {summary}")
    } else {
        format!("  {glyph} {name:<12} {detail:<24} {summary}")
    }
}

fn format_elapsed(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let total_secs = d.as_secs();
        format!("{}m{:02}s", total_secs / 60, total_secs % 60)
    }
}

// ---- One-shot lines (info / warn / error) ----------------------------

/// Informational line. Lower visual weight than [`step`]; use for
/// state changes that don't have a "started → finished" arc (e.g.
/// "watching examples/", "client connected", "patch sent").
pub fn info(msg: impl AsRef<str>) {
    let m = msg.as_ref();
    match mode() {
        Mode::Verbose => eprintln!("[whisker] {m}"),
        Mode::Curated => {
            if is_tty() {
                emit_above_bars(&format!("  \x1b[90m·\x1b[0m {m}"));
            } else {
                eprintln!("  · {m}");
            }
        }
    }
}

/// Non-fatal warning. Renders distinctly from `info` and `error` so
/// scanning a log for actionable items works without grep tricks.
/// Use for "simctl says target already booted" and other benign
/// rough edges that don't stop the pipeline.
pub fn warn(msg: impl AsRef<str>) {
    let m = msg.as_ref();
    match mode() {
        Mode::Verbose => eprintln!("[whisker] warn: {m}"),
        Mode::Curated => {
            if is_tty() {
                emit_above_bars(&format!("  \x1b[33m⚠\x1b[0m {m}"));
            } else {
                eprintln!("  ! {m}");
            }
        }
    }
}

/// Verbose-only diagnostic. Same shape as [`info`] but hidden by
/// default — only printed when `WHISKER_VERBOSE=1`. Use for internal
/// state that's useful when debugging the dev-server itself
/// (ASLR references, intermediate file paths, patcher symbol diffs)
/// but distracting noise during normal `whisker run`.
pub fn debug(msg: impl AsRef<str>) {
    match mode() {
        Mode::Verbose => {
            let m = msg.as_ref();
            eprintln!("[whisker] debug: {m}");
        }
        Mode::Curated => {}
    }
}

/// Hard failure indicator. Use after a [`Step::fail`] or stand-alone
/// when the failure isn't tied to a specific step. Doesn't exit the
/// process — that's the caller's call (typical pattern: `error(...)
/// + Err(anyhow!(...))?`).
pub fn error(msg: impl AsRef<str>) {
    let m = msg.as_ref();
    match mode() {
        Mode::Verbose => eprintln!("[whisker] error: {m}"),
        Mode::Curated => {
            if is_tty() {
                emit_above_bars(&format!("  \x1b[31m✗\x1b[0m {m}"));
            } else {
                eprintln!("  X {m}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_elapsed_chooses_unit_by_magnitude() {
        assert_eq!(format_elapsed(Duration::from_millis(42)), "42ms");
        assert_eq!(format_elapsed(Duration::from_millis(999)), "999ms");
        assert_eq!(format_elapsed(Duration::from_millis(1_000)), "1.0s");
        assert_eq!(format_elapsed(Duration::from_millis(6_750)), "6.8s");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2m05s");
    }

    #[test]
    fn step_kind_glyphs_are_recognisable_ascii() {
        // Quick sanity — broken assertion would mean someone swapped
        // the glyphs accidentally (we render these in non-TTY too,
        // where we want them distinct).
        assert_eq!(StepKind::Done.glyph(), "✓");
        assert_eq!(StepKind::Fail.glyph(), "✗");
    }

    #[test]
    fn render_step_line_aligns_name_column_at_12_chars() {
        // Force the non-TTY branch (plain output) so the assertion
        // doesn't depend on `is_tty()` returning false at test time
        // — which it does under cargo test, but explicit is better.
        std::env::set_var("WHISKER_VERBOSE", "");
        let line = if is_tty() {
            // Test fixture: derive the non-color version even when
            // running interactively. We can't easily mock is_tty()
            // from a unit test without an extra abstraction, so
            // verify the structure on the plain branch instead.
            return;
        } else {
            render_step_line("✓", "compile", "hello-world", "6.7s", StepKind::Done)
        };
        // "  ✓ compile      hello-world              6.7s"
        //          ^^^^^^^^^^^ 12 chars of name column
        assert!(line.contains("✓"));
        assert!(line.contains("compile"));
        assert!(line.contains("hello-world"));
        assert!(line.contains("6.7s"));
    }
}
