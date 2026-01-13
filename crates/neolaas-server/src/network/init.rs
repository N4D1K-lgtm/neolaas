//! P2P Network Initialization
//!
//! Sets up the libp2p swarm with Kameo remote actors and peer discovery

use super::{behaviour::NeolaasNetworkBehaviour, NetworkState, PingActor};
use futures::{StreamExt, TryStreamExt};
use kameo::prelude::*;
use libp2p::{identify, mdns, noise, swarm::SwarmEvent, tcp, yamux, PeerId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// Initialize and start the P2P network
pub async fn initialize_p2p_network(
    node_id: String,
) -> Result<(PeerId, ActorRef<PingActor>), anyhow::Error> {
    info!("Initializing P2P network for node: {}", node_id);

    // Create libp2p swarm with production-ready peer discovery
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

            info!(
                target: "neolaas::p2p::init",
                peer_id = %local_peer_id,
                "Initializing P2P network with peer ID"
            );

            NeolaasNetworkBehaviour::new(local_peer_id, public_key)
                .expect("Failed to create NetworkBehaviour")
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    // Initialize the global Kameo swarm
    swarm.behaviour().kameo.init_global();

    let local_peer_id = *swarm.local_peer_id();
    info!("Peer ID (short): {}", &local_peer_id.to_base58()[46..]);

    // Create network state for tracking peer connections
    let network_state = NetworkState::new(local_peer_id);
    let network_state = Arc::new(network_state);

    // Listen on configured P2P port
    let p2p_port = std::env::var("P2P_PORT")
        .unwrap_or_else(|_| "9000".to_string())
        .parse::<u16>()?;

    let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", p2p_port);
    swarm.listen_on(listen_addr.parse()?)?;
    info!("Listening on port {}", p2p_port);

    // Wrap swarm in Arc<Mutex> for sharing across tasks
    let swarm = Arc::new(Mutex::new(swarm));

    // Register this peer in etcd and discover other peers
    let etcd_endpoints = std::env::var("ETCD_ENDPOINTS")
        .unwrap_or_else(|_| "http://etcd.infra.svc.cluster.local:2379".to_string())
        .split(',')
        .map(String::from)
        .collect();

    let pod_ip = std::env::var("POD_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
    let p2p_port = p2p_port;
    let local_multiaddr = format!("/ip4/{}/tcp/{}", pod_ip, p2p_port)
        .parse::<libp2p::Multiaddr>()
        .expect("Failed to parse local multiaddr");

    let mut registry =
        super::PeerRegistry::new(etcd_endpoints, local_peer_id, local_multiaddr.clone()).await?;

    // Register this peer
    registry.register().await?;

    // Discover and dial existing peers
    match registry.discover_peers().await {
        Ok(peers) if !peers.is_empty() => {
            info!(
                target: "neolaas::p2p::init",
                peer_count = peers.len(),
                "Discovered peers from etcd, connecting..."
            );

            let mut swarm_lock = swarm.lock().await;
            for (peer_id, addr) in peers {
                // Add to Kademlia routing table
                swarm_lock.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());

                // Dial the peer
                if let Err(e) = swarm_lock.dial(addr.clone()) {
                    debug!(
                        target: "neolaas::p2p::init",
                        peer_id = %peer_id,
                        peer_id_short = &peer_id.to_base58()[46..],
                        error = %e,
                        "Failed to dial peer (Kademlia will retry)"
                    );
                }
            }

            // Bootstrap Kademlia now that we have some peers
            if let Err(e) = swarm_lock.behaviour_mut().kademlia.bootstrap() {
                debug!(
                    target: "neolaas::p2p::init",
                    error = ?e,
                    "Kademlia bootstrap will retry"
                );
            }
        }
        Ok(_) => {
            info!(
                target: "neolaas::p2p::init",
                "No peers in etcd yet - waiting for others to join"
            );
        }
        Err(e) => {
            warn!(
                target: "neolaas::p2p::init",
                error = %e,
                "Failed to discover peers from etcd"
            );
        }
    }

    // Spawn background task for periodic peer discovery and cleanup
    let swarm_for_discovery = swarm.clone();
    let network_state_for_cleanup = network_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.tick().await; // Skip first tick

        loop {
            interval.tick().await;

            // Discover new peers from etcd
            match registry.discover_peers().await {
                Ok(peers) if !peers.is_empty() => {
                    let mut swarm_lock = swarm_for_discovery.lock().await;
                    for (peer_id, addr) in peers {
                        // Just add to Kademlia - let it decide whether to connect
                        swarm_lock.behaviour_mut().kademlia.add_address(&peer_id, addr);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    debug!(
                        target: "neolaas::p2p::init",
                        error = %e,
                        "Failed to discover peers from etcd"
                    );
                }
            }

            // Clean up stale peer state
            network_state_for_cleanup
                .cleanup_stale_peers(Duration::from_secs(300))
                .await;
        }
    });

    // Spawn swarm event loop FIRST (before actor registration)
    let swarm_for_events = swarm.clone();
    let network_state_for_events = network_state.clone();
    tokio::spawn(async move {
        loop {
            let event = {
                let mut swarm = swarm_for_events.lock().await;
                swarm.select_next_some().await
            };

            match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    info!(
                        target: "neolaas::p2p::network",
                        address = %address,
                        "P2P network listening"
                    );
                }
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    info!(
                        target: "neolaas::p2p::connection",
                        peer_id = %peer_id,
                        peer_id_short = &peer_id.to_base58()[46..],
                        remote_addr = %endpoint.get_remote_address(),
                        "Peer connection established"
                    );

                    // Add peer to Kademlia routing table
                    let mut swarm = swarm_for_events.lock().await;
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, endpoint.get_remote_address().clone());

                    // Mark peer as connected in network state
                    network_state_for_events.mark_connected(&peer_id).await;
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    warn!(
                        target: "neolaas::p2p::connection",
                        peer_id = %peer_id,
                        peer_id_short = &peer_id.to_base58()[46..],
                        cause = ?cause,
                        "Peer connection closed"
                    );

                    // Mark peer as disconnected in network state
                    network_state_for_events.mark_disconnected(&peer_id).await;
                }
                // Handle mDNS discovery events
                SwarmEvent::Behaviour(super::behaviour::NeolaasNetworkBehaviourEvent::Mdns(
                    mdns::Event::Discovered(peers),
                )) => {
                    for (peer_id, multiaddr) in peers {
                        info!(
                            target: "neolaas::p2p::discovery",
                            peer_id = %peer_id,
                            peer_id_short = &peer_id.to_base58()[46..],
                            addr = %multiaddr,
                            protocol = "mDNS",
                            "Discovered peer via mDNS"
                        );

                        // Add peer address to swarm and Kademlia
                        let mut swarm = swarm_for_events.lock().await;
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, multiaddr.clone());

                        // Dial the discovered peer
                        if let Err(e) = swarm.dial(multiaddr.clone()) {
                            debug!(
                                target: "neolaas::p2p::discovery",
                                peer_id = %peer_id,
                                error = %e,
                                "Failed to dial mDNS-discovered peer"
                            );
                        }
                    }
                }
                SwarmEvent::Behaviour(super::behaviour::NeolaasNetworkBehaviourEvent::Mdns(
                    mdns::Event::Expired(peers),
                )) => {
                    for (peer_id, multiaddr) in peers {
                        info!(
                            target: "neolaas::p2p::discovery",
                            peer_id = %peer_id,
                            peer_id_short = &peer_id.to_base58()[46..],
                            addr = %multiaddr,
                            protocol = "mDNS",
                            "Peer expired from mDNS cache"
                        );

                        // Remove peer from Kademlia
                        let mut swarm = swarm_for_events.lock().await;
                        swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                    }
                }
                // Handle Identify events
                SwarmEvent::Behaviour(
                    super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                        identify::Event::Received { peer_id, info, .. },
                    ),
                ) => {
                    debug!(
                        target: "neolaas::p2p::identify",
                        peer_id = %peer_id,
                        protocol_version = %info.protocol_version,
                        agent_version = %info.agent_version,
                        listen_addrs = info.listen_addrs.len(),
                        "Received peer identification"
                    );

                    // Add all advertised addresses to Kademlia
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
                    debug!(
                        target: "neolaas::p2p::identify",
                        peer_id = %peer_id,
                        "Sent identification to peer"
                    );
                }
                SwarmEvent::Behaviour(
                    super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                        identify::Event::Pushed { peer_id, .. },
                    ),
                ) => {
                    debug!(
                        target: "neolaas::p2p::identify",
                        peer_id = %peer_id,
                        "Pushed identification update to peer"
                    );
                }
                SwarmEvent::Behaviour(
                    super::behaviour::NeolaasNetworkBehaviourEvent::Identify(
                        identify::Event::Error { peer_id, error, .. },
                    ),
                ) => {
                    debug!(
                        target: "neolaas::p2p::identify",
                        peer_id = %peer_id,
                        error = %error,
                        "Identify protocol error"
                    );
                }
                // Handle Kademlia events
                SwarmEvent::Behaviour(
                    super::behaviour::NeolaasNetworkBehaviourEvent::Kademlia(event),
                ) => {
                    debug!(
                        target: "neolaas::p2p::kademlia",
                        event = ?event,
                        "Kademlia DHT event"
                    );
                }
                // Handle Kameo events (actor messaging)
                SwarmEvent::Behaviour(super::behaviour::NeolaasNetworkBehaviourEvent::Kameo(
                    _event,
                )) => {
                    // Kameo events are handled internally
                }
                SwarmEvent::IncomingConnection { .. } => {
                    debug!(
                        target: "neolaas::p2p::connection",
                        "Incoming connection attempt"
                    );
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    if let Some(peer) = peer_id {
                        debug!(
                            target: "neolaas::p2p::connection",
                            peer_id = %peer,
                            peer_id_short = &peer.to_base58()[46..],
                            error = %error,
                            "Outgoing connection failed"
                        );
                    } else {
                        debug!(
                            target: "neolaas::p2p::connection",
                            error = %error,
                            "Outgoing connection failed to unknown peer"
                        );
                    }
                }
                SwarmEvent::IncomingConnectionError { error, .. } => {
                    debug!(
                        target: "neolaas::p2p::connection",
                        error = %error,
                        "Incoming connection failed"
                    );
                }
                _ => {}
            }
        }
    });

    // Give the swarm a moment to start processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // NOW create and register the PingActor (after swarm is running)
    let ping_actor = PingActor::new(node_id.clone());
    let actor_ref = PingActor::spawn(ping_actor);
    actor_ref.register("ping").await?;
    info!("Registered PingActor as 'ping'");

    // Clone actor_ref for returning
    let actor_ref_clone = actor_ref.clone();

    // Spawn ping message sender loop
    let local_peer_id_for_ping = local_peer_id;
    let node_id_for_ping = node_id.clone();
    let network_state_for_ping = network_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut sequence = 0u64;

        // Reduced initial wait time for faster peer discovery
        tokio::time::sleep(Duration::from_secs(2)).await;

        loop {
            interval.tick().await;
            sequence += 1;

            // Look up all remote ping actors
            let ping_actors = RemoteActorRef::<PingActor>::lookup_all("ping");

            let mut sent_count = 0;
            let mut skipped_count = 0;
            futures::pin_mut!(ping_actors);
            while let Ok(Some(remote_ping)) = ping_actors.try_next().await {
                // Skip self
                if remote_ping.id().peer_id() == Some(&local_peer_id_for_ping) {
                    continue;
                }

                // Get peer_id for filtering (clone to avoid lifetime issues)
                let peer_id = match remote_ping.id().peer_id() {
                    Some(id) => *id,
                    None => continue,
                };

                // Skip peers that should be excluded based on failure count and backoff
                if network_state_for_ping.should_skip_ping(&peer_id).await {
                    skipped_count += 1;
                    continue;
                }

                // Send ping message
                let ping_msg = crate::network::test_actor::PingMessage {
                    from_node: node_id_for_ping.clone(),
                    from_peer: local_peer_id_for_ping,
                    sequence,
                    timestamp: chrono::Utc::now().timestamp(),
                };

                match remote_ping.ask(&ping_msg).await {
                    Ok(pong) => {
                        info!(
                            target: "neolaas::p2p::messaging",
                            local_node = %node_id_for_ping,
                            remote_node = %pong.from_node,
                            sequence = sequence,
                            rtt_ms = (chrono::Utc::now().timestamp() - ping_msg.timestamp) * 1000,
                            "PING/PONG exchange successful"
                        );
                        sent_count += 1;

                        // Mark success in network state
                        network_state_for_ping.mark_success(&peer_id).await;
                    }
                    Err(e) => {
                        let peer_id_str = peer_id.to_base58();
                        error!(
                            target: "neolaas::p2p::messaging",
                            peer_id_short = &peer_id_str[46..],
                            error = %e,
                            "PING message failed"
                        );

                        // Mark failure in network state
                        network_state_for_ping.mark_failure(&peer_id).await;
                    }
                }
            }

            if sent_count > 0 {
                info!(
                    target: "neolaas::p2p::messaging",
                    local_node = %node_id_for_ping,
                    sequence = sequence,
                    peers_reached = sent_count,
                    peers_skipped = skipped_count,
                    "Ping round completed"
                );
            } else {
                debug!(
                    target: "neolaas::p2p::messaging",
                    local_node = %node_id_for_ping,
                    sequence = sequence,
                    peers_skipped = skipped_count,
                    "No remote peers available for ping round"
                );
            }
        }
    });

    Ok((local_peer_id, actor_ref_clone))
}
