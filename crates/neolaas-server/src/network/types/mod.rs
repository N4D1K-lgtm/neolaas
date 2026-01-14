//! Network Type Definitions
//!
//! Contains all type definitions for network state and peer management.

mod peer;
mod state;

pub use peer::{ConnectionStatus, PeerInfo};
pub use state::{NetworkState, PeerConnectionState};
