//! P2P Network Module
//!
//! Provides peer-to-peer networking capabilities using Kameo remote actors
//! and libp2p for peer discovery and communication within a Kubernetes cluster.
//!
//! This module contains:
//! - `behaviour`: libp2p NetworkBehaviour configuration
//! - `config`: Network configuration from environment
//! - `discovery`: Hybrid etcd-Kademlia peer discovery state machine
//! - `health`: Health monitoring actor and ping loop
//! - `init`: Network initialization
//! - `sharding`: Maglev consistent hashing for machine actor distribution
//! - `swarm`: Swarm builder and event loop
//! - `types`: Shared types (PeerInfo, NetworkState, etc.)

pub mod behaviour;
pub mod config;
pub mod discovery;
pub mod health;
pub mod init;
pub mod sharding;
pub mod swarm;
pub mod types;

pub use behaviour::NeolaasNetworkBehaviour;
pub use config::NetworkConfig;
pub use discovery::{create_etcd_client, DiscoveryCommand, DiscoveryController, DiscoveryState};
pub use health::{HealthActor, PingMessage};
pub use sharding::{LookupMachine, LookupResult, MaglevHasher, ShardingCoordinator, TopologyChanged};
pub use types::{ConnectionStatus, NetworkState, PeerConnectionState, PeerInfo};
