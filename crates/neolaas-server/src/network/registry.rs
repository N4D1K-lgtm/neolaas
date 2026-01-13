//! Etcd-based peer registry for P2P discovery
//!
//! Each peer registers itself in etcd with its P2P address.
//! Peers discover each other by querying etcd.

use anyhow::Result;
use etcd_client::{Client, PutOptions};
use libp2p::{Multiaddr, PeerId};
use std::time::Duration;
use tracing::{debug, error, info, warn};

const PEER_REGISTRY_PREFIX: &str = "/neolaas/peers/";
const LEASE_TTL: i64 = 10; // 10 seconds

/// Etcd-based peer registry
pub struct PeerRegistry {
    client: Client,
    local_peer_id: PeerId,
    local_addr: Multiaddr,
}

impl PeerRegistry {
    pub async fn new(
        etcd_endpoints: Vec<String>,
        local_peer_id: PeerId,
        local_addr: Multiaddr,
    ) -> Result<Self> {
        let client = Client::connect(etcd_endpoints, None).await?;
        Ok(Self {
            client,
            local_peer_id,
            local_addr,
        })
    }

    /// Register this peer in etcd with a lease
    pub async fn register(&mut self) -> Result<()> {
        let key = format!("{}{}", PEER_REGISTRY_PREFIX, self.local_peer_id);
        let value = self.local_addr.to_string();

        // Create a lease
        let lease = self.client.lease_grant(LEASE_TTL, None).await?;
        let lease_id = lease.id();

        info!(
            target: "neolaas::p2p::registry",
            peer_id = %self.local_peer_id,
            peer_id_short = &self.local_peer_id.to_base58()[46..],
            addr = %self.local_addr,
            lease_id = lease_id,
            "Registering peer in etcd"
        );

        // Put with lease
        let options = PutOptions::new().with_lease(lease_id);
        self.client.put(key.clone(), value, Some(options)).await?;

        // Spawn keep-alive task for the lease
        let mut client_for_keepalive = self.client.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs((LEASE_TTL / 2) as u64));
            loop {
                interval.tick().await;
                match client_for_keepalive.lease_keep_alive(lease_id).await {
                    Ok((_keeper, mut stream)) => {
                        if let Err(e) = stream.message().await {
                            error!(
                                target: "neolaas::p2p::registry",
                                error = %e,
                                "Failed to keep lease alive"
                            );
                            break;
                        }
                    }
                    Err(e) => {
                        error!(
                            target: "neolaas::p2p::registry",
                            error = %e,
                            "Failed to keep lease alive"
                        );
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Discover all registered peers
    pub async fn discover_peers(&mut self) -> Result<Vec<(PeerId, Multiaddr)>> {
        debug!(
            target: "neolaas::p2p::registry",
            "Discovering peers from etcd"
        );

        let resp = self
            .client
            .get(PEER_REGISTRY_PREFIX, Some(etcd_client::GetOptions::new().with_prefix()))
            .await?;

        let mut peers = Vec::new();
        for kv in resp.kvs() {
            let key = kv.key_str()?;
            let value = kv.value_str()?;

            // Extract peer ID from key
            if let Some(peer_id_str) = key.strip_prefix(PEER_REGISTRY_PREFIX) {
                match peer_id_str.parse::<PeerId>() {
                    Ok(peer_id) => {
                        // Skip self
                        if peer_id == self.local_peer_id {
                            continue;
                        }

                        match value.parse::<Multiaddr>() {
                            Ok(addr) => {
                                debug!(
                                    target: "neolaas::p2p::registry",
                                    peer_id = %peer_id,
                                    peer_id_short = &peer_id.to_base58()[46..],
                                    addr = %addr,
                                    "Discovered peer"
                                );
                                peers.push((peer_id, addr));
                            }
                            Err(e) => {
                                warn!(
                                    target: "neolaas::p2p::registry",
                                    error = %e,
                                    value = %value,
                                    "Invalid multiaddr in registry"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            target: "neolaas::p2p::registry",
                            error = %e,
                            key = %key,
                            "Invalid peer ID in registry"
                        );
                    }
                }
            }
        }

        info!(
            target: "neolaas::p2p::registry",
            peer_count = peers.len(),
            "Discovered peers from etcd"
        );

        Ok(peers)
    }
}
