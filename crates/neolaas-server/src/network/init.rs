//! P2P Network Initialization
//!
//! Sets up the libp2p swarm with Kameo remote actors and the hybrid etcd-Kademlia
//! discovery protocol. This module handles:
//!
//! - Swarm creation with TCP/QUIC transports, noise encryption, and yamux multiplexing
//! - Event loop for handling swarm events (connections, mDNS discovery, etc.)
//! - Command processing from [`DiscoveryController`]
//! - Periodic ping loop for peer health monitoring

use super::{discovery::DiscoveryCommand, health::HealthActor, NetworkState};
use kameo::prelude::*;
use libp2p::PeerId;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

/// Initialize the P2P network
///
/// Returns: (PeerId, HealthActor ActorRef, Shutdown sender, Readiness state)
pub async fn initialize_p2p_network(
    node_id: String,
) -> Result<
    (
        PeerId,
        ActorRef<HealthActor>,
        mpsc::UnboundedSender<()>,
        Arc<std::sync::atomic::AtomicBool>,
    ),
    anyhow::Error,
> {
    debug!(node_id = %node_id, "Initializing P2P network");

    // Load configuration from environment
    let config = super::config::NetworkConfig::from_env();

    // Build swarm using the swarm module
    let (swarm, local_peer_id) = super::swarm::build_swarm(&config).await?;

    info!(
        peer_id = %local_peer_id,
        peer_id_short = &local_peer_id.to_base58()[46..],
        "P2P swarm created"
    );

    let network_state = NetworkState::new(local_peer_id, config.clone());
    let network_state = Arc::new(network_state);

    // Wrap swarm in Arc<Mutex> for sharing across tasks
    let swarm = Arc::new(Mutex::new(swarm));

    // Create channels for communication between DiscoveryController and the swarm
    let (discovery_tx, discovery_rx) = mpsc::unbounded_channel::<DiscoveryCommand>();
    let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel::<()>();

    let readiness = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let readiness_for_controller = readiness.clone();

    let local_multiaddr = format!("/ip4/{}/tcp/{}", config.pod_ip, config.p2p_port)
        .parse::<libp2p::Multiaddr>()
        .expect("Failed to parse local multiaddr");

    debug!(
        cluster_id = %config.cluster_id,
        "Starting DiscoveryController"
    );

    let config_for_discovery = config.clone();
    let _discovery_handle = tokio::spawn(async move {
        if let Err(e) = super::discovery::run_discovery_controller(
            config_for_discovery.cluster_id.clone(),
            local_peer_id,
            local_multiaddr,
            config_for_discovery.etcd_endpoints.clone(),
            discovery_tx,
            shutdown_rx,
            readiness_for_controller,
            config_for_discovery,
        )
        .await
        {
            error!(error = %e, "DiscoveryController failed");
        }
    });

    // Spawn swarm event loop using the swarm module
    super::swarm::spawn_event_loop(swarm.clone(), network_state.clone(), discovery_rx);

    // Spawn periodic cleanup loop using the swarm module
    super::swarm::spawn_cleanup_loop(network_state.clone(), config.clone());

    // brief delay to allow swarm and discovery controller to initialize
    tokio::time::sleep(config.initialization_delay).await;

    // Register HealthActor for health monitoring via remote actor messaging
    let health_actor = HealthActor::new(node_id.clone());
    let actor_ref = HealthActor::spawn(health_actor);
    actor_ref.register("ping").await?;
    debug!("Registered HealthActor");

    // Spawn periodic ping loop using the health module
    super::health::spawn_ping_loop(local_peer_id, node_id, network_state, config);

    Ok((local_peer_id, actor_ref, shutdown_tx, readiness))
}
