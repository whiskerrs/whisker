//! Store submission: the App Store Connect and Google Play API
//! clients behind `whisker submit`, plus artifact inspection.
//!
//! Like `whisker-credentials`, this crate has no TTY or UI
//! dependencies — it takes already-decrypted keys and returns
//! results; prompting and progress display are the CLI's job.

pub mod asc;
pub mod ipa;
pub mod play;
