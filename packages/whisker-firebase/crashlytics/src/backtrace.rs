//! Rust backtraces as Crashlytics stack frames.

/// One frame of a reported stack trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    pub symbol: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Capture the current thread's backtrace, skipping frames inside this crate and the
/// panic machinery. Empty unless the binary keeps debug info.
pub(crate) fn capture() -> Vec<StackFrame> {
    parse(&std::backtrace::Backtrace::force_capture().to_string())
}

/// `Backtrace` only exposes frames through its `Display` output, e.g.
/// `  12: app::sync\n             at ./src/sync.rs:12:5`.
pub(crate) fn parse(text: &str) -> Vec<StackFrame> {
    let mut frames: Vec<StackFrame> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(location) = line.strip_prefix("at ") {
            if let Some(frame) = frames.last_mut()
                && frame.file.is_none()
            {
                let mut parts = location.rsplitn(3, ':');
                let (_column, line, file) = (parts.next(), parts.next(), parts.next());
                match (file, line.and_then(|line| line.parse().ok())) {
                    (Some(file), Some(line)) => {
                        frame.file = Some(file.into());
                        frame.line = Some(line);
                    }
                    _ => frame.file = Some(location.into()),
                }
            }
        } else if let Some((index, symbol)) = line.split_once(": ")
            && index.chars().all(|c| c.is_ascii_digit())
        {
            frames.push(StackFrame {
                symbol: symbol.into(),
                file: None,
                line: None,
            });
        }
    }
    let internal = |frame: &StackFrame| {
        [
            "std::backtrace",
            "whisker_firebase_crashlytics::",
            "std::panicking",
            "core::panicking",
        ]
        .iter()
        .any(|prefix| frame.symbol.starts_with(prefix))
            || frame.symbol.contains("rust_begin_unwind")
            || frame.symbol.contains("__rust_")
    };
    let first = frames
        .iter()
        .position(|frame| !internal(frame))
        .unwrap_or(0);
    frames.drain(..first);
    // Without debug info, symbols resolve to the nearest exported function and mislead.
    frames.retain(|frame| frame.line.is_some());
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symbols_and_locations_and_skips_internal_frames() {
        let text = "   0: std::backtrace::Backtrace::force_capture
             at /rustc/abc/library/std/src/backtrace.rs:312:13
   1: whisker_firebase_crashlytics::backtrace::capture
   2: app::sync::run
             at ./src/sync.rs:12:5
   3: <unknown>
";
        let frames = parse(text);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].symbol, "app::sync::run");
        assert_eq!(frames[0].file.as_deref(), Some("./src/sync.rs"));
        assert_eq!(frames[0].line, Some(12));
    }
}
