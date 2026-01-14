//! Etcd Operations
//!
//! Handles all etcd-specific operations including connection, lease management,
//! and peer metadata storage.

use super::super::config::NetworkConfig;
use anyhow::{anyhow, Result};
use backoff::{future::retry, ExponentialBackoff};
use etcd_client::{Client, LeaseKeepAliveStream, LeaseKeeper};
use http::Uri;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, error, info, trace, warn};

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
    /// Connect to etcd with exponential backoff and proper DNS resolution.
    ///
    /// Uses hickory-resolver to resolve Kubernetes DNS names, which works correctly
    /// in containerized environments where tonic's default resolver may fail.
    pub async fn connect(endpoints: Vec<String>, config: &NetworkConfig) -> Result<Client> {
        let backoff = ExponentialBackoff {
            initial_interval: config.etcd_backoff_initial,
            max_interval: config.etcd_backoff_max,
            max_elapsed_time: Some(config.etcd_backoff_max_elapsed),
            multiplier: config.etcd_backoff_multiplier,
            ..Default::default()
        };

        retry(backoff, || async {
            match create_etcd_client(&endpoints).await {
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
        format!("/kameo/v1/clusters/{}/peers/{}", self.cluster_id, peer_id)
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

/// Create an etcd client with proper DNS resolution for Kubernetes environments.
///
/// This function resolves hostnames using hickory-resolver (which uses the system resolver)
/// before creating the tonic channel, avoiding issues with tonic's internal DNS resolution
/// in containerized environments.
pub async fn create_etcd_client(endpoints: &[String]) -> Result<Client, etcd_client::Error> {
    let channel = create_resolved_channel(endpoints).await?;
    // Wrap the tonic channel in etcd_client's Channel enum
    let etcd_channel = etcd_client::Channel::Tonic(channel);
    Client::from_channel(etcd_channel, None).await
}

/// Create a tonic channel with DNS-resolved endpoints.
///
/// Parses each endpoint URL, resolves any hostnames to IP addresses using the system
/// DNS resolver (via hickory-resolver), and creates a load-balanced tonic channel.
async fn create_resolved_channel(
    endpoints: &[String],
) -> std::result::Result<Channel, etcd_client::Error> {
    use hickory_resolver::TokioResolver;

    // Create resolver using system configuration (/etc/resolv.conf)
    let resolver = TokioResolver::builder_tokio()
        .map_err(|e| etcd_client::Error::IoError(std::io::Error::other(e)))?
        .build();

    let mut resolved_endpoints: Vec<Endpoint> = Vec::new();

    for endpoint_str in endpoints {
        let uri: Uri = endpoint_str
            .parse()
            .map_err(|e: http::uri::InvalidUri| etcd_client::Error::InvalidUri(e))?;

        let scheme = uri.scheme_str().unwrap_or("http");
        let port = uri.port_u16().unwrap_or(2379);

        if let Some(host) = uri.host() {
            // Check if host is already an IP address
            if host.parse::<std::net::IpAddr>().is_ok() {
                // Already an IP, use as-is
                let endpoint = Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
                    etcd_client::Error::IoError(std::io::Error::other(format!(
                        "invalid endpoint: {}",
                        e
                    )))
                })?;
                resolved_endpoints.push(endpoint);
                debug!(endpoint = %endpoint_str, "Using IP endpoint directly");
            } else {
                // Resolve hostname to IP addresses
                match resolver.lookup_ip(host).await {
                    Ok(lookup) => {
                        for ip in lookup.iter() {
                            let ip: std::net::IpAddr = ip;
                            let resolved_uri = if ip.is_ipv6() {
                                format!("{}://[{}]:{}", scheme, ip, port)
                            } else {
                                format!("{}://{}:{}", scheme, ip, port)
                            };
                            let endpoint =
                                Endpoint::from_shared(resolved_uri.clone()).map_err(|e| {
                                    etcd_client::Error::IoError(std::io::Error::other(format!(
                                        "invalid resolved endpoint: {}",
                                        e
                                    )))
                                })?;
                            resolved_endpoints.push(endpoint);
                            info!(
                                original = %host,
                                resolved = %ip,
                                uri = %resolved_uri,
                                "Resolved etcd endpoint"
                            );
                        }
                    }
                    Err(e) => {
                        warn!(host = %host, error = %e, "DNS resolution failed, using original endpoint");
                        // Fall back to original endpoint
                        let endpoint =
                            Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
                                etcd_client::Error::IoError(std::io::Error::other(format!(
                                    "invalid fallback endpoint: {}",
                                    e
                                )))
                            })?;
                        resolved_endpoints.push(endpoint);
                    }
                }
            }
        } else {
            // No host in URI, use as-is
            let endpoint = Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
                etcd_client::Error::IoError(std::io::Error::other(format!(
                    "invalid endpoint without host: {}",
                    e
                )))
            })?;
            resolved_endpoints.push(endpoint);
        }
    }

    if resolved_endpoints.is_empty() {
        return Err(etcd_client::Error::InvalidArgs("no valid endpoints".into()));
    }

    // Create a load-balanced channel from the resolved endpoints
    let (channel, tx) = Channel::balance_channel(64);

    for endpoint in resolved_endpoints {
        let uri = endpoint.uri().clone();
        tx.send(tonic::transport::channel::Change::Insert(uri, endpoint))
            .await
            .map_err(|e| {
                etcd_client::Error::IoError(std::io::Error::other(format!(
                    "failed to add endpoint: {}",
                    e
                )))
            })?;
    }

    Ok(channel)
}
