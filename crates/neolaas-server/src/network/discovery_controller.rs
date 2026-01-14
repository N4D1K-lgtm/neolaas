//! Discovery Controller (Deprecated)
//!
//! DEPRECATED: This module has been reorganized into `discovery/`.
//! This file exists for backward compatibility only.
//!
//! The discovery controller has been split into:
//! - `discovery/controller.rs` - State machine orchestration
//! - `discovery/phases.rs` - Phase implementations
//! - `discovery/etcd.rs` - Etcd operations

#[deprecated(
    since = "0.2.0",
    note = "Use discovery module instead (discovery::DiscoveryController, etc.)"
)]
pub use super::discovery::{
    run_discovery_controller, DiscoveryCommand, DiscoveryController, DiscoveryState, PeerMetadata,
};
