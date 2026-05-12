//! Development-only runtime extensions for Lyra.
//!
//! Included only when the `lyra/dev` feature is enabled. Provides:
//! - WebSocket client connecting to `lyra dev` server
//! - Tier 1 hot reload: receive rsx delta, patch element tree
//! - Tier 2 hot reload: receive new dylib, swap, migrate state
//! - Red Screen overlay for build/runtime errors
