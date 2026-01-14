//! Periodic Ping Loop
//!
//! Sends periodic ping messages to all discovered peers for health monitoring.

use super::super::config::NetworkConfig;
use super::super::types::NetworkState;
use super::actor::HealthActor;
use super::messages::PingMessage;
use futures::TryStreamExt;
use kameo::prelude::*;
use libp2p::PeerId;
use std::sync::Arc;
use tracing::{trace, warn};

/// Spawns the periodic ping loop for peer health monitoring
pub fn spawn_ping_loop(
    local_peer_id: PeerId,
    node_id: String,
    network_state: Arc<NetworkState>,
    config: NetworkConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(config.ping_interval);
        let mut sequence = 0u64;

        // Brief initial delay for peer discovery to populate routing table
        tokio::time::sleep(config.ping_initial_delay).await;

        loop {
            interval.tick().await;
            sequence += 1;

            let ping_actors = RemoteActorRef::<HealthActor>::lookup_all("ping");

            let mut sent_count = 0;
            let mut skipped_count = 0;
            futures::pin_mut!(ping_actors);
            while let Ok(Some(remote_ping)) = ping_actors.try_next().await {
                if remote_ping.id().peer_id() == Some(&local_peer_id) {
                    continue;
                }

                let peer_id = match remote_ping.id().peer_id() {
                    Some(id) => *id,
                    None => continue,
                };

                // Skip peers no longer in network state
                if !network_state.has_peer(&peer_id).await {
                    skipped_count += 1;
                    continue;
                }

                // Apply exponential backoff for peers with repeated failures
                if network_state.should_skip_ping(&peer_id).await {
                    skipped_count += 1;
                    continue;
                }

                let ping_msg = PingMessage {
                    from_node: node_id.clone(),
                    from_peer: local_peer_id,
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
                        network_state.mark_success(&peer_id).await;
                    }
                    Err(e) => {
                        // Peer may have been removed between check and send
                        if !network_state.has_peer(&peer_id).await {
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
                        network_state.mark_failure(&peer_id).await;
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
    })
}
