//! Host Management Endpoints
//!
//! REST API handlers for host actor creation, provisioning, and status.

use super::state::AppState;
use crate::actors::host_actor::{GetHostStatus, HostActor, HostStatus, ProvisionHost};
use crate::models::{HostAllocation, ProvisionState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use kameo::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request to create a new host actor
#[derive(Debug, Deserialize)]
pub struct CreateHostRequest {
    pub host_id: String,
    pub booking_id: Uuid,
    pub owner: String,
    pub duration_hours: u32,
}

/// Response for host creation
#[derive(Debug, Serialize)]
pub struct CreateHostResponse {
    pub host_id: String,
    pub allocation_id: Uuid,
    pub message: String,
}

/// Create a new host actor
pub async fn create_host_actor(
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
pub struct ProvisionRequest {
    pub image: String,
    pub config: Option<serde_json::Value>,
}

/// Provision a host with an image
pub async fn provision_host(
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

/// Get the status of a host
pub async fn get_host_status(
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
