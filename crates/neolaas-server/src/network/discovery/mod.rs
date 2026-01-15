//! Discovery Protocol
//!
//! Implements the hybrid etcd-Kademlia discovery protocol with four phases:
//! 1. Initialization: Connect to etcd with retry
//! 2. Registration: Grant lease, spawn keep-alive, PUT peer record
//! 3. Convergence: Fetch all peers, inject into Kademlia routing table
//! 4. Maintenance: Watch for peer changes, handle PUT/DELETE events

mod controller;
mod etcd;
mod phases;

pub use controller::{
    run_discovery_controller, DiscoveryCommand, DiscoveryController, DiscoveryState,
};
pub use etcd::{create_etcd_client, PeerMetadata};
