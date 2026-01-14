//! Swarm Module
//!
//! Manages the libp2p swarm including creation, event handling, and lifecycle.

mod builder;
mod event_loop;

pub use builder::build_swarm;
pub use event_loop::{spawn_cleanup_loop, spawn_event_loop};
