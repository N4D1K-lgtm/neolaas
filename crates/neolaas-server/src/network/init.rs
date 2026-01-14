//! P2P Network Initialization
//!
//! Sets up the libp2p swarm with Kameo remote actors and the hybrid etcd-Kademlia
//! discovery protocol. This module handles:
//!
//! - Swarm creation with TCP/QUIC transports, noise encryption, and yamux multiplexing
//! - Event loop for handling swarm events (connections, mDNS discovery, etc.)
//! - Command processing from the DiscoveryController
//! - Periodic ping loop for peer health monitoring

use super::{behaviour::NeolaasNetworkBehaviour, DiscoveryCommand, NetworkState, PingActor};
use futures::{StreamExt, TryStreamExt};
use kameo::prelude::*;
use libp2p::{identify, mdns, noise, swarm::SwarmEvent, tcp, yamux, PeerId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

/// Initialize and start the P2P network with hybrid discovery
///
/// Returns: (PeerId, PingActor ActorRef, Shutdown sender, Readiness state)
pub async fn initialize_p2p_network(
    node_id: String,
) -> Result<
    (
        PeerId,
        ActorRef<PingActor>,
        mpsc::UnboundedSender<()>,
        Arc<std::sync::atomic::AtomicBool>,
    ),
    anyhow::Error,
> {
    debug!(node_id = %node_id, "Initializing P2P network");

    // Create libp2p swarm with TCP/QUIC transports, noise encryption, and yamux multiplexing
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            let local_peer_id = key.public().to_peer_id();
            let public_key = key.public();

            debug!(
                peer_id = %local_peer_id,
                "Created swarm identity"
            );

            NeolaasNetworkBehaviour::new(local_peer_id, public_key)
                .expect("Failed to create NetworkBehaviour")
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    // Initialize the global Kameo swarm
    swarm.behaviour().kameo.init_global();

    let local_peer_id = *swarm.local_peer_id();
    info!(
        peer_id = %local_peer_id,
        peer_id_short = &local_peer_id.to_base58()[46..],
        "P2P swarm created"
    );

    // Create network state for tracking peer connections
    let network_state = NetworkState::new(local_peer_id);
    let network_state = Arc::new(network_state);

    // Listen on configured P2P port
    let p2p_port = std::env::var("P2P_PORT")
        .unwrap_or_else(|_| "9000".to_string())
        .parse::<u16>()?;

    let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", p2p_port);
    swarm.listen_on(listen_addr.parse()?)?;
    debug!(port = p2p_port, "Configured P2P listen address");

    // Wrap swarm in Arc<Mutex> for sharing across tasks
    let swarm = Arc::new(Mutex::new(swarm));

    // Create command channel for DiscoveryController -> Swarm communication
    let (discovery_tx, mut discovery_rx) = mpsc::unbounded_channel::<DiscoveryCommand>();

    // Create shutdown channel for coordinated graceful shutdown
    let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel::<()>();

    // Create readiness state (shared between DiscoveryController and API)
    let readiness = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let readiness_for_controller = readiness.clone();

    // Get configuration for DiscoveryController
    let cluster_id = std::env::var("CLUSTER_ID").unwrap_or_else(|_| "default".to_string());
    let etcd_endpoints = std::env::var("ETCD_ENDPOINTS")
        .unwrap_or_else(|_| "http://etcd.infra.svc.cluster.local:2379".to_string())
        .split(',')
        .map(String::from)
        .collect();
    let pod_ip = std::env::var("POD_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
    let local_multiaddr = format!("/ip4/{}/tcp/{}", pod_ip, p2p_port)
        .parse::<libp2p::Multiaddr>()
        .expect("Failed to parse local multiaddr");

    // Spawn DiscoveryController to manage peer registration and discovery via etcd
    debug!(
        cluster_id = %cluster_id,
        "Starting DiscoveryController"
    );

    let _discovery_handle = tokio::spawn(async move {
        if let Err(e) = super::discovery_controller::run_discovery_controller(
            cluster_id,
            local_peer_id,
            local_multiaddr,
            etcd_endpoints,
            discovery_tx,
            shutdown_rx,
            readiness_for_controller,
        )
        .await
        {
            error!(error = %e, "DiscoveryController failed");
        }
    });

    // Spawn swarm event loop with command handling
    let swarm_for_events = swarm.clone();
    let network_state_for_events = network_state.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Handle swarm events
                event = async {
                    let mut swarm = swarm_for_events.lock().await;
                    swarm.select_next_some().await
                } => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!(address = %address, "P2P network listening");
                        }
                        SwarmEvent::ConnectionEstablished {
                            peer_id, endpoint, ..
                        } => {
                            info!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                remote_addr = %endpoint.get_remote_address(),
                                "Peer connected"
                            );
                            network_state_for_events.mark_connected(&peer_id).await;
                        }
                        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                            debug!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                cause = ?cause,
                                "Peer disconnected"
                            );
                            network_state_for_events.mark_disconnected(&peer_id).await;
                        }
                        // mDNS discovery events for local network peer detection
                        SwarmEvent::Behaviour(super::behaviour::NeolaasNetworkBehaviourEvent::Mdns(
                            mdns::Event::Discovered(peers),
                        )) => {
                            for (peer_id, multiaddr) in peers {
                                debug!(
                                    peer_id_short = &peer_id.to_base58()[46..],
                                    addr = %multiaddr,
                                    "mDNS: discovered peer"
                                );

                                let mut swarm = swarm_for_events.lock().await;
                                swarm
                                    .behaviour_mut()
                                    .kademlia
                                    .add_address(&peer_id, multiaddr.clone());

                                if let Err(e) = swarm.dial(multiaddr.clone()) {
                                    trace!(
                                        peer_id_short = &peer_id.to_base58()[46..],
                                        error = %e,
                                        "mDNS: dial failed"
                                    );
                                }
                            }
                        }
                        SwarmEvent::Behaviour(super::behaviour::NeolaasNetworkBehaviourEvent::Mdns(
                            mdns::Event::Expired(peers),
                        )) => {
                            for (peer_id, multiaddr) in peers {
                                trace!(
                                    peer_id_short = &peer_id.to_base58()[46..],
                                    addr = %multiaddr,
                                    "mDNS: peer expired"
                                );
                                let mut swarm = swarm_for_events.lock().await;
                                swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                            }
                        }
                        // Identify protocol events for peer info exchange
                        SwarmEvent::Behaviour(
                            super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                                identify::Event::Received { peer_id, info, .. },
                            ),
                        ) => {
                            trace!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                protocol_version = %info.protocol_version,
                                listen_addrs = info.listen_addrs.len(),
                                "Identify: received peer info"
                            );
                            let mut swarm = swarm_for_events.lock().await;
                            for addr in info.listen_addrs {
                                swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                            }
                        }
                        SwarmEvent::Behaviour(
                            super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                                identify::Event::Sent { peer_id, .. },
                            ),
                        ) => {
                            trace!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                "Identify: sent to peer"
                            );
                        }
                        SwarmEvent::Behaviour(
                            super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                                identify::Event::Pushed { peer_id, .. },
                            ),
                        ) => {
                            trace!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                "Identify: pushed update"
                            );
                        }
                        SwarmEvent::Behaviour(
                            super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                                identify::Event::Error { peer_id, error, .. },
                            ),
                        ) => {
                            trace!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                error = %error,
                                "Identify: protocol error"
                            );
                        }
                        // Kademlia DHT events
                        SwarmEvent::Behaviour(
                            super::behaviour::NeolaasNetworkBehaviourEvent::Kademlia(event),
                        ) => {
                            trace!(event = ?event, "Kademlia: DHT event");
                        }
                        // Kameo events are handled internally by the remote actor system
                        SwarmEvent::Behaviour(super::behaviour::NeolaasNetworkBehaviourEvent::Kameo(
                            _event,
                        )) => {}
                        SwarmEvent::IncomingConnection { .. } => {
                            trace!("Incoming connection attempt");
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            if let Some(peer) = peer_id {
                                trace!(
                                    peer_id_short = &peer.to_base58()[46..],
                                    error = %error,
                                    "Outgoing connection failed"
                                );
                            } else {
                                trace!(error = %error, "Outgoing connection failed to unknown peer");
                            }
                        }
                        SwarmEvent::IncomingConnectionError { error, .. } => {
                            trace!(error = %error, "Incoming connection failed");
                        }
                        _ => {}
                    }
                }

                // Commands from DiscoveryController for routing table updates
                Some(cmd) = discovery_rx.recv() => {
                    match cmd {
                        DiscoveryCommand::AddPeer { peer_id, addr } => {
                            debug!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                addr = %addr,
                                "etcd: adding peer to routing table"
                            );

                            let mut swarm = swarm_for_events.lock().await;
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());

                            // Extract IP and port from multiaddr for network state tracking
                            let peer_info = super::PeerInfo {
                                pod_name: format!("peer-{}", peer_id),
                                address: match addr.iter().find_map(|p| {
                                    if let libp2p::multiaddr::Protocol::Ip4(ip) = p {
                                        Some(std::net::IpAddr::V4(ip))
                                    } else {
                                        None
                                    }
                                }) {
                                    Some(ip) => ip,
                                    None => continue,
                                },
                                port: match addr.iter().find_map(|p| {
                                    if let libp2p::multiaddr::Protocol::Tcp(port) = p {
                                        Some(port)
                                    } else {
                                        None
                                    }
                                }) {
                                    Some(port) => port,
                                    None => continue,
                                },
                            };
                            network_state_for_events.add_peer(peer_id, peer_info).await;

                            if let Err(e) = swarm.dial(addr.clone()) {
                                trace!(
                                    peer_id_short = &peer_id.to_base58()[46..],
                                    error = %e,
                                    "Dial failed, Kademlia will retry"
                                );
                            }
                        }
                        DiscoveryCommand::RemovePeer { peer_id } => {
                            debug!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                "etcd: removing peer from routing table"
                            );

                            let mut swarm = swarm_for_events.lock().await;
                            // Explicit remove_peer ensures the peer is fully purged from Kademlia
                            swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                            network_state_for_events.remove_peer(&peer_id).await;
                        }
                        DiscoveryCommand::Shutdown => {
                            info!("Received shutdown command from DiscoveryController");
                        }
                    }
                }
            }
        }
    });

    // Periodic cleanup of peers that have been in failed state too long (every 60s, 5min max age)
    let network_state_for_cleanup = network_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            network_state_for_cleanup
                .cleanup_stale_peers(Duration::from_secs(300))
                .await;
        }
    });

    // Brief delay to allow swarm and discovery controller to initialize
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Register PingActor for health monitoring via remote actor messaging
    let ping_actor = PingActor::new(node_id.clone());
    let actor_ref = PingActor::spawn(ping_actor);
    actor_ref.register("ping").await?;
    debug!("Registered PingActor");

    let actor_ref_clone = actor_ref.clone();

    // Periodic ping loop for peer health monitoring (every 5s)
    let local_peer_id_for_ping = local_peer_id;
    let node_id_for_ping = node_id.clone();
    let network_state_for_ping = network_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut sequence = 0u64;

        // Brief initial delay for peer discovery to populate routing table
        tokio::time::sleep(Duration::from_secs(2)).await;

        loop {
            interval.tick().await;
            sequence += 1;

            let ping_actors = RemoteActorRef::<PingActor>::lookup_all("ping");

            let mut sent_count = 0;
            let mut skipped_count = 0;
            futures::pin_mut!(ping_actors);
            while let Ok(Some(remote_ping)) = ping_actors.try_next().await {
                if remote_ping.id().peer_id() == Some(&local_peer_id_for_ping) {
                    continue;
                }

                let peer_id = match remote_ping.id().peer_id() {
                    Some(id) => *id,
                    None => continue,
                };

                // Skip peers no longer in network state (removed by etcd DELETE event)
                if !network_state_for_ping.has_peer(&peer_id).await {
                    skipped_count += 1;
                    continue;
                }

                // Apply exponential backoff for peers with repeated failures
                if network_state_for_ping.should_skip_ping(&peer_id).await {
                    skipped_count += 1;
                    continue;
                }

                let ping_msg = crate::network::test_actor::PingMessage {
                    from_node: node_id_for_ping.clone(),
                    from_peer: local_peer_id_for_ping,
                    sequence,
                    timestamp: chrono::Utc::now().timestamp(),
                };

                match remote_ping.ask(&ping_msg).await {
                    Ok(pong) => {
                        trace!(
                            remote_node = %pong.from_node,
                            sequence = sequence,
                            "Ping/pong successful"
                        );
                        sent_count += 1;
                        network_state_for_ping.mark_success(&peer_id).await;
                    }
                    Err(e) => {
                        // Peer may have been removed between check and send
                        if !network_state_for_ping.has_peer(&peer_id).await {
                            trace!(
                                peer_id_short = &peer_id.to_base58()[46..],
                                "Ping failed to removed peer"
                            );
                            continue;
                        }

                        warn!(
                            peer_id_short = &peer_id.to_base58()[46..],
                            error = %e,
                            "Ping failed"
                        );
                        network_state_for_ping.mark_failure(&peer_id).await;
                    }
                }
            }

            if sent_count > 0 {
                trace!(
                    sequence = sequence,
                    peers_reached = sent_count,
                    peers_skipped = skipped_count,
                    "Ping round completed"
                );
            } else {
                trace!(
                    sequence = sequence,
                    peers_skipped = skipped_count,
                    "No peers available for ping"
                );
            }
        }
    });

    Ok((local_peer_id, actor_ref_clone, shutdown_tx, readiness))
}
