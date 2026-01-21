//! Prometheus Metrics Endpoint
//!
//! Exposes Prometheus metrics at GET /metrics

use super::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse};

/// GET /metrics - Prometheus metrics endpoint
///
/// Returns all collected metrics in Prometheus exposition format.
#[tracing::instrument(skip(state))]
pub async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    match &state.metrics_state {
        Some(metrics) => {
            let output = metrics.prometheus_handle.render();
            (
                StatusCode::OK,
                [("content-type", "text/plain; charset=utf-8")],
                output,
            )
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            [("content-type", "text/plain; charset=utf-8")],
            "Metrics not initialized".to_string(),
        ),
    }
}
