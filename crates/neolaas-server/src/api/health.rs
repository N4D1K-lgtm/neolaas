//! Health Check Endpoints
//!
//! Liveness and readiness probe handlers for Kubernetes.

use super::state::AppState;
use axum::{extract::State, http::StatusCode};

/// Liveness probe endpoint. Verifies etcd connection is healthy.
#[tracing::instrument(skip(state))]
pub async fn health_check(State(state): State<AppState>) -> Result<&'static str, StatusCode> {
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
#[tracing::instrument(skip(state))]
pub async fn readiness_check(State(state): State<AppState>) -> Result<&'static str, StatusCode> {
    if state.readiness.load(std::sync::atomic::Ordering::Acquire) {
        Ok("READY")
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}
