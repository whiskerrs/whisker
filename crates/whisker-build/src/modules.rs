//! Compatibility surface for CNG-owned Whisker module discovery.
//!
//! Dependency-graph interpretation belongs to `whisker-cng`; build-system
//! adapters still import these names through `whisker-build` while the public
//! crates transition without duplicating that policy.

pub use whisker_cng::modules::*;
