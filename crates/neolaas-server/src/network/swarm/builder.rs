//! Swarm Builder
//!
//! Handles libp2p swarm creation with TCP/QUIC transports, noise encryption,
//! and yamux multiplexing.

use super::super::behaviour::NeolaasNetworkBehaviour;
use super::super::config::NetworkConfig;
use anyhow::Result;
use libp2p::{noise, swarm::Swarm, tcp, yamux, PeerId};
use tracing::debug;

/// Creates and configures a libp2p swarm
pub async fn build_swarm(config: &NetworkConfig) -> Result<(Swarm<NeolaasNetworkBehaviour>, PeerId)> {
    let config_for_swarm = config.clone();
    let config_for_timeout = config.clone();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(move |key| {
            let local_peer_id = key.public().to_peer_id();
            let public_key = key.public();

            debug!(
                peer_id = %local_peer_id,
                "Created swarm identity"
            );

            NeolaasNetworkBehaviour::new(local_peer_id, public_key, &config_for_swarm)
                .expect("Failed to create NetworkBehaviour")
        })?
        .with_swarm_config(move |c| {
            c.with_idle_connection_timeout(config_for_timeout.connection_idle_timeout)
        })
        .build();

    // Initialize the global Kameo swarm
    swarm.behaviour().kameo.init_global();

    let local_peer_id = *swarm.local_peer_id();

    // Configure listen address
    let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", config.p2p_port);
    swarm.listen_on(listen_addr.parse()?)?;
    debug!(port = config.p2p_port, "Configured P2P listen address");

    Ok((swarm, local_peer_id))
}
