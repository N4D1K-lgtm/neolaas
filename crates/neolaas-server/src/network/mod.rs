//! P2P Network Module
//!
//! Provides peer-to-peer networking capabilities using Kameo remote actors
//! and libp2p for peer discovery and communication within a Kubernetes cluster.
//!
//! This module contains:
//! - `init`: Network initialization and swarm event loop
//! - `behaviour`: libp2p NetworkBehaviour configuration
//! - `discovery_controller`: Hybrid etcd-Kademlia peer discovery state machine
//! - `test_actor`: Simple ping/pong actor for health checking

pub mod behaviour;
pub mod config;
pub mod discovery;
pub mod discovery_controller; // Deprecated - for backward compatibility
pub mod health;
pub mod init;
pub mod swarm;
pub mod test_actor; // Deprecated - for backward compatibility
pub mod types;

pub use behaviour::NeolaasNetworkBehaviour;
pub use config::NetworkConfig;
pub use discovery::{DiscoveryCommand, DiscoveryController, DiscoveryState};
#[allow(deprecated)]
pub use health::{HealthActor, PingActor, PingMessage};
pub use types::{ConnectionStatus, NetworkState, PeerConnectionState, PeerInfo};
