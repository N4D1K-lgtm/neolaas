//! Swarm Event Loop
//!
//! Handles all libp2p swarm events including connections, mDNS discovery,
//! identify protocol, and Kademlia DHT events.

use super::super::behaviour::{NeolaasNetworkBehaviour, NeolaasNetworkBehaviourEvent};
use super::super::config::NetworkConfig;
use super::super::discovery::DiscoveryCommand;
use super::super::types::{NetworkState, PeerInfo};
use futures::StreamExt;
use libp2p::{identify, mdns, swarm::SwarmEvent, Swarm};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, trace};

/// Spawns the swarm event loop that handles all swarm events and discovery commands
pub fn spawn_event_loop(
    swarm: Arc<Mutex<Swarm<NeolaasNetworkBehaviour>>>,
    network_state: Arc<NetworkState>,
    mut discovery_rx: mpsc::UnboundedReceiver<DiscoveryCommand>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                event = async {
                    let mut swarm = swarm.lock().await;
                    swarm.select_next_some().await
                } => {
                    handle_swarm_event(event, &swarm, &network_state).await;
                }

                Some(cmd) = discovery_rx.recv() => {
                    handle_discovery_command(cmd, &swarm, &network_state).await;
                }
            }
        }
    })
}

/// Handle a single swarm event
async fn handle_swarm_event(
    event: SwarmEvent<NeolaasNetworkBehaviourEvent>,
    swarm: &Arc<Mutex<Swarm<NeolaasNetworkBehaviour>>>,
    network_state: &NetworkState,
) {
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
            network_state.mark_connected(&peer_id).await;
        }
        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
            debug!(
                peer_id_short = &peer_id.to_base58()[46..],
                cause = ?cause,
                "Peer disconnected"
            );
            network_state.mark_disconnected(&peer_id).await;
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Mdns(mdns::Event::Discovered(
            peers,
        ))) => {
            for (peer_id, multiaddr) in peers {
                debug!(
                    peer_id_short = &peer_id.to_base58()[46..],
                    addr = %multiaddr,
                    "mDNS: discovered peer"
                );

                let mut swarm = swarm.lock().await;
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
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Mdns(mdns::Event::Expired(peers))) => {
            for (peer_id, multiaddr) in peers {
                trace!(
                    peer_id_short = &peer_id.to_base58()[46..],
                    addr = %multiaddr,
                    "mDNS: peer expired"
                );
                let mut swarm = swarm.lock().await;
                swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
            }
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Identify(
            identify::Event::Received { peer_id, info, .. },
        )) => {
            trace!(
                peer_id_short = &peer_id.to_base58()[46..],
                protocol_version = %info.protocol_version,
                listen_addrs = info.listen_addrs.len(),
                "Identify: received peer info"
            );
            let mut swarm = swarm.lock().await;
            for addr in info.listen_addrs {
                swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
            }
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Identify(identify::Event::Sent {
            peer_id,
            ..
        })) => {
            trace!(
                peer_id_short = &peer_id.to_base58()[46..],
                "Identify: sent to peer"
            );
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Identify(
            identify::Event::Pushed { peer_id, .. },
        )) => {
            trace!(
                peer_id_short = &peer_id.to_base58()[46..],
                "Identify: pushed update"
            );
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Identify(
            identify::Event::Error { peer_id, error, .. },
        )) => {
            trace!(
                peer_id_short = &peer_id.to_base58()[46..],
                error = %error,
                "Identify: protocol error"
            );
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Kademlia(event)) => {
            trace!(event = ?event, "Kademlia: DHT event");
        }
        SwarmEvent::Behaviour(NeolaasNetworkBehaviourEvent::Kameo(_event)) => {
            // Kameo events are handled internally by the remote actor system
        }
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

/// Handle discovery commands from the etcd discovery controller
async fn handle_discovery_command(
    cmd: DiscoveryCommand,
    swarm: &Arc<Mutex<Swarm<NeolaasNetworkBehaviour>>>,
    network_state: &NetworkState,
) {
    match cmd {
        DiscoveryCommand::AddPeer { peer_id, addr } => {
            debug!(
                peer_id_short = &peer_id.to_base58()[46..],
                addr = %addr,
                "etcd: adding peer to routing table"
            );

            let mut swarm = swarm.lock().await;
            swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr.clone());

            // Extract IP and port from multiaddr
            let peer_info = PeerInfo {
                pod_name: format!("peer-{}", peer_id),
                address: match addr.iter().find_map(|p| {
                    if let libp2p::multiaddr::Protocol::Ip4(ip) = p {
                        Some(std::net::IpAddr::V4(ip))
                    } else {
                        None
                    }
                }) {
                    Some(ip) => ip,
                    None => return,
                },
                port: match addr.iter().find_map(|p| {
                    if let libp2p::multiaddr::Protocol::Tcp(port) = p {
                        Some(port)
                    } else {
                        None
                    }
                }) {
                    Some(port) => port,
                    None => return,
                },
            };
            network_state.add_peer(peer_id, peer_info).await;

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

            let mut swarm = swarm.lock().await;
            swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
            network_state.remove_peer(&peer_id).await;
        }
        DiscoveryCommand::Shutdown => {
            info!("Received shutdown command from DiscoveryController");
        }
    }
}

/// Spawns a background task to periodically clean up stale failed peers
pub fn spawn_cleanup_loop(
    network_state: Arc<NetworkState>,
    config: NetworkConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(config.cleanup_interval);
        loop {
            interval.tick().await;
            network_state.cleanup_stale_peers(config.stale_peer_max_age).await;
        }
    })
}
