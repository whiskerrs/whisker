//! Development-only runtime extensions for Tuft.
//!
//! Included only when the `tuft/dev` feature is enabled. Provides:
//! - WebSocket client connecting to `tuft dev` server
//! - Tier 1 hot reload: receive rsx delta, patch element tree
//! - Tier 2 hot reload: receive new dylib, swap, migrate state
//! - Red Screen overlay for build/runtime errors
