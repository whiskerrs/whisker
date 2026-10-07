//! Firebase Crashlytics for Whisker on Android and iOS (default app).
//!
//! ```ignore
//! use whisker_firebase::crashlytics::Crashlytics;
//!
//! let crashlytics = Crashlytics::instance()?;
//! crashlytics.record_panics(); // report Rust panics as crashes with their message
//! crashlytics.set_user_id("user-42")?;
//! crashlytics.set_custom_key("plan", "pro")?;
//! crashlytics.log("opened settings")?;
//! if let Err(error) = sync_inbox().await {
//!     crashlytics.record_error(&error)?; // non-fatal, with the Rust backtrace
//! }
//! ```
//!
//! Native crashes, including the abort that follows a Rust panic, are reported by the
//! SDKs on the next launch. Adding this crate installs the Crashlytics Gradle plugin and
//! NDK library on Android, and a Release-only dSYM upload phase on iOS.
mod backtrace;
mod plugin;

pub use backtrace::StackFrame;
pub use plugin::{WhiskerFirebaseCrashlytics, WhiskerFirebaseCrashlyticsConfig};
pub use whisker_firebase_core::{FirebaseError, Result};

use std::cell::Cell;
use std::sync::Once;
use whisker::platform_module::WhiskerValue as Wire;
use whisker_firebase_core::__private::unwrap_response;
use whisker_firebase_core::FirebaseApp;

const SERVICE: &str = "crashlytics";

fn module() -> whisker::PlatformModule {
    whisker::module!("FirebaseCrashlytics")
}

fn invoke(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke(method, args))
}

fn unit(method: &str, args: Vec<Wire>) -> Result<()> {
    match invoke(method, args)? {
        Wire::Null => Ok(()),
        _ => Err(response("expected an empty result")),
    }
}

fn boolean(wire: Wire) -> Result<bool> {
    match wire {
        Wire::Bool(value) => Ok(value),
        _ => Err(response("expected a boolean")),
    }
}

fn response(message: &str) -> FirebaseError {
    FirebaseError::invalid_response(SERVICE, message)
}

/// Firebase Crashlytics for the default app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crashlytics {
    _private: (),
}

impl Crashlytics {
    /// Initialize the default Firebase app if needed and return its Crashlytics instance.
    pub fn instance() -> Result<Self> {
        FirebaseApp::initialize()?;
        Ok(Self { _private: () })
    }

    /// Add a breadcrumb included with the next crash or non-fatal report.
    pub fn log(&self, message: &str) -> Result<()> {
        unit("log", vec![Wire::String(message.into())])
    }

    pub fn set_user_id(&self, id: &str) -> Result<()> {
        unit("setUserId", vec![Wire::String(id.into())])
    }

    /// Attach a key/value to later reports (up to 64 keys).
    pub fn set_custom_key(&self, key: &str, value: impl Into<CustomValue>) -> Result<()> {
        let value = match value.into() {
            CustomValue::String(value) => Wire::String(value),
            CustomValue::Integer(value) => Wire::Int(value),
            CustomValue::Double(value) => Wire::Float(value),
            CustomValue::Bool(value) => Wire::Bool(value),
        };
        unit("setCustomKey", vec![Wire::String(key.into()), value])
    }

    /// Report a non-fatal error with its `source()` chain and the current Rust backtrace.
    pub fn record_error<E: std::error::Error + ?Sized>(&self, error: &E) -> Result<()> {
        let mut reason = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            reason.push_str(&format!("\nCaused by: {cause}"));
            source = cause.source();
        }
        self.record_report(&ErrorReport {
            name: short_type_name::<E>().into(),
            reason,
            frames: backtrace::capture(),
        })
    }

    /// Report a non-fatal error with explicit contents.
    pub fn record_report(&self, report: &ErrorReport) -> Result<()> {
        unit("recordError", vec![report.encode()])
    }

    /// Report each Rust panic as the app's crash, named `RustPanic` with the panic message,
    /// location, and backtrace, instead of an anonymous abort. Idempotent. The report is
    /// written synchronously through the SDK's crash handler, which then ends the process;
    /// a panic raised while a native call is already in progress may only produce the abort.
    pub fn record_panics(&self) {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                record_panic(info);
                previous(info);
            }));
        });
    }

    /// Whether reports are sent automatically. Persisted across launches.
    pub fn is_collection_enabled(&self) -> Result<bool> {
        boolean(invoke("isCollectionEnabled", vec![])?)
    }

    /// Persisted across launches. When disabled, crash reports stay on the device until
    /// [`send_unsent_reports`](Self::send_unsent_reports) or collection is re-enabled.
    pub fn set_collection_enabled(&self, enabled: bool) -> Result<()> {
        unit("setCollectionEnabled", vec![Wire::Bool(enabled)])
    }

    /// Whether reports from earlier crashes are waiting to be sent (collection disabled).
    pub async fn check_for_unsent_reports(&self) -> Result<bool> {
        boolean(unwrap_response(
            SERVICE,
            module().invoke_async("checkForUnsentReports", vec![]).await,
        )?)
    }

    pub fn send_unsent_reports(&self) -> Result<()> {
        unit("sendUnsentReports", vec![])
    }

    pub fn delete_unsent_reports(&self) -> Result<()> {
        unit("deleteUnsentReports", vec![])
    }

    /// Whether the previous run of the app ended in a crash.
    pub fn did_crash_on_previous_execution(&self) -> Result<bool> {
        boolean(invoke("didCrashOnPreviousExecution", vec![])?)
    }

    /// Crash the app from native code to test the setup. The report arrives on the next launch;
    /// on iOS, run without the debugger attached.
    pub fn crash(&self) -> ! {
        let _ = module().invoke("crash", vec![]);
        std::process::abort()
    }
}

