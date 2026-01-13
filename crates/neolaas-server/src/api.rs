//! API handlers

use crate::actors::host_actor::{GetHostStatus, HostActor, HostStatus, ProvisionHost};
use crate::api_p2p::{broadcast_message, get_p2p_stats};
use crate::models::{HostAllocation, ProvisionState};
use crate::network::test_actor::PingActor;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use etcd_client::Client;
use kameo::prelude::*;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub etcd_client: Arc<RwLock<Client>>,
    pub node_id: String,
    pub actors: Arc<RwLock<HashMap<String, ActorRef<HostActor>>>>,
    pub peer_id: Option<PeerId>,
    pub ping_actor: Option<ActorRef<PingActor>>,
}

/// Create API router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/hosts", post(create_host_actor))
        .route("/hosts/{host_id}/provision", post(provision_host))
        .route("/hosts/{host_id}/status", get(get_host_status))
        .route("/p2p/stats", get(get_p2p_stats))
        .route("/p2p/broadcast", post(broadcast_message))
        .with_state(state)
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}

/// Request to create a new host actor
#[derive(Debug, Deserialize)]
struct CreateHostRequest {
    host_id: String,
    booking_id: Uuid,
    owner: String,
    duration_hours: u32,
}

/// Response for host creation
#[derive(Debug, Serialize)]
struct CreateHostResponse {
    host_id: String,
    allocation_id: Uuid,
    message: String,
}

/// Create a new host actor
async fn create_host_actor(
    State(state): State<AppState>,
    Json(req): Json<CreateHostRequest>,
) -> Result<Json<CreateHostResponse>, (StatusCode, String)> {
    tracing::info!("Creating host actor for {}", req.host_id);

    // Check if actor already exists
    {
        let actors = state.actors.read().await;
        if actors.contains_key(&req.host_id) {
            return Err((
                StatusCode::CONFLICT,
                format!("Host actor for {} already exists", req.host_id),
            ));
        }
    }

    // Create allocation
    let allocation_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::hours(req.duration_hours as i64);

    let allocation = HostAllocation {
        id: allocation_id,
        host_id: req.host_id.clone(),
        booking_id: req.booking_id,
        owner: req.owner,
        allocated_at: now,
        expires_at,
        provision_state: ProvisionState::Provisioning,
        metadata: serde_json::json!({}),
    };

    // Store allocation in etcd
    {
        let mut client = state.etcd_client.write().await;
        let allocation_key = format!("/neolaas/host_allocations/{}", allocation_id);
        let allocation_value = serde_json::to_vec(&allocation).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize allocation: {}", e),
            )
        })?;

        client
            .put(allocation_key.as_bytes(), allocation_value, None)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to store allocation in etcd: {}", e),
                )
            })?;
    }

    // Create and spawn actor
    let actor = HostActor::new(
        req.host_id.clone(),
        allocation.clone(),
        state.etcd_client.clone(),
        state.node_id.clone(),
    );

    let actor_ref = HostActor::spawn(actor);

    // Store actor reference
    {
        let mut actors = state.actors.write().await;
        actors.insert(req.host_id.clone(), actor_ref);
    }

    Ok(Json(CreateHostResponse {
        host_id: req.host_id,
        allocation_id,
        message: "Host actor created successfully".to_string(),
    }))
}

/// Request to provision a host
#[derive(Debug, Deserialize)]
struct ProvisionRequest {
    image: String,
    config: Option<serde_json::Value>,
}

/// Provision a host
async fn provision_host(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
    Json(req): Json<ProvisionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tracing::info!("Provisioning host {} with image {}", host_id, req.image);

    // Get actor reference
    let actor_ref = {
        let actors = state.actors.read().await;
        actors.get(&host_id).cloned().ok_or((
            StatusCode::NOT_FOUND,
            format!("Host actor {} not found", host_id),
        ))?
    };

    // Send provision message to actor
    let result = actor_ref
        .ask(ProvisionHost {
            image: req.image.clone(),
            config: req.config.unwrap_or(serde_json::json!({})),
        })
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to send provision message: {}", e),
            )
        })?;

    Ok(Json(serde_json::json!({
        "message": result,
        "host_id": host_id,
        "image": req.image,
    })))
}

/// Get host status
async fn get_host_status(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
) -> Result<Json<HostStatus>, (StatusCode, String)> {
    tracing::info!("Getting status for host {}", host_id);

    // Get actor reference
    let actor_ref = {
        let actors = state.actors.read().await;
        actors.get(&host_id).cloned().ok_or((
            StatusCode::NOT_FOUND,
            format!("Host actor {} not found", host_id),
        ))?
    };

    // Send get status message to actor
    let status = actor_ref.ask(GetHostStatus).send().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get host status: {}", e),
        )
    })?;

    Ok(Json(status))
}
