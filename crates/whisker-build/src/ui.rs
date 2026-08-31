//! Curated terminal output for `whisker run`.
//!
//! One uniform event surface across whisker-build, whisker-dev-server
//! and whisker-cli, which the user sees as:
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
//!   ✓  patch        hot reload                  730ms
//! ```
//!
//! ## Behaviour modes
//!
//! - **Default**: spinners + curated step list, color when stderr is
//!   a TTY, ASCII fallback otherwise.
//! - **`WHISKER_VERBOSE=1`**: every event is emitted as plain
//!   `[whisker] …` lines without spinners.
//!   Underlying tool output (cargo / xcodebuild / gradle) also
//!   streams through verbatim — the caller is responsible for piping
//!   those streams; we don't capture them here.
//!
//! `WHISKER_VERBOSE` is meant as the `--verbose` CLI flag's
//! transport: the CLI sets it before invoking the dev-server / build
//! pipeline so the env-var is the single source of truth across
//! crate boundaries.
//!
//! ## Presentation seam
//!
//! Call sites use the free functions in this module. A top-level
//! workflow may install a scoped [`ProgressReporter`] to receive typed
//! events; without one, the same calls render deterministic terminal
//! output directly.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

// ---- Structured progress interface ---------------------------------

/// Stable vocabulary shared by `whisker build`, `whisker run`, plain
/// terminal output, and future editor integrations. Presentation code must
/// not infer workflow state by parsing rendered strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Setup,
    Compile,
    Gradle,
    Xcodebuild,
    Boot,
    Install,
    Launch,
    HotReload,
    Package,
    Open,
}

impl OperationKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Compile => "compile",
            Self::Gradle => "gradle",
            Self::Xcodebuild => "xcodebuild",
            Self::Boot => "boot",
            Self::Install => "install",
            Self::Launch => "launch",
            Self::HotReload => "hot reload",
            Self::Package => "package",
            Self::Open => "open",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationOutcome {
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageLevel {
    Info,
    Warning,
    Error,
    Debug,
    Log,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressEvent {
    Section(String),
    OperationStarted {
        kind: OperationKind,
        detail: String,
    },
    OperationProgress {
        kind: OperationKind,
        message: String,
    },
    OperationFinished {
        kind: OperationKind,
        detail: String,
        outcome: OperationOutcome,
        summary: String,
        elapsed: Duration,
    },
    Message {
        level: MessageLevel,
        text: String,
    },
    Status(String),
}

/// Adapter interface at the presentation seam. Only a top-level user-facing
/// workflow installs a reporter; nested Gradle/Xcode helper commands keep
/// their deterministic plain stderr/stdout contracts.
pub trait ProgressReporter: Send + Sync {
    fn report(&self, event: ProgressEvent);
}

impl<F> ProgressReporter for F
where
    F: Fn(ProgressEvent) + Send + Sync,
{
    fn report(&self, event: ProgressEvent) {
        self(event);
    }
}

struct ReporterSlot {
    id: u64,
    reporter: Arc<dyn ProgressReporter>,
}

static REPORTER: Mutex<Option<ReporterSlot>> = Mutex::new(None);
static NEXT_REPORTER_ID: AtomicU64 = AtomicU64::new(1);

/// Keeps the installed reporter scoped to one top-level workflow.
pub struct ReporterGuard {
    id: u64,
}

impl Drop for ReporterGuard {
    fn drop(&mut self) {
        if let Ok(mut slot) = REPORTER.lock() {
            if slot.as_ref().map(|current| current.id) == Some(self.id) {
                *slot = None;
            }
        }
    }
}

#[derive(Debug)]
pub struct ReporterAlreadyInstalled;

impl std::fmt::Display for ReporterAlreadyInstalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a Whisker progress reporter is already installed")
    }
}

impl std::error::Error for ReporterAlreadyInstalled {}