/// A custom key value.
#[derive(Debug, Clone, PartialEq)]
pub enum CustomValue {
    String(String),
    Integer(i64),
    Double(f64),
    Bool(bool),
}

macro_rules! custom_from {
    ($variant:ident: $($ty:ty),*) => {$(
        impl From<$ty> for CustomValue {
            fn from(value: $ty) -> Self {
                CustomValue::$variant(value.into())
            }
        }
    )*};
}
custom_from!(String: String, &str, &String);
custom_from!(Integer: i8, i16, i32, i64, u8, u16, u32);
custom_from!(Double: f32, f64);
custom_from!(Bool: bool);

/// The contents of a non-fatal report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    /// Groups reports in the console, like an exception class name.
    pub name: String,
    pub reason: String,
    /// Innermost frame first.
    pub frames: Vec<StackFrame>,
}

impl ErrorReport {
    fn encode(&self) -> Wire {
        let frames = self
            .frames
            .iter()
            .map(|frame| {
                Wire::Map(
                    [
                        ("symbol".to_owned(), Wire::String(frame.symbol.clone())),
                        (
                            "file".to_owned(),
                            frame.file.clone().map_or(Wire::Null, Wire::String),
                        ),
                        (
                            "line".to_owned(),
                            frame.line.map_or(Wire::Null, |line| Wire::Int(line.into())),
                        ),
                    ]
                    .into(),
                )
            })
            .collect();
        Wire::Map(
            [
                ("name".to_owned(), Wire::String(self.name.clone())),
                ("reason".to_owned(), Wire::String(self.reason.clone())),
                ("frames".to_owned(), Wire::Array(frames)),
            ]
            .into(),
        )
    }
}

fn short_type_name<T: ?Sized>() -> &'static str {
    let name = std::any::type_name::<T>();
    let base = name.split('<').next().unwrap_or(name);
    base.rsplit("::").next().unwrap_or(base)
}

thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

fn record_panic(info: &std::panic::PanicHookInfo<'_>) {
    // A panic inside the recording call itself must not recurse.
    if IN_PANIC_HOOK.with(|flag| flag.replace(true)) {
        return;
    }
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Box<dyn Any>".into());
    let location = info
        .location()
        .map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_default();
    let report = ErrorReport {
        name: "RustPanic".into(),
        reason: format!("{message}{location}"),
        frames: backtrace::capture(),
    };
    let _ = std::panic::catch_unwind(|| module().invoke("recordPanic", vec![report.encode()]));
    IN_PANIC_HOOK.with(|flag| flag.set(false));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Outer(std::io::Error);
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("sync failed")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn reports_name_the_error_type_and_encode_frames() {
        assert_eq!(short_type_name::<Outer>(), "Outer");
        assert_eq!(short_type_name::<Vec<String>>(), "Vec");
        let report = ErrorReport {
            name: "Outer".into(),
            reason: "sync failed".into(),
            frames: vec![StackFrame {
                symbol: "app::sync".into(),
                file: Some("src/sync.rs".into()),
                line: Some(12),
            }],
        };
        let Wire::Map(fields) = report.encode() else {
            panic!()
        };
        let Wire::Array(frames) = &fields["frames"] else {
            panic!()
        };
        assert_eq!(frames.len(), 1);
    }
}
