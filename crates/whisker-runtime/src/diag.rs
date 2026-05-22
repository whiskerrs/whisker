//! Cross-platform diagnostic logging that actually reaches the
//! developer console on Android + iOS.
//!
//! `eprintln!` on iOS app builds writes to FD 2, which `xcrun simctl
//! launch` redirects to `/dev/null` — invisible from the launching
//! terminal AND from the unified log. For the async / wake / signal
//! pipeline tracing we need during early Phase 7 debugging we want
//! the output to surface in `xcrun simctl spawn booted log stream`
//! so the developer can actually correlate it with the app's runtime
//! behaviour.
//!
//! Mirrors the `whisker_dev_runtime::devlog` shape (Android →
//! `__android_log_write`, iOS → `syslog`, host → `eprintln!`). Lives
//! in `whisker-runtime` because the modules that need it (async
//! tasks, signal scheduler, host-wake bridge) are all below
//! `whisker-dev-runtime` in the crate graph — taking a dep on
//! dev-runtime would invert the layering.

/// Log a line under the `whisker-rt` tag (Android) /
/// `whisker-rt: <line>` (iOS syslog) / plain stderr (host tests).
///
/// Currently called only from `eprintln!`-replacement sites added
/// during the hn-reader Loading-stuck investigation. Cheap (one
/// libc call) but not free — gate behind compile-time flags before
/// shipping if it becomes a hot path.
pub fn log(line: &str) {
    #[cfg(target_os = "android")]
    {
        // bionic exports __android_log_write(prio, tag, text) → int.
        // ANDROID_LOG_INFO = 4. Both tag and text must be NUL-terminated.
        unsafe extern "C" {
            fn __android_log_write(
                prio: std::os::raw::c_int,
                tag: *const std::os::raw::c_char,
                text: *const std::os::raw::c_char,
            ) -> std::os::raw::c_int;
        }
        const ANDROID_LOG_INFO: std::os::raw::c_int = 4;
        let tag = b"whisker-rt\0";
        let mut buf: Vec<u8> = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line.as_bytes());
        buf.push(0);
        unsafe {
            __android_log_write(
                ANDROID_LOG_INFO,
                tag.as_ptr() as *const _,
                buf.as_ptr() as *const _,
            );
        }
    }
    #[cfg(target_os = "ios")]
    {
        // iOS: stderr from a `simctl launch`'d app goes to /dev/null,
        // but `syslog(3)` reaches the unified log so the developer
        // can tail it with `xcrun simctl spawn booted log stream`.
        unsafe extern "C" {
            fn syslog(priority: std::os::raw::c_int, fmt: *const std::os::raw::c_char, ...);
        }
        const LOG_INFO: std::os::raw::c_int = 6;
        let mut buf: Vec<u8> = Vec::with_capacity(line.len() + 16);
        buf.extend_from_slice(b"[whisker-rt] ");
        buf.extend_from_slice(line.as_bytes());
        buf.push(0);
        let fmt = b"%s\0";
        unsafe {
            syslog(LOG_INFO, fmt.as_ptr() as *const _, buf.as_ptr());
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        eprintln!("[whisker-rt] {line}");
    }
}

/// Convenience: format-style log. `crate::diag::logf!("…{}", x)`.
#[macro_export]
macro_rules! diag_log {
    ($($arg:tt)*) => {
        $crate::diag::log(&format!($($arg)*));
    };
}
