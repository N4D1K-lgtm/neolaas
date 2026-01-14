//! P2P API Endpoints
//!
//! HTTP endpoints for P2P network statistics and broadcast messaging.

use super::state::AppState;
use crate::network::health::{BroadcastMessage, GetStatsMessage, HealthActor};
use axum::{extract::State, http::StatusCode, response::Json};
use futures::TryStreamExt;
use kameo::prelude::*;
use serde::{Deserialize, Serialize};

/// P2P statistics response
#[derive(Debug, Serialize)]
pub struct P2PStatsResponse {
    pub node_id: String,
    pub peer_id: String,
    pub local_stats: LocalStats,
    pub remote_peers: Vec<RemotePeerStats>,
}

#[derive(Debug, Serialize)]
pub struct LocalStats {
    pub pings_received: u64,
    pub pongs_sent: u64,
}

#[derive(Debug, Serialize)]
pub struct RemotePeerStats {
    pub peer_id: String,
    pub node_id: String,
    pub pings_received: u64,
    pub pongs_sent: u64,
    pub reachable: bool,
}

/// Get P2P network statistics
pub async fn get_p2p_stats(
    State(state): State<AppState>,
) -> Result<Json<P2PStatsResponse>, (StatusCode, String)> {
    let peer_id = state.peer_id.ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "P2P network not initialized".to_string(),
    ))?;

    let ping_actor = state.ping_actor.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Ping actor not available".to_string(),
    ))?;

    // Get local stats
    let local_stats_reply = ping_actor.ask(GetStatsMessage).send().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get local stats: {}", e),
        )
    })?;

    let local_stats = LocalStats {
        pings_received: local_stats_reply.pings_received,
        pongs_sent: local_stats_reply.pongs_sent,
    };

    // Query remote peers for their stats
    let mut remote_peers = Vec::new();
    let remote_ping_actors = RemoteActorRef::<HealthActor>::lookup_all("ping");
    futures::pin_mut!(remote_ping_actors);

    while let Ok(Some(remote_ping)) = remote_ping_actors.try_next().await {
        // Skip self
        if remote_ping.id().peer_id() == Some(&peer_id) {
            continue;
        }

        match remote_ping.ask(&GetStatsMessage).await {
            Ok(stats) => {
                remote_peers.push(RemotePeerStats {
                    peer_id: remote_ping
                        .id()
                        .peer_id()
                        .map(|p| p.to_base58())
                        .unwrap_or_else(|| "unknown".to_string()),
                    node_id: stats.node_id,
                    pings_received: stats.pings_received,
                    pongs_sent: stats.pongs_sent,
                    reachable: true,
                });
            }
            Err(e) => {
                tracing::debug!(error = %e, "Failed to get stats from remote peer");
                remote_peers.push(RemotePeerStats {
                    peer_id: remote_ping
                        .id()
                        .peer_id()
                        .map(|p| p.to_base58())
                        .unwrap_or_else(|| "unknown".to_string()),
                    node_id: "unknown".to_string(),
                    pings_received: 0,
                    pongs_sent: 0,
                    reachable: false,
                });
            }
        }
    }

    Ok(Json(P2PStatsResponse {
        node_id: state.node_id.clone(),
        peer_id: peer_id.to_base58(),
        local_stats,
        remote_peers,
    }))
}

/// Broadcast message request
#[derive(Debug, Deserialize)]
pub struct BroadcastRequest {
    pub content: String,
}

/// Broadcast message response
#[derive(Debug, Serialize)]
pub struct BroadcastResponse {
    pub message: String,
    pub peers_reached: usize,
    pub peers_failed: usize,
}

/// Broadcast a message to all peers
pub async fn broadcast_message(
    State(state): State<AppState>,
    Json(req): Json<BroadcastRequest>,
) -> Result<Json<BroadcastResponse>, (StatusCode, String)> {
    let peer_id = state.peer_id.ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "P2P network not initialized".to_string(),
    ))?;

    tracing::debug!(content = %req.content, "Broadcasting message");

    let remote_ping_actors = RemoteActorRef::<HealthActor>::lookup_all("ping");
    futures::pin_mut!(remote_ping_actors);

    let mut peers_reached = 0;
    let mut peers_failed = 0;

    while let Ok(Some(remote_ping)) = remote_ping_actors.try_next().await {
        if remote_ping.id().peer_id() == Some(&peer_id) {
            continue;
        }

        let broadcast_msg = BroadcastMessage {
            from_node: state.node_id.clone(),
            from_peer: peer_id,
            content: req.content.clone(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        match remote_ping.ask(&broadcast_msg).await {
            Ok(ack) => {
                if ack.success {
                    tracing::trace!(from_node = %ack.from_node, "Broadcast acknowledged");
                    peers_reached += 1;
                } else {
                    peers_failed += 1;
                }
            }
            Err(e) => {
                let peer_id_str = remote_ping
                    .id()
                    .peer_id()
                    .map(|p| p.to_base58())
                    .unwrap_or_else(|| "unknown".to_string());
                tracing::debug!(
                    peer_id_short = &peer_id_str[46.min(peer_id_str.len())..],
                    error = %e,
                    "Broadcast failed"
                );
                peers_failed += 1;
            }
        }
    }

    Ok(Json(BroadcastResponse {
        message: format!("Broadcast sent: '{}'", req.content),
        peers_reached,
        peers_failed,
    }))
}
