//! HTTP API Handlers
//!
//! REST API endpoints for host management, health checks, and P2P statistics.

use crate::actors::host_actor::{GetHostStatus, HostActor, HostStatus, ProvisionHost};
use crate::api_p2p::{broadcast_message, get_p2p_stats};
use crate::models::{HostAllocation, ProvisionState};
use crate::network::health::HealthActor;
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
    pub ping_actor: Option<ActorRef<HealthActor>>,
    pub readiness: Arc<std::sync::atomic::AtomicBool>,
}

/// Create API router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/hosts", post(create_host_actor))
        .route("/hosts/{host_id}/provision", post(provision_host))
        .route("/hosts/{host_id}/status", get(get_host_status))
        .route("/p2p/stats", get(get_p2p_stats))
        .route("/p2p/broadcast", post(broadcast_message))
        .with_state(state)
}

/// Liveness probe endpoint. Verifies etcd connection is healthy.
async fn health_check(State(state): State<AppState>) -> Result<&'static str, StatusCode> {
    let etcd_check = tokio::time::timeout(tokio::time::Duration::from_secs(2), async {
        let mut client = state.etcd_client.write().await;
        client.status().await
    })
    .await;

    match etcd_check {
        Ok(Ok(_)) => Ok("OK"),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "etcd health check failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
        Err(_) => {
            tracing::warn!("etcd health check timed out");
            Err(StatusCode::REQUEST_TIMEOUT)
        }
    }
}

/// Readiness probe endpoint. Returns OK after discovery convergence completes.
async fn readiness_check(State(state): State<AppState>) -> Result<&'static str, StatusCode> {
    if state.readiness.load(std::sync::atomic::Ordering::Acquire) {
        Ok("READY")
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
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

async fn create_host_actor(
    State(state): State<AppState>,
    Json(req): Json<CreateHostRequest>,
) -> Result<Json<CreateHostResponse>, (StatusCode, String)> {
    tracing::debug!(host_id = %req.host_id, "Creating host actor");

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

async fn provision_host(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
    Json(req): Json<ProvisionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tracing::debug!(host_id = %host_id, image = %req.image, "Provisioning host");

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

async fn get_host_status(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
) -> Result<Json<HostStatus>, (StatusCode, String)> {
    tracing::trace!(host_id = %host_id, "Getting host status");

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