pub fn install_reporter(
    reporter: impl ProgressReporter + 'static,
) -> Result<ReporterGuard, ReporterAlreadyInstalled> {
    let id = NEXT_REPORTER_ID.fetch_add(1, Ordering::Relaxed);
    let mut slot = REPORTER.lock().map_err(|_| ReporterAlreadyInstalled)?;
    if slot.is_some() {
        return Err(ReporterAlreadyInstalled);
    }
    *slot = Some(ReporterSlot {
        id,
        reporter: Arc::new(reporter),
    });
    Ok(ReporterGuard { id })
}

fn report(event: ProgressEvent) -> bool {
    let reporter = REPORTER
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|slot| Arc::clone(&slot.reporter)));
    if let Some(reporter) = reporter {
        reporter.report(event);
        true
    } else {
        false
    }
}

fn reporter_active() -> bool {
    REPORTER.lock().map(|slot| slot.is_some()).unwrap_or(false)
}

// ---- Shared MultiProgress + status bar -------------------------------
//
// Everything that draws shares one `MultiProgress`; independent bars
// would interleave their redraws with each other's `eprintln!`s and
// wedge printed lines between spinner frames.

fn multi() -> &'static MultiProgress {
    static M: OnceLock<MultiProgress> = OnceLock::new();
    M.get_or_init(MultiProgress::new)
}

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

// ---- Dev-server status (printed-line model) -------------------------
//
// Deliberately no persistent bottom-anchored bar: printing above one
// leaves its then-current spinner frame in scrollback, stacking a copy
// of the status row per printed line. Dev-server state changes are
// rare (bind, client connect, patch sent), so one printed line per
// state change costs nothing and renders correctly.

/// Last status string we printed. Used to dedupe rapid-fire
/// `set_status` calls.
static LAST_STATUS: Mutex<Option<String>> = Mutex::new(None);

/// Mark the dev-server's status surface as "active". A no-op recorder
/// beyond that: `whisker-dev-server` calls it as the sentinel meaning
/// "`set_status` is allowed from here on".
pub fn ensure_status(_label: impl Into<String>) {
    if reporter_active() {
        return;
    }
    if let Ok(mut guard) = LAST_STATUS.lock() {
        *guard = Some(String::new());
    }
}

/// Emit a dev-server status line. Deduplicates against the last
/// emission so back-to-back `set_status("X")` calls don't double-
/// print the same content. The line goes through `info()` so it
/// shares the `· <msg>` visual style with other one-shot lines.
///
/// With a structured reporter this becomes a [`ProgressEvent::Status`].
pub fn set_status(msg: impl Into<String>) {
    let msg = msg.into();
    if report(ProgressEvent::Status(msg.clone())) {
        return;
    }
    let m = msg;
    let m_for_dedupe = m.clone();
    if let Ok(mut guard) = LAST_STATUS.lock() {
        if guard.as_ref() == Some(&m_for_dedupe) {
            return;
        }
        *guard = Some(m_for_dedupe);
    }
    info(format!("dev-server · {m}"));
}

/// Emit a final dev-server status line on shutdown. Like `set_status`
/// minus the dedupe, so the goodbye shows even when it repeats the
/// previous status.
pub fn finish_status(final_msg: impl Into<String>) {
    let final_msg = final_msg.into();
    if report(ProgressEvent::Status(final_msg.clone())) {
        return;
    }
    info(format!("dev-server · {final_msg}"));
}

// ---- Section headers --------------------------------------------------

