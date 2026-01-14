//! Discovery Controller
//!
//! Implements the hybrid etcd-Kademlia discovery protocol state machine as described
//! in the design document. The protocol has four phases:
//!
//! 1. Initialization: Connect to etcd with retry
//! 2. Registration: Grant lease, spawn keep-alive, PUT peer record
//! 3. Convergence: Fetch all peers, inject into Kademlia routing table
//! 4. Maintenance: Watch for peer changes, handle PUT/DELETE events
//!
//! The controller uses etcd leases for automatic cleanup when peers crash.

use anyhow::{anyhow, Result};
use backoff::{future::retry, ExponentialBackoff};
use etcd_client::{
    Client, EventType, GetOptions, LeaseKeepAliveStream, LeaseKeeper, PutOptions, WatchOptions,
    WatchStream,
};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// TTL for etcd leases. Peers that crash will be automatically removed after this duration.
const LEASE_TTL: i64 = 15;

/// Keep-alive interval set to 1/3 of TTL to ensure lease renewal before expiration.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

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

/// Discovery controller state machine states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryState {
    /// Initial state: generating identity and connecting to etcd
    Initialization,
    /// Registering peer in etcd with lease
    Registration,
    /// Performing initial sync of all peers
    Convergence,
    /// Steady-state: watching for peer changes
    Maintenance,
    /// Graceful shutdown in progress
    Terminating,
}

/// Commands sent to the discovery controller
#[derive(Debug)]
pub enum DiscoveryCommand {
    /// Add or update a peer in the routing table
    AddPeer { peer_id: PeerId, addr: Multiaddr },
    /// Remove a peer from the routing table
    RemovePeer { peer_id: PeerId },
    /// Initiate graceful shutdown
    Shutdown,
}

/// Discovery Controller
///
/// Manages the hybrid etcd-Kademlia discovery protocol.
/// Implements the state machine: Initialization -> Registration -> Convergence -> Maintenance
pub struct DiscoveryController {
    /// Cluster identifier for namespace isolation
    cluster_id: String,
    /// Local peer ID
    local_peer_id: PeerId,
    /// Local multiaddress
    local_addr: Multiaddr,
    /// Etcd client
    etcd_client: Client,
    /// Current lease ID
    lease_id: Option<i64>,
    /// Lease keeper for keep-alive
    lease_keeper: Option<(LeaseKeeper, LeaseKeepAliveStream)>,
    /// Watch stream for peer events
    watch_stream: Option<WatchStream>,
    /// Channel for sending commands to swarm
    swarm_tx: mpsc::UnboundedSender<DiscoveryCommand>,
    /// Channel for receiving shutdown signal (None after extracted in run())
    shutdown_rx: Option<mpsc::UnboundedReceiver<()>>,
    /// Channel for sending internal shutdown signals (from keep-alive task)
    internal_shutdown_tx: mpsc::UnboundedSender<()>,
    /// Channel for receiving internal shutdown signals (None after extracted in run())
    internal_shutdown_rx: Option<mpsc::UnboundedReceiver<()>>,
    /// Shared readiness state (true when convergence phase completes)
    readiness: Arc<std::sync::atomic::AtomicBool>,
    /// Current state
    state: DiscoveryState,
}

