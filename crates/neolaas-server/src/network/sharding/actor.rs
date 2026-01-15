//! ShardingCoordinator Actor
//!
//! Manages the Maglev hash ring and determines machine-to-node ownership.

use super::maglev::MaglevHasher;
use super::messages::{GetShardStats, LookupMachine, LookupResult, ShardStats, TopologyAck, TopologyChanged};
use kameo::{
    message::{Context, Message},
    Actor,
};
use libp2p::PeerId;
use tracing::{debug, info};

/// Coordinates machine actor sharding across cluster nodes.
///
/// Uses Maglev consistent hashing to map machine IDs to owner nodes
/// with minimal disruption during topology changes.
#[derive(Actor)]
pub struct ShardingCoordinator {
    /// Local peer ID
    local_peer_id: PeerId,
    /// Maglev hash table
    hasher: MaglevHasher,
}

impl ShardingCoordinator {
    /// Create a new ShardingCoordinator.
    ///
    /// The local peer should be included in the initial_peers list.
    pub fn new(local_peer_id: PeerId, initial_peers: Vec<PeerId>) -> Self {
        info!(
            local_peer = %local_peer_id.to_base58()[46..],
            peer_count = initial_peers.len(),
            "Initializing ShardingCoordinator"
        );

        Self {
            local_peer_id,
            hasher: MaglevHasher::new(initial_peers),
        }
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }
}

impl Message<LookupMachine> for ShardingCoordinator {
    type Reply = LookupResult;

    async fn handle(
        &mut self,
        msg: LookupMachine,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let owner_peer = self.hasher.lookup(&msg.machine_id).copied();
        let is_local = owner_peer == Some(self.local_peer_id);

        let owner_short = owner_peer.map(|p| {
            let b58 = p.to_base58();
            b58[46..].to_string()
        });

        debug!(
            machine_id = %msg.machine_id,
            owner = ?owner_short,
            is_local = is_local,
            "Machine lookup"
        );

        LookupResult {
            machine_id: msg.machine_id,
            owner_peer,
            is_local,
        }
    }
}

impl Message<TopologyChanged> for ShardingCoordinator {
    type Reply = TopologyAck;

    async fn handle(
        &mut self,
        msg: TopologyChanged,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let previous_count = self.hasher.node_count();
        let new_count = msg.peers.len();

        info!(
            previous = previous_count,
            new = new_count,
            "Topology change: rebuilding Maglev table"
        );

        self.hasher.rebuild(msg.peers);

        TopologyAck {
            previous_count,
            new_count,
        }
    }
}

impl Message<GetShardStats> for ShardingCoordinator {
    type Reply = ShardStats;

    async fn handle(
        &mut self,
        _msg: GetShardStats,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        ShardStats {
            node_count: self.hasher.node_count(),
            local_peer_id: self.local_peer_id.to_base58(),
        }
    }
}
