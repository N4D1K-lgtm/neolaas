//! Discovery Protocol Phases
//!
//! Implements the four phases of the hybrid etcd-Kademlia discovery protocol:
//! 1. Initialization - Connect to etcd
//! 2. Registration - Register peer with lease
//! 3. Convergence - Fetch all peers and inject into Kademlia
//! 4. Maintenance - Watch for peer changes

use super::super::config::NetworkConfig;
use super::etcd::{EtcdOperations, PeerMetadata};
use super::DiscoveryCommand;
use anyhow::{anyhow, Result};
use etcd_client::{
    EventType, GetOptions, LeaseKeepAliveStream, LeaseKeeper, PutOptions, WatchOptions,
    WatchStream,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Helper struct for managing discovery phases
pub struct DiscoveryPhases {
    pub etcd_ops: EtcdOperations,
    pub local_peer_id: PeerId,
    pub local_addr: Multiaddr,
    pub swarm_tx: mpsc::UnboundedSender<DiscoveryCommand>,
    pub readiness: Arc<std::sync::atomic::AtomicBool>,
    pub config: NetworkConfig,
    /// Known peers (maintained for sharding updates)
    known_peers: HashSet<PeerId>,
    /// Channel to notify ShardingCoordinator of topology changes
    sharding_tx: Option<mpsc::UnboundedSender<Vec<PeerId>>>,
}

impl DiscoveryPhases {
    pub fn new(
        etcd_ops: EtcdOperations,
        local_peer_id: PeerId,
        local_addr: Multiaddr,
        swarm_tx: mpsc::UnboundedSender<DiscoveryCommand>,
        readiness: Arc<std::sync::atomic::AtomicBool>,
        config: NetworkConfig,
        sharding_tx: Option<mpsc::UnboundedSender<Vec<PeerId>>>,
    ) -> Self {
        // Start with local peer in the known set
        let mut known_peers = HashSet::new();
        known_peers.insert(local_peer_id);

        Self {
            etcd_ops,
            local_peer_id,
            local_addr,
            swarm_tx,
            readiness,
            config,
            known_peers,
            sharding_tx,
        }
    }

    /// Notify the ShardingCoordinator of topology changes
    fn notify_sharding(&self) {
        if let Some(tx) = &self.sharding_tx {
            let peers: Vec<PeerId> = self.known_peers.iter().copied().collect();
            debug!(peer_count = peers.len(), "Notifying ShardingCoordinator of topology change");
            if tx.send(peers).is_err() {
                warn!("Failed to send topology update to ShardingCoordinator");
            }
        }
    }

    /// Phase 1: Verify etcd connection is ready.
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Discovery: Phase 1 - Initialization");

        match self.etcd_ops.client.status().await {
            Ok(status) => {
                debug!(version = %status.version(), "etcd connection verified");
            }
            Err(e) => {
                error!(error = %e, "etcd connection check failed");
                return Err(anyhow!("Etcd connection failed: {}", e));
            }
        }

        Ok(())
    }

    /// Phase 2: Grant lease, start keep-alive, register peer in etcd.
    pub async fn register(&mut self) -> Result<(i64, LeaseKeeper, LeaseKeepAliveStream)> {
        info!("Discovery: Phase 2 - Registration");

        let lease_resp = self
            .etcd_ops
            .client
            .lease_grant(self.config.lease_ttl, None)
            .await?;
        let lease_id = lease_resp.id();
        debug!(
            lease_id = lease_id,
            ttl = self.config.lease_ttl,
            "Lease granted"
        );

        let (keeper, stream) = self.etcd_ops.client.lease_keep_alive(lease_id).await?;

        let key = self.etcd_ops.peer_key(&self.local_peer_id);
        let metadata = PeerMetadata {
            multiaddr: self.local_addr.to_string(),
            meta: HashMap::new(),
            shard: 0,
        };
        let value = serde_json::to_string(&metadata)?;

        let put_options = PutOptions::new().with_lease(lease_id);
        self.etcd_ops
            .client
            .put(key.clone(), value, Some(put_options))
            .await?;

        debug!(key = %key, "Peer registered in etcd");
        Ok((lease_id, keeper, stream))
    }

    /// Phase 3: Fetch all peers from etcd and inject into Kademlia routing table.
    pub async fn converge(&mut self) -> Result<()> {
        info!("Discovery: Phase 3 - Convergence");

        let prefix = self.etcd_ops.peers_prefix();
        let get_options = GetOptions::new().with_prefix();
        let resp = self
            .etcd_ops
            .client
            .get(prefix, Some(get_options))
            .await?;

        let mut peer_count = 0;
        for kv in resp.kvs() {
            let key = kv.key_str()?;
            let value = kv.value_str()?;

            if let Some(peer_id_str) = key.strip_prefix(&self.etcd_ops.peers_prefix()) {
                match peer_id_str.parse::<PeerId>() {
                    Ok(peer_id) => {
                        if peer_id == self.local_peer_id {
                            continue;
                        }

                        match serde_json::from_str::<PeerMetadata>(value) {
                            Ok(metadata) => {
                                match metadata.multiaddr.parse::<Multiaddr>() {
                                    Ok(addr) => {
                                        debug!(
                                            peer_id_short = &peer_id.to_base58()[46..],
                                            addr = %addr,
                                            "Convergence: discovered peer"
                                        );

                                        if let Err(e) =
                                            self.swarm_tx.send(DiscoveryCommand::AddPeer {
                                                peer_id,
                                                addr,
                                            })
                                        {
                                            error!(error = %e, "Failed to send AddPeer command");
                                        }
                                        self.known_peers.insert(peer_id);
                                        peer_count += 1;
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Invalid multiaddr in peer metadata");
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "Invalid peer metadata JSON");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, key = %key, "Invalid peer ID in etcd key");
                    }
                }
            }
        }

        info!(peer_count = peer_count, "Convergence complete");

        // Notify ShardingCoordinator of initial topology
        self.notify_sharding();

        self.readiness
            .store(true, std::sync::atomic::Ordering::Release);
        info!("Node ready to accept traffic");

        Ok(())
    }

    /// Phase 4: Open watch stream on peers prefix for real-time updates.
    pub async fn enter_maintenance(&mut self) -> Result<WatchStream> {
        info!("Discovery: Phase 4 - Maintenance");

        let prefix = self.etcd_ops.peers_prefix();
        let watch_options = WatchOptions::new().with_prefix();
        let stream = self
            .etcd_ops
            .client
            .watch(prefix, Some(watch_options))
            .await?;

        debug!("Watch stream established");
        Ok(stream)
    }

    /// Process a single watch event from etcd.
    /// Returns immediately after processing one message to allow shutdown signal handling.
    pub async fn process_watch_event(&mut self, stream: &mut WatchStream) -> Result<()> {
        let peers_prefix = self.etcd_ops.peers_prefix();
        let local_peer_id = self.local_peer_id;
        let swarm_tx = self.swarm_tx.clone();

        if let Some(resp) = stream.message().await? {
            if resp.canceled() {
                warn!("Watch stream canceled, will reconnect");
                return Err(anyhow!("Watch stream canceled"));
            }

            for event in resp.events() {
                match event.event_type() {
                    EventType::Put => {
                        if let Some(kv) = event.kv() {
                            let key = kv.key_str()?;
                            let value = kv.value_str()?;

                            if let Some(peer_id_str) = key.strip_prefix(&peers_prefix) {
                                if let Ok(peer_id) = peer_id_str.parse::<PeerId>() {
                                    if peer_id == local_peer_id {
                                        continue;
                                    }

                                    if let Ok(metadata) =
                                        serde_json::from_str::<PeerMetadata>(value)
                                    {
                                        if let Ok(addr) = metadata.multiaddr.parse::<Multiaddr>() {
                                            debug!(
                                                peer_id_short = &peer_id.to_base58()[46..],
                                                addr = %addr,
                                                "Watch: peer joined"
                                            );
                                            let _ =
                                                swarm_tx.send(DiscoveryCommand::AddPeer { peer_id, addr });

                                            // Track peer and notify sharding
                                            self.known_peers.insert(peer_id);
                                            self.notify_sharding();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    EventType::Delete => {
                        if let Some(kv) = event.kv() {
                            let key = kv.key_str()?;

                            if let Some(peer_id_str) = key.strip_prefix(&peers_prefix) {
                                if let Ok(peer_id) = peer_id_str.parse::<PeerId>() {
                                    debug!(
                                        peer_id_short = &peer_id.to_base58()[46..],
                                        "Watch: peer departed"
                                    );
                                    let _ = swarm_tx.send(DiscoveryCommand::RemovePeer { peer_id });

                                    // Remove peer and notify sharding
                                    self.known_peers.remove(&peer_id);
                                    self.notify_sharding();
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
