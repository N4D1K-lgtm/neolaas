//! ShardingCoordinator Message Types
//!
//! Messages for querying shard ownership and updating topology.

use kameo::Reply;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

/// Query which node owns a machine.
///
/// The ShardingCoordinator will hash the machine_id and
/// return the owning peer from the Maglev table.
#[derive(Debug, Clone)]
pub struct LookupMachine {
    pub machine_id: String,
}

/// Result of a machine lookup.
#[derive(Debug, Clone, Reply)]
pub struct LookupResult {
    /// The machine ID that was looked up
    pub machine_id: String,
    /// The peer that owns this machine (None if no nodes)
    pub owner_peer: Option<PeerId>,
    /// Whether the owner is the local node
    pub is_local: bool,
}

/// Topology change notification.
///
/// Sent by the DiscoveryController when peers join or leave.
/// Contains the complete current peer list.
#[derive(Debug, Clone)]
pub struct TopologyChanged {
    /// Complete list of currently active peers
    pub peers: Vec<PeerId>,
}

/// Acknowledgement of topology change.
#[derive(Debug, Clone, Reply)]
pub struct TopologyAck {
    /// Number of nodes before the change
    pub previous_count: usize,
    /// Number of nodes after the change
    pub new_count: usize,
}

/// Request current shard statistics.
#[derive(Debug, Clone)]
pub struct GetShardStats;

/// Shard statistics response.
#[derive(Debug, Clone, Serialize, Deserialize, Reply)]
pub struct ShardStats {
    /// Number of nodes in the cluster
    pub node_count: usize,
    /// Local peer ID (base58 encoded)
    pub local_peer_id: String,
}