impl DiscoveryController {
    pub async fn new(
        cluster_id: String,
        local_peer_id: PeerId,
        local_addr: Multiaddr,
        etcd_endpoints: Vec<String>,
        swarm_tx: mpsc::UnboundedSender<DiscoveryCommand>,
        shutdown_rx: mpsc::UnboundedReceiver<()>,
        readiness: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Self> {
        debug!(
            cluster_id = %cluster_id,
            peer_id_short = &local_peer_id.to_base58()[46..],
            "Creating discovery controller"
        );

        // Connect to etcd with exponential backoff (handles pods starting before etcd)
        let etcd_client = {
            let endpoints = etcd_endpoints.clone();
            let backoff = ExponentialBackoff {
                initial_interval: Duration::from_secs(1),
                max_interval: Duration::from_secs(10),
                max_elapsed_time: Some(Duration::from_secs(60)),
                multiplier: 2.0,
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
            .map_err(|e| anyhow!("Failed to connect to etcd after retries: {:?}", e))?
        };

        let (internal_shutdown_tx, internal_shutdown_rx) = mpsc::unbounded_channel();

        Ok(Self {
            cluster_id,
            local_peer_id,
            local_addr,
            etcd_client,
            lease_id: None,
            lease_keeper: None,
            watch_stream: None,
            swarm_tx,
            shutdown_rx: Some(shutdown_rx),
            internal_shutdown_tx,
            internal_shutdown_rx: Some(internal_shutdown_rx),
            readiness,
            state: DiscoveryState::Initialization,
        })
    }

    /// Get the etcd key for a peer
    fn peer_key(&self, peer_id: &PeerId) -> String {
        format!(
            "/kameo/v1/clusters/{}/peers/{}",
            self.cluster_id, peer_id
        )
    }

    /// Get the etcd prefix for all peers in the cluster
    fn peers_prefix(&self) -> String {
        format!("/kameo/v1/clusters/{}/peers/", self.cluster_id)
    }

    /// Phase 1: Verify etcd connection is ready.
    pub async fn initialize(&mut self) -> Result<()> {
        info!("Discovery: Phase 1 - Initialization");
        self.state = DiscoveryState::Initialization;

        match self.etcd_client.status().await {
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
    pub async fn register(&mut self) -> Result<()> {
        info!("Discovery: Phase 2 - Registration");
        self.state = DiscoveryState::Registration;

        let lease_resp = self.etcd_client.lease_grant(LEASE_TTL, None).await?;
        let lease_id = lease_resp.id();
        self.lease_id = Some(lease_id);
        debug!(lease_id = lease_id, ttl = LEASE_TTL, "Lease granted");

        let (keeper, stream) = self.etcd_client.lease_keep_alive(lease_id).await?;
        self.lease_keeper = Some((keeper, stream));
        self.spawn_keepalive_task(lease_id);

        let key = self.peer_key(&self.local_peer_id);
        let metadata = PeerMetadata {
            multiaddr: self.local_addr.to_string(),
            meta: HashMap::new(),
            shard: 0,
        };
        let value = serde_json::to_string(&metadata)?;

        let put_options = PutOptions::new().with_lease(lease_id);
        self.etcd_client
            .put(key.clone(), value, Some(put_options))
            .await?;

        debug!(key = %key, "Peer registered in etcd");
        Ok(())
    }

    /// Phase 3: Fetch all peers from etcd and inject into Kademlia routing table.
    pub async fn converge(&mut self) -> Result<()> {
        info!("Discovery: Phase 3 - Convergence");
        self.state = DiscoveryState::Convergence;

        let prefix = self.peers_prefix();
        let get_options = GetOptions::new().with_prefix();
        let resp = self.etcd_client.get(prefix, Some(get_options)).await?;

        let mut peer_count = 0;
        for kv in resp.kvs() {
            let key = kv.key_str()?;
            let value = kv.value_str()?;

            if let Some(peer_id_str) = key.strip_prefix(&self.peers_prefix()) {
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

        self.readiness
            .store(true, std::sync::atomic::Ordering::Release);
        info!("Node ready to accept traffic");

        Ok(())
    }

    /// Phase 4: Open watch stream on peers prefix for real-time updates.
    pub async fn enter_maintenance(&mut self) -> Result<()> {
        info!("Discovery: Phase 4 - Maintenance");
        self.state = DiscoveryState::Maintenance;

        let prefix = self.peers_prefix();
        let watch_options = WatchOptions::new().with_prefix();
        let (_watcher, stream) = self.etcd_client.watch(prefix, Some(watch_options)).await?;
        self.watch_stream = Some(stream);

        debug!("Watch stream established");
        Ok(())
    }

    /// Process a single watch event from etcd.
    /// Returns immediately after processing one message to allow shutdown signal handling.
    pub async fn process_next_watch_event(&mut self) -> Result<()> {
        let peers_prefix = self.peers_prefix();
        let local_peer_id = self.local_peer_id;
        let swarm_tx = self.swarm_tx.clone();

        let stream = self
            .watch_stream
            .as_mut()
            .ok_or_else(|| anyhow!("Watch stream not initialized"))?;

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

                                    if let Ok(metadata) = serde_json::from_str::<PeerMetadata>(value) {
                                        if let Ok(addr) = metadata.multiaddr.parse::<Multiaddr>() {
                                            debug!(
                                                peer_id_short = &peer_id.to_base58()[46..],
                                                addr = %addr,
                                                "Watch: peer joined"
                                            );
                                            let _ = swarm_tx.send(DiscoveryCommand::AddPeer { peer_id, addr });
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
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Spawn background task to send periodic keep-alive requests for the lease.
    /// Signals internal shutdown if keep-alive fails (node becomes invisible to cluster).
    fn spawn_keepalive_task(&mut self, lease_id: i64) {
        let (mut keeper, mut keeper_stream) = match self.lease_keeper.take() {
            Some((keeper, stream)) => (keeper, stream),
            None => {
                error!("No lease keeper available");
                return;
            }
        };

        let shutdown_tx = self.internal_shutdown_tx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(KEEPALIVE_INTERVAL);
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
        });
    }

    /// Graceful shutdown: revoke lease, wait for DELETE propagation, signal swarm.
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Discovery: initiating graceful shutdown");
        self.state = DiscoveryState::Terminating;

        if let Some(lease_id) = self.lease_id {
            debug!(lease_id = lease_id, "Revoking lease");

            match self.etcd_client.lease_revoke(lease_id).await {
                Ok(_) => {
                    debug!("Lease revoked");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to revoke lease");
                }
            }

            // Wait for DELETE event to propagate to other peers
            debug!("Waiting 5s for DELETE propagation");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        self.swarm_tx.send(DiscoveryCommand::Shutdown)?;
        info!("Discovery: shutdown complete");
        Ok(())
    }

    /// Run the complete discovery protocol state machine.
    pub async fn run(mut self) -> Result<()> {
        self.initialize().await?;
        self.register().await?;
        self.converge().await?;
        self.enter_maintenance().await?;

        let mut shutdown_rx = self.shutdown_rx.take()
            .ok_or_else(|| anyhow!("Shutdown receiver already consumed"))?;

        let mut internal_shutdown_rx = self.internal_shutdown_rx.take()
            .ok_or_else(|| anyhow!("Internal shutdown receiver already consumed"))?;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Discovery: received external shutdown signal");
                    self.shutdown().await?;
                    return Ok(());
                }

                _ = internal_shutdown_rx.recv() => {
                    error!("Discovery: keep-alive failure triggered shutdown");
                    self.shutdown().await?;
                    return Err(anyhow!("Keep-alive failure - node invisible to cluster"));
                }

                result = self.process_next_watch_event() => {
                    if let Err(e) = result {
                        warn!(error = %e, "Watch event processing failed, reconnecting");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        if let Err(e) = self.enter_maintenance().await {
                            error!(error = %e, "Failed to re-establish watch stream");
                            return Err(e);
                        }
                    }
                }
            }
        }
    }
}

/// Wrapper to run the discovery controller with graceful shutdown handling
pub async fn run_discovery_controller(
    cluster_id: String,
    local_peer_id: PeerId,
    local_addr: Multiaddr,
    etcd_endpoints: Vec<String>,
    swarm_tx: mpsc::UnboundedSender<DiscoveryCommand>,
    shutdown_rx: mpsc::UnboundedReceiver<()>,
    readiness: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let controller = DiscoveryController::new(
        cluster_id,
        local_peer_id,
        local_addr,
        etcd_endpoints,
        swarm_tx,
        shutdown_rx,
        readiness,
    )
    .await?;

    // Run the controller
    controller.run().await
}
