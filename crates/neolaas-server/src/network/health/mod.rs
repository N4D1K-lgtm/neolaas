//! Health Monitoring Module
//!
//! Provides peer health monitoring through ping/pong actor messaging.

mod actor;
mod messages;
mod ping_loop;

#[allow(deprecated)]
pub use actor::{HealthActor, PingActor}; // PingActor is deprecated alias
pub use messages::{
    AckReply, BroadcastMessage, GetStatsMessage, PingMessage, PongReply, StatsReply,
};
pub use ping_loop::spawn_ping_loop;
