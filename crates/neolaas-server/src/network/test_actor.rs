//! Test Actor (Deprecated)
//!
//! DEPRECATED: This module has been reorganized into `health/`.
//! This file exists for backward compatibility only.
//!
//! The test actor has been renamed to HealthActor and split into:
//! - `health/actor.rs` - HealthActor implementation
//! - `health/messages.rs` - Message types
//! - `health/ping_loop.rs` - Periodic ping loop

#[deprecated(
    since = "0.2.0",
    note = "Use health module instead (health::HealthActor, health::PingMessage, etc.)"
)]
pub use super::health::{
    AckReply, BroadcastMessage, GetStatsMessage, HealthActor as PingActor, PingMessage,
    PongReply, StatsReply,
};
