//! Etcd Operations
//!
//! Handles all etcd-specific operations including connection, lease management,
//! and peer metadata storage.

use super::super::config::NetworkConfig;
use anyhow::{anyhow, Result};
use backoff::{future::retry, ExponentialBackoff};
use etcd_client::{Client, LeaseKeepAliveStream, LeaseKeeper};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};

/// Peer metadata stored in etcd
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMetadata {
    /// Full libp2p multiaddress
    pub multiaddr: String,
    /// Application-specific metadata
    #[serde(default)]
    pub meta: HashMap<String, String>,
    /// Shard identifier (for sharding strategies)
    #[serde(default)]
    pub shard: u32,
}

/// Etcd operations helper
pub struct EtcdOperations {
    pub client: Client,
    pub cluster_id: String,
}

impl EtcdOperations {
    /// Connect to etcd with exponential backoff
    pub async fn connect(endpoints: Vec<String>, config: &NetworkConfig) -> Result<Client> {
        let backoff = ExponentialBackoff {
            initial_interval: config.etcd_backoff_initial,
            max_interval: config.etcd_backoff_max,
            max_elapsed_time: Some(config.etcd_backoff_max_elapsed),
            multiplier: config.etcd_backoff_multiplier,
            ..Default::default()
        };

        retry(backoff, || async {
            match Client::connect(&endpoints, None).await {
                Ok(client) => {
                    debug!("Connected to etcd");
                    Ok(client)
                }
                Err(e) => {
                    warn!(error = %e, "etcd connection failed, retrying");
                    Err(backoff::Error::transient(e))
                }
            }
        })
        .await
        .map_err(|e| anyhow!("Failed to connect to etcd after retries: {:?}", e))
    }

    /// Get the etcd key for a peer
    pub fn peer_key(&self, peer_id: &PeerId) -> String {
        format!(
            "/kameo/v1/clusters/{}/peers/{}",
            self.cluster_id, peer_id
        )
    }

    /// Get the etcd prefix for all peers in the cluster
    pub fn peers_prefix(&self) -> String {
        format!("/kameo/v1/clusters/{}/peers/", self.cluster_id)
    }
}

/// Spawn background task to send periodic keep-alive requests for the lease.
/// Signals internal shutdown if keep-alive fails (node becomes invisible to cluster).
pub fn spawn_keepalive_task(
    mut keeper: LeaseKeeper,
    mut keeper_stream: LeaseKeepAliveStream,
    lease_id: i64,
    config: &NetworkConfig,
    shutdown_tx: mpsc::UnboundedSender<()>,
) -> tokio::task::JoinHandle<()> {
    let keepalive_interval = config.keepalive_interval;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(keepalive_interval);
        interval.tick().await; // Skip first immediate tick

        loop {
            interval.tick().await;

            if let Err(e) = keeper.keep_alive().await {
                error!(lease_id = lease_id, error = %e, "Keep-alive send failed");
                let _ = shutdown_tx.send(());
                break;
            }

            match keeper_stream.message().await {
                Ok(Some(resp)) => {
                    trace!(lease_id = lease_id, ttl = resp.ttl(), "Keep-alive OK");
                }
                Ok(None) => {
                    error!(lease_id = lease_id, "Keep-alive stream closed");
                    let _ = shutdown_tx.send(());
                    break;
                }
                Err(e) => {
                    error!(lease_id = lease_id, error = %e, "Keep-alive failed");
                    let _ = shutdown_tx.send(());
                    break;
                }
            }
        }
    })
}