/// Print a section header. Sections group related steps together:
/// `"Build"`, `"Patch"`, `"Watch"`, `"Install"`. Keep names short
/// (one word) so the visual rhythm is regular.
pub fn section(name: &str) {
    if report(ProgressEvent::Section(name.to_string())) {
        return;
    }
    match mode() {
        Mode::Verbose => {
            eprintln!("[whisker] ─── {name} ───");
        }
        Mode::Curated => {
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

/// `true` when indicatif's in-place redraw machinery is allowed to run.
/// Structured reporters return before reaching this plain renderer.
fn indicatif_active() -> bool {
    matches!(mode(), Mode::Curated) && is_tty()
}

/// Print a line, routing through the shared MultiProgress when a
/// status bar / step bar is alive so the line lands ABOVE the bars
/// instead of overlapping with their redraw. Falls back to plain
/// `eprintln!` when nothing's animated.
fn emit_above_bars(line: &str) {
    // `multi.suspend`, never `multi.println`: suspend clears the bars,
    // runs the closure, and redraws, whereas println leaves the bar's
    // then-current spinner frame stuck in scrollback above each line.
    if !indicatif_active() {
        // Nothing is animating.
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
    kind: OperationKind,
    detail: String,
    reported: bool,
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
        // Track the PID so build cancellation can SIGTERM cargo /
        // gradle / xcodebuild instead of orphaning it. The guard
        // unregisters when `pipe` returns.
        let _child_guard = crate::child_guard::track(child.id());
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let bar_stdout = self.bar.clone();
        let bar_stderr = self.bar.clone();
        let reported = self.reported;
        let kind = self.kind;
        let t_out =
            std::thread::spawn(move || stream_through_bar(stdout, bar_stdout, reported, kind));
        let t_err =
            std::thread::spawn(move || stream_through_bar(stderr, bar_stderr, reported, kind));
        let status = child.wait()?;
        let _ = t_out.join();
        let _ = t_err.join();
        Ok(status)
    }

    fn finish(self, kind: StepKind, summary: &str) {
        let duration = self.started_at.elapsed();
        if self.reported {
            report(ProgressEvent::OperationFinished {
                kind: self.kind,
                detail: self.detail,
                outcome: match kind {
                    StepKind::Done => OperationOutcome::Done,
                    StepKind::Fail => OperationOutcome::Failed,
                },
                summary: summary.to_string(),
                elapsed: duration,
            });
            return;
        }
        let elapsed = format_elapsed(duration);
        let summary = if summary.is_empty() {
            elapsed
        } else {
            format!("{summary}  {elapsed}")
        };
        let glyph = kind.glyph();
        let line = render_step_line(glyph, self.kind.label(), &self.detail, &summary, kind);
        if let Some(bar) = self.bar {
            // Plain `{msg}` template so the final line is exactly the
            // text built above; the live template would re-render its
            // spinner glyph, prefix and trailing `…` around it.
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
///    deprecation banner) — dropped silently; advisory text the user
///    can't act on.
/// 3. **Everything else** — printed above the bar so it persists in
///    scrollback for triage (rustc errors, gradle task failures,
///    user `println!`s reaching this path through `cmd.pipe`).
///
/// Verbose mode (`WHISKER_VERBOSE=1`) bypasses 1 + 2 and emits every
/// non-empty line verbatim — useful when debugging the filter itself.
fn stream_through_bar<R: std::io::Read + Send + 'static>(
    stream: Option<R>,
    bar: Option<ProgressBar>,
    reported: bool,
    kind: OperationKind,
) {
    use std::io::{BufRead, BufReader};
    let Some(s) = stream else { return };
    let reader = BufReader::new(s);
    for line in reader.lines().map_while(Result::ok) {
        if let Some(progress) = subprocess_progress_text(&line) {
            if reported {
                report(ProgressEvent::OperationProgress {
                    kind,
                    message: progress,
                });
                continue;
            }
            if let Some(bar) = &bar {
                bar.set_message(progress);
                // No steady tick (see `step`), so repaint by hand.
                bar.tick();
            }
            // Without a bar these lines would vanish from CI logs.
            else if matches!(mode(), Mode::Verbose) {
                eprintln!("[whisker] {line}");
            }
        } else if !line.is_empty() {
            // Verbose keeps everything, so a real diagnostic that gets
            // misclassified as noise is still reachable.
            if matches!(mode(), Mode::Curated) && is_subprocess_noise(&line) {
                continue;
            }
            if reported {
                report(ProgressEvent::Message {
                    level: MessageLevel::Log,
                    text: line,
                });
                continue;
            }
            // `multi.suspend`, not `bar.println` — see `emit_above_bars`.
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
    // Mirrors cargo's `Status` shell verbs.
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
/// these patterns, so folding them into the spinner is what keeps an
/// assemble from scroll-bursting the curated layout.
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
pub fn step(kind: OperationKind, detail: impl Into<String>) -> Step {
    let detail = detail.into();
    let started_at = Instant::now();
    if report(ProgressEvent::OperationStarted {
        kind,
        detail: detail.clone(),
    }) {
        return Step {
            bar: None,
            started_at,
            kind,
            detail,
            reported: true,
        };
    }
    let name = kind.label();

    match mode() {
        Mode::Verbose => {
            eprintln!("[whisker] ⏵ {name}: {detail}");
            Step {
                bar: None,
                started_at,
                kind,
                detail,
                reported: false,
            }
        }
        Mode::Curated if is_tty() => {
            let bar = ProgressBar::new_spinner();
            // No `enable_steady_tick`: an async tick races the
            // clear/redraw cycle `multi.suspend` runs around external
            // writes and can leave the bar redrawn at a stale
            // position. The {msg} column already moves on every cargo
            // progress line, which is the real "still working" signal.
            bar.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {prefix:<12} {msg:<24} …")
                    .expect("template literal is valid"),
            );
            bar.set_prefix(name);
            bar.set_message(detail.clone());
            let bar = multi().add(bar);
            // Show the bar now instead of at the first `set_message`.
            bar.tick();
            Step {
                bar: Some(bar),
                started_at,
                kind,
                detail,
                reported: false,
            }
        }
        Mode::Curated => {
            eprintln!("  ⏵ {name:<12} {detail}");
            Step {
                bar: None,
                started_at,
                kind,
                detail,
                reported: false,
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
    if report(ProgressEvent::Message {
        level: MessageLevel::Info,
        text: m.to_string(),
    }) {
        return;
    }
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
    if report(ProgressEvent::Message {
        level: MessageLevel::Warning,
        text: m.to_string(),
    }) {
        return;
    }
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
    if reporter_active() {
        if is_verbose() {
            report(ProgressEvent::Message {
                level: MessageLevel::Debug,
                text: msg.as_ref().to_string(),
            });
        }
        return;
    }
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
    if report(ProgressEvent::Message {
        level: MessageLevel::Error,
        text: m.to_string(),
    }) {
        return;
    }
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
    fn scoped_reporter_receives_typed_progress_events() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let guard = install_reporter(move |event| captured.lock().unwrap().push(event)).unwrap();

        section("Build");
        let operation = step(OperationKind::Compile, "host-smoke");
        operation.done("ready");
        info("artifact ready");

        let events = events.lock().unwrap();
        assert_eq!(
            events.first(),
            Some(&ProgressEvent::Section("Build".into()))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            ProgressEvent::OperationStarted {
                kind: OperationKind::Compile,
                detail,
            } if detail == "host-smoke"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ProgressEvent::OperationFinished {
                kind: OperationKind::Compile,
                outcome: OperationOutcome::Done,
                summary,
                ..
            } if summary == "ready"
        )));
        drop(events);
        drop(guard);
        assert!(!reporter_active());
    }

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
        assert_eq!(StepKind::Done.glyph(), "✓");
        assert_eq!(StepKind::Fail.glyph(), "✗");
    }

    #[test]
    fn render_step_line_aligns_name_column_at_12_chars() {
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("WHISKER_VERBOSE", "") };
        let line = if is_tty() {
            // `is_tty()` can't be mocked without another abstraction,
            // so an interactive run skips instead of asserting on the
            // colored branch.
            return;
        } else {
            render_step_line("✓", "compile", "hello-world", "6.7s", StepKind::Done)
        };
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
        // `> Configure project :app` blocks are triage material, so a
        // `>` prefix without `Task` must not be folded away.
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
