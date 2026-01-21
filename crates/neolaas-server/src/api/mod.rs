//! HTTP API Module
//!
//! REST API endpoints for the neolaas server.
//!
//! This module contains:
//! - `state`: Shared application state
//! - `health`: Liveness and readiness probes
//! - `p2p`: P2P network statistics and broadcast
//! - `metrics`: Prometheus metrics endpoint
//! - `machines`: Machine actor state endpoint

mod health;
// TODO: Re-enable once MachineActor is implemented in sharding module
// mod hosts;
mod machines;
mod metrics;
mod p2p;
mod state;

pub use state::AppState;

use axum::{
    routing::{get, post},
    Router,
};

/// Create the API router with all endpoints
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health checks
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check))
        // Observability
        .route("/metrics", get(metrics::get_metrics))
        // Actor state
        .route("/actors/machines", get(machines::get_machines))
        // TODO: Re-enable host routes once MachineActor is implemented
        // .route("/hosts", post(hosts::create_host_actor))
        // .route("/hosts/{host_id}/provision", post(hosts::provision_host))
        // .route("/hosts/{host_id}/status", get(hosts::get_host_status))
        // P2P endpoints
        .route("/p2p/stats", get(p2p::get_p2p_stats))
        .route("/p2p/broadcast", post(p2p::broadcast_message))
        .with_state(state)
}
