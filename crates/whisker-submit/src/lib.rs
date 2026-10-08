//! Store submission: the App Store Connect and Google Play API
//! clients behind `whisker submit` and `whisker store push`, plus
//! artifact inspection.
//!
//! [`asc`] and [`play`] are the HTTP clients; [`appstore`] and
//! [`playstore`] turn a `store.rs` config into calls on them.
//!
//! Like `whisker-credentials`, this crate has no TTY or UI
//! dependencies — it takes already-decrypted keys and returns
//! results; prompting and progress display are the CLI's job.

pub mod appstore;
pub mod asc;
pub mod ipa;
pub mod play;
pub mod playstore;
