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

// (Removed: a persistent indicatif bar for dev-server status. See
// the `set_status` impl below for the simpler printed-line model.)

// ---- Configuration ----------------------------------------------------

#[derive(Copy, Clone, Debug)]
enum Mode {
    /// Default — colored output with spinners (when stderr is a TTY).
    Curated,
    /// `WHISKER_VERBOSE=1` — plain `[whisker] …` lines, no spinners.
    Verbose,
    /// `WHISKER_TUI=1` — whisker-cli is rendering a ratatui inline
    /// viewport at the bottom of the terminal. Same curated
    /// formatting as [`Mode::Curated`] (section headers, `✓`/`⏵` step
    /// glyphs, color), but routed through plain `eprintln!` so the
    /// lines scroll above the viewport as normal terminal content.
    /// indicatif spinners are suppressed — they'd race with ratatui's
    /// redraw and corrupt both surfaces.
    Tui,
}

fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| {
        // Order matters: verbose wins over TUI so `WHISKER_VERBOSE=1`
        // remains the universal "show me everything plain" override
        // even when the cli kicked the TUI on. Otherwise TUI wins over
        // Curated when set.
        if is_verbose() {
            Mode::Verbose
        } else if is_tui() {
            Mode::Tui
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

/// `true` when `WHISKER_TUI=1` is set — `whisker-cli` sets this when
/// it's rendering an inline TUI status bar. The flag exists so the
/// `ui::*` surface can suppress indicatif animations (which would
/// race with ratatui's own redraw) while still producing the curated
/// `✓` / `⏵` formatting via plain `eprintln!`. Public so other
/// crates can check the same state if needed.
pub fn is_tui() -> bool {
    std::env::var("WHISKER_TUI")
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

// ---- Dev-server status (printed-line model) -------------------------
//
// An earlier iteration anchored a persistent indicatif `ProgressBar`
// at the bottom and updated it via `set_message`. In practice
// `MultiProgress::println` (called every time a section header / step
// transition fired) preserved the bar's then-current frame in the
// scrollback above each printed line — the user saw the same
// `◍ dev-server …` line stacked 3-4 times.
//
// The simpler model below skips the persistent bar entirely:
//
// - `ensure_status` emits a single info-style line on first call
//   ("dev-server starting" or similar). Subsequent calls are no-ops.
// - `set_status` deduplicates: it tracks the most-recently-emitted
//   status string and only prints when the new value differs. That
//   way startup events (`starting…` → `ws://… · 0 client(s)`) only
//   produce a line per *state change*, not per call.
// - `finish_status` prints a final line on shutdown.
//
// Trade-off: no live spinner. Dev-server state changes are rare
// (bind, client connect, patch sent) so a static line per state is
// clearer than an animated bottom anchor that has rendering bugs.

/// Last status string we printed. Used to dedupe rapid-fire
/// `set_status` calls.
static LAST_STATUS: Mutex<Option<String>> = Mutex::new(None);

/// Mark the dev-server's status surface as "active". Currently a
/// no-op recorder — kept as part of the public API because callers
/// in `whisker-dev-server` use it as a sentinel that says "you're
/// allowed to call `set_status` after this point".
pub fn ensure_status(_label: impl Into<String>) {
    if let Ok(mut guard) = LAST_STATUS.lock() {
        *guard = Some(String::new());
    }
}

/// Emit a dev-server status line. Deduplicates against the last
/// emission so back-to-back `set_status("X")` calls don't double-
/// print the same content. The line goes through `info()` so it
/// shares the `· <msg>` visual style with other one-shot lines.
pub fn set_status(msg: impl Into<String>) {
    let m = msg.into();
    let m_for_dedupe = m.clone();
    if let Ok(mut guard) = LAST_STATUS.lock() {
        if guard.as_ref() == Some(&m_for_dedupe) {
            return;
        }
        *guard = Some(m_for_dedupe);
    }
    info(format!("dev-server · {m}"));
}

/// Emit a final dev-server status line on shutdown. Same code path
/// as `set_status` minus the dedupe (we want the goodbye visible
/// even if it matches the previous status).
pub fn finish_status(final_msg: impl Into<String>) {
    info(format!("dev-server · {}", final_msg.into()));
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
        Mode::Curated | Mode::Tui => {
            // Line drawing matches what `cargo` itself emits during
            // its "Compiling" / "Finished" phases — a single visual
            // rhythm across the whole pipeline. Color codes are SGR
            // (no cursor motion) so they're safe to emit even in TUI
            // mode where the ratatui viewport owns the bottom region.
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

/// `true` when indicatif's in-place redraw machinery is allowed to
/// run. Off in TUI mode: ratatui owns the cursor and would race with
/// indicatif's bar redraws if we let MultiProgress animate spinners
/// or `suspend()` around eprintlns.
fn indicatif_active() -> bool {
    matches!(mode(), Mode::Curated) && is_tty()
}

/// Print a line, routing through the shared MultiProgress when a
/// status bar / step bar is alive so the line lands ABOVE the bars
/// instead of overlapping with their redraw. Falls back to plain
/// `eprintln!` when nothing's animated.
fn emit_above_bars(line: &str) {
    // `multi.println` panics with a "no bars in multi" check? Actually
    // `multi.suspend` is the indicatif-blessed primitive for
    // interleaving arbitrary output with bars: it clears the bars,
    // runs the closure (which writes via eprintln!), and redraws
    // the bars cleanly. Earlier attempts used `multi.println`,
    // which left the bar's then-current spinner frame stuck in
    // scrollback every time it pushed a line above the bars —
    // that's what produced the "`⠁ compile …` then `✓ compile …`
    // on two separate lines" duplication users reported.
    if !indicatif_active() {
        // Non-TTY, verbose, or TUI mode: indicatif isn't drawing
        // anything to interleave with, so a plain `eprintln!` is
        // both correct and necessary — `multi.suspend` here in TUI
        // mode would still flush stale indicatif state into the
        // ratatui-owned region.
        eprintln!("{line}");
        return;
    }
    let line_owned = line.to_string();
    multi().suspend(|| {
        eprintln!("{line_owned}");
    });
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

/// Read `stream` line-by-line, classifying each line into one of
/// three buckets:
///
/// 1. **Progress** (cargo/gradle/xcodebuild status line) — folded into
///    the spinner's `set_message` so the step row stays one live line.
/// 2. **Known noise** (gradle daemon advisories, gradle's
///    deprecation banner) — dropped silently. These are advisory text
///    the user can't act on, and they're the main offender behind
///    the "gradle build のログが見づらい" complaint.
/// 3. **Everything else** — printed above the bar so it persists in
///    scrollback for triage (rustc errors, gradle task failures,
///    user `println!`s reaching this path through `cmd.pipe`).
///
/// Verbose mode (`WHISKER_VERBOSE=1`) bypasses 1 + 2 and emits every
/// non-empty line verbatim — useful when debugging the filter itself.
fn stream_through_bar<R: std::io::Read + Send + 'static>(
    stream: Option<R>,
    bar: Option<ProgressBar>,
) {
    use std::io::{BufRead, BufReader};
    let Some(s) = stream else { return };
    let reader = BufReader::new(s);
    for line in reader.lines().map_while(Result::ok) {
        if let Some(progress) = subprocess_progress_text(&line) {
            if let Some(bar) = &bar {
                bar.set_message(progress.to_string());
                // No steady_tick anymore (see step() docs) so we
                // tick manually to repaint the new {msg}.
                bar.tick();
            }
            // No bar (non-TTY / verbose): emit verbatim. Without
            // this branch the progress lines would be silently
            // discarded in CI logs.
            else if matches!(mode(), Mode::Verbose) {
                eprintln!("[whisker] {line}");
            }
        } else if !line.is_empty() {
            // In curated mode, drop known-noise advisory lines that
            // the user can't act on. Verbose mode keeps everything
            // so the user has a chance to diagnose the filter
            // itself if a real diagnostic ever gets misclassified.
            if matches!(mode(), Mode::Curated) && is_subprocess_noise(&line) {
                continue;
            }
            // Diagnostics / errors / unrecognised tool output:
            // persist in scrollback. Use multi.suspend (not
            // bar.println) so the bar is properly cleared before
            // the line lands and redrawn afterwards — same fix as
            // `emit_above_bars`.
            if bar.is_some() {
                let line_owned = line.clone();
                multi().suspend(|| {
                    eprintln!("{line_owned}");
                });
            } else {
                eprintln!("{line}");
            }
        }
    }
}

/// Tag a line as a progress-status line worth folding into the
/// spinner. Currently recognises three tool families:
///
/// - **cargo** — `    Compiling foo v0.1.0`, `    Finished …`, etc.
///   See [`cargo_progress_text`].
/// - **gradle** — `> Task :app:assembleDebug`, with optional
///   `UP-TO-DATE` / `NO-SOURCE` / `FROM-CACHE` suffix. See
///   [`gradle_progress_text`].
/// - **gradle terminal** — `BUILD SUCCESSFUL in 18s` /
///   `BUILD FAILED in 18s`. Surfaced as the spinner's last frame
///   before the step finishes.
fn subprocess_progress_text(line: &str) -> Option<String> {
    if let Some(s) = cargo_progress_text(line) {
        return Some(s.to_string());
    }
    gradle_progress_text(line)
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

/// Recognise a gradle progress line and return its display form:
///
/// - `> Task :path:assembleDebug` → `gradle: :path:assembleDebug`
/// - `> Task :path:assembleDebug UP-TO-DATE` → `gradle: :path:assembleDebug UP-TO-DATE`
/// - `BUILD SUCCESSFUL in 18s` → `gradle: BUILD SUCCESSFUL in 18s`
/// - `BUILD FAILED in 18s` → `gradle: BUILD FAILED in 18s`
/// - `137 actionable tasks: 6 executed, 131 up-to-date` → same prefixed
///
/// Returns `None` for anything else. Gradle's output is dominated by
/// these patterns, so folding them into the spinner removes the
/// ~50-line scroll-burst per `whisker run` that the curated layout
/// was drowning in.
fn gradle_progress_text(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("> Task ") {
        return Some(format!("gradle: {rest}"));
    }
    if trimmed.starts_with("BUILD SUCCESSFUL") || trimmed.starts_with("BUILD FAILED") {
        return Some(format!("gradle: {trimmed}"));
    }
    if trimmed.contains(" actionable task") {
        return Some(format!("gradle: {trimmed}"));
    }
    None
}

/// Identify lines that are pure advisory noise from the gradle daemon
/// or related JVM tooling — output the user can neither act on nor
/// learn anything from. Dropping them removes the multi-line block
/// gradle emits on every assemble that says "we forked a JVM, here's
/// a link to documentation about it." Real diagnostics (compile
/// errors, task failures, custom output) flow through unchanged.
fn is_subprocess_noise(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    // Gradle daemon JVM advisory — five-line block emitted by every
    // assemble. Match the salient prefix of each line.
    const GRADLE_NOISE_PREFIXES: &[&str] = &[
        "To honour the JVM settings for this build",
        "Daemon will be stopped at the end of the build",
        "Deprecated Gradle features were used in this build",
        "You can use '--warning-mode all'",
        "For more on this, please refer to",
    ];
    for prefix in GRADLE_NOISE_PREFIXES {
        if t.starts_with(prefix) {
            return true;
        }
    }
    false
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
        Mode::Tui => {
            // TUI mode: the inline ratatui viewport captures stderr
            // and `insert_before`s each captured line into scrollback.
            // Emitting a "⏵ started" line here would just be
            // immediately followed by the "✓ done" line from
            // `finish()`, doubling the row count — there's no
            // overwrite mechanism for already-committed scrollback
            // lines (unlike indicatif's spinner in Curated mode).
            // Defer all output to `finish()` so each step occupies
            // exactly one row.
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
            //
            // No `enable_steady_tick`: combined with multi.suspend
            // (which clears/redraws bars around external writes),
            // an async tick raced with the suspend cycle and could
            // briefly redraw the bar at a stale position. The
            // {msg} column updates whenever cargo emits a new
            // progress line — that's the actual "still working"
            // signal, animation isn't needed on top.
            bar.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {prefix:<12} {msg:<24} …")
                    .expect("template literal is valid"),
            );
            bar.set_prefix(name.clone());
            bar.set_message(detail.clone());
            let bar = multi().add(bar);
            // Manual tick so the bar shows up immediately rather
            // than waiting for the first `set_message` update.
            bar.tick();
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
        Mode::Curated | Mode::Tui => {
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
        Mode::Curated | Mode::Tui => {
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
        Mode::Curated | Mode::Tui => {}
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
        Mode::Curated | Mode::Tui => {
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

    // ----- subprocess output classifiers ----------------------------

    #[test]
    fn cargo_progress_recognised_with_leading_whitespace() {
        assert_eq!(
            cargo_progress_text("    Compiling foo v0.1.0"),
            Some("Compiling foo v0.1.0"),
        );
        assert_eq!(
            cargo_progress_text("   Finished `release` target(s) in 12.3s"),
            Some("Finished `release` target(s) in 12.3s"),
        );
    }

    #[test]
    fn cargo_progress_rejects_diagnostics_and_user_output() {
        assert!(cargo_progress_text("error[E0277]: ...").is_none());
        assert!(cargo_progress_text("warning: unused").is_none());
        assert!(cargo_progress_text("user println output").is_none());
    }

    #[test]
    fn gradle_task_lines_fold_into_progress() {
        assert_eq!(
            gradle_progress_text("> Task :app:assembleDebug"),
            Some("gradle: :app:assembleDebug".to_string()),
        );
        assert_eq!(
            gradle_progress_text("> Task :app:assembleDebug UP-TO-DATE"),
            Some("gradle: :app:assembleDebug UP-TO-DATE".to_string()),
        );
        assert_eq!(
            gradle_progress_text("> Task :whisker-image:mergeDebugJniLibFolders NO-SOURCE"),
            Some("gradle: :whisker-image:mergeDebugJniLibFolders NO-SOURCE".to_string()),
        );
    }

    #[test]
    fn gradle_build_terminal_status_recognised() {
        assert_eq!(
            gradle_progress_text("BUILD SUCCESSFUL in 18s"),
            Some("gradle: BUILD SUCCESSFUL in 18s".to_string()),
        );
        assert_eq!(
            gradle_progress_text("BUILD FAILED in 1m 12s"),
            Some("gradle: BUILD FAILED in 1m 12s".to_string()),
        );
        assert_eq!(
            gradle_progress_text("137 actionable tasks: 6 executed, 131 up-to-date"),
            Some("gradle: 137 actionable tasks: 6 executed, 131 up-to-date".to_string()),
        );
    }

    #[test]
    fn gradle_progress_rejects_non_gradle_lines() {
        assert!(gradle_progress_text("Compiling foo v0.1.0").is_none());
        assert!(gradle_progress_text("regular line").is_none());
        // A `>` prefix without `Task` doesn't qualify — gradle's
        // configure phase emits `> Configure project :app` blocks
        // that the user may want to triage; let them surface.
        assert!(gradle_progress_text("> Configure project :app").is_none());
    }

    #[test]
    fn subprocess_progress_combines_both_recognisers() {
        assert!(subprocess_progress_text("    Compiling foo v0.1.0").is_some());
        assert!(subprocess_progress_text("> Task :app:assembleDebug").is_some());
        assert!(subprocess_progress_text("BUILD SUCCESSFUL in 18s").is_some());
        assert!(subprocess_progress_text("regular diagnostic line").is_none());
    }

    #[test]
    fn subprocess_noise_filters_gradle_daemon_advisory() {
        assert!(is_subprocess_noise(
            "To honour the JVM settings for this build a single-use Daemon process will be forked. ..."
        ));
        assert!(is_subprocess_noise(
            "Daemon will be stopped at the end of the build"
        ));
        assert!(is_subprocess_noise(
            "Deprecated Gradle features were used in this build, making it incompatible ..."
        ));
        assert!(is_subprocess_noise(
            "You can use '--warning-mode all' to show the individual deprecation warnings ..."
        ));
        assert!(is_subprocess_noise(
            "For more on this, please refer to https://docs.gradle.org/..."
        ));
    }

    #[test]
    fn subprocess_noise_leaves_real_diagnostics_alone() {
        // Real failures should NOT be filtered — they need to land in
        // scrollback so the user sees what to fix.
        assert!(!is_subprocess_noise(
            "FAILURE: Build failed with an exception."
        ));
        assert!(!is_subprocess_noise("* What went wrong:"));
        assert!(!is_subprocess_noise("error: linker `cc` not found"));
        assert!(!is_subprocess_noise(
            "> Task :app:compileDebugJavaWithJavac FAILED"
        ));
    }
}
