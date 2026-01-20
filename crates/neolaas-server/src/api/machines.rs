//! Machine Actor State Endpoint
//!
//! Exposes machine actor states at GET /actors/machines

use super::AppState;
use crate::network::machines::GetAllMachineStatuses;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

/// Response for GET /actors/machines
#[derive(Debug, Serialize)]
pub struct MachinesResponse {
    pub machines: Vec<MachineInfo>,
}

/// Information about a single machine
#[derive(Debug, Serialize)]
pub struct MachineInfo {
    pub machine_id: String,
    pub state: String,
    pub admin_state: String,
    pub cluster: String,
    pub current_booking: Option<String>,
    pub last_activity: Option<i64>,
}

/// GET /actors/machines - Get all machine actor states
///
/// Returns the current state of all machine actors managed by this node.
#[tracing::instrument(skip(state))]
pub async fn get_machines(State(state): State<AppState>) -> impl IntoResponse {
    let Some(ref manager_ref) = state.machine_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(MachinesResponse { machines: vec![] }),
        );
    };

    match manager_ref.ask(GetAllMachineStatuses).send().await {
        Ok(statuses) => {
            let machines: Vec<MachineInfo> = statuses
                .into_iter()
                .map(|s| MachineInfo {
                    machine_id: s.machine_id,
                    state: s.state.to_string(),
                    admin_state: s.admin_state,
                    cluster: s.cluster,
                    current_booking: s.current_booking,
                    last_activity: s.last_activity,
                })
                .collect();

            (StatusCode::OK, Json(MachinesResponse { machines }))
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to get machine statuses");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MachinesResponse { machines: vec![] }),
            )
        }
    }
}
