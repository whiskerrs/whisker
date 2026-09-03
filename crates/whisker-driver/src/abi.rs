//! Compatibility re-export of the raw mobile ABI.
//!
//! The ABI Interface is owned by `whisker-driver-sys`; this safe adapter only
//! converts core protocol values to and from those borrowed representations.

pub use whisker_driver_sys::*;
