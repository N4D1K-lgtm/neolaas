//! Discovery Controller
//!
//! Orchestrates the hybrid etcd-Kademlia discovery protocol state machine.

use super::etcd::{spawn_keepalive_task, EtcdOperations};
use super::phases::DiscoveryPhases;
use super::super::config::NetworkConfig;
use anyhow::{anyhow, Result};
use libp2p::{Multiaddr, PeerId};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

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
    /// Phase manager
    phases: DiscoveryPhases,
    /// Current lease ID
    lease_id: Option<i64>,
    /// Channel for receiving shutdown signal (None after extracted in run())
    shutdown_rx: Option<mpsc::UnboundedReceiver<()>>,
    /// Channel for sending internal shutdown signals (from keep-alive task)
    internal_shutdown_tx: mpsc::UnboundedSender<()>,
    /// Channel for receiving internal shutdown signals (None after extracted in run())
    internal_shutdown_rx: Option<mpsc::UnboundedReceiver<()>>,
    /// Current state
    state: DiscoveryState,
    /// Configuration
    config: NetworkConfig,
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
        config: NetworkConfig,
        sharding_tx: Option<mpsc::UnboundedSender<Vec<PeerId>>>,
    ) -> Result<Self> {
        debug!(
            cluster_id = %cluster_id,
            peer_id_short = &local_peer_id.to_base58()[46..],
            "Creating discovery controller"
        );

        // Connect to etcd with exponential backoff
        let etcd_client = EtcdOperations::connect(etcd_endpoints, &config).await?;

        let etcd_ops = EtcdOperations {
            client: etcd_client,
            cluster_id,
        };

        let phases = DiscoveryPhases::new(
            etcd_ops,
            local_peer_id,
            local_addr,
            swarm_tx,
            readiness,
            config.clone(),
            sharding_tx,
        );

        let (internal_shutdown_tx, internal_shutdown_rx) = mpsc::unbounded_channel();

        Ok(Self {
            phases,
            lease_id: None,
            shutdown_rx: Some(shutdown_rx),
            internal_shutdown_tx,
            internal_shutdown_rx: Some(internal_shutdown_rx),
            state: DiscoveryState::Initialization,
            config,
        })
    }

    /// Graceful shutdown: revoke lease, wait for DELETE propagation, signal swarm.
    async fn shutdown(&mut self) -> Result<()> {
        info!("Discovery: initiating graceful shutdown");
        self.state = DiscoveryState::Terminating;

        if let Some(lease_id) = self.lease_id {
            debug!(lease_id = lease_id, "Revoking lease");

            match self.phases.etcd_ops.client.lease_revoke(lease_id).await {
                Ok(_) => {
                    debug!("Lease revoked");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to revoke lease");
                }
            }

            // Wait for DELETE event to propagate to other peers
            debug!("Waiting for DELETE propagation");
            tokio::time::sleep(self.config.delete_propagation_wait).await;
        }

        self.phases
            .swarm_tx
            .send(DiscoveryCommand::Shutdown)?;
        info!("Discovery: shutdown complete");
        Ok(())
    }

    /// Run the complete discovery protocol state machine.
    pub async fn run(mut self) -> Result<()> {
        // Phase 1: Initialize
        self.state = DiscoveryState::Initialization;
        self.phases.initialize().await?;

        // Phase 2: Register
        self.state = DiscoveryState::Registration;
        let (lease_id, keeper, stream) = self.phases.register().await?;
        self.lease_id = Some(lease_id);

        // Spawn keep-alive task
        spawn_keepalive_task(
            keeper,
            stream,
            lease_id,
            &self.config,
            self.internal_shutdown_tx.clone(),
        );

        // Phase 3: Converge
        self.state = DiscoveryState::Convergence;
        self.phases.converge().await?;

        // Phase 4: Enter Maintenance
        self.state = DiscoveryState::Maintenance;
        let mut watch_stream = self.phases.enter_maintenance().await?;

        let mut shutdown_rx = self
            .shutdown_rx
            .take()
            .ok_or_else(|| anyhow!("Shutdown receiver already consumed"))?;

        let mut internal_shutdown_rx = self
            .internal_shutdown_rx
            .take()
            .ok_or_else(|| anyhow!("Internal shutdown receiver already consumed"))?;

        // Main event loop
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

                result = self.phases.process_watch_event(&mut watch_stream) => {
                    if let Err(e) = result {
                        warn!(error = %e, "Watch event processing failed, reconnecting");
                        tokio::time::sleep(self.config.reconnect_delay).await;
                        match self.phases.enter_maintenance().await {
                            Ok(new_stream) => {
                                watch_stream = new_stream;
                            }
                            Err(e) => {
                                error!(error = %e, "Failed to re-establish watch stream");
                                return Err(e);
                            }
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
    config: NetworkConfig,
    sharding_tx: Option<mpsc::UnboundedSender<Vec<PeerId>>>,
) -> Result<()> {
    let controller = DiscoveryController::new(
        cluster_id,
        local_peer_id,
        local_addr,
        etcd_endpoints,
        swarm_tx,
        shutdown_rx,
        readiness,
        config,
        sharding_tx,
    )
    .await?;

    // Run the controller
    controller.run().await
}
