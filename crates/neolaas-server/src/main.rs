//! Neolaas Server - Distributed Baremetal Host Allocator
//!
//! Entry point for the neolaas server. Initializes:
//! - Tracing/logging with configurable levels via RUST_LOG
//! - etcd client for distributed state
//! - P2P network for peer discovery and actor messaging
//! - HTTP API server for client requests

use neolaas_server::api;
use neolaas_server::network::create_etcd_client;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    info!("Starting Neolaas Server");

    let etcd_endpoints = std::env::var("ETCD_ENDPOINTS")
        .unwrap_or_else(|_| "http://127.0.0.1:2379".to_string())
        .split(',')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let node_id = std::env::var("NODE_ID").unwrap_or_else(|_| {
        format!("neolaas-node-{}", uuid::Uuid::new_v4())
    });

    info!(
        node_id = %node_id,
        bind_addr = %bind_addr,
        "Configuration loaded"
    );
    debug!(etcd_endpoints = ?etcd_endpoints, "etcd endpoints");

    let etcd_client = create_etcd_client(&etcd_endpoints).await?;
    let etcd_client = Arc::new(RwLock::new(etcd_client));
    debug!("Connected to etcd");

    let (peer_id, ping_actor, sharding_coordinator, shutdown_tx, readiness) =
        neolaas_server::network::init::initialize_p2p_network(node_id.clone(), etcd_client.clone()).await?;
    info!(peer_id = %peer_id, "P2P network initialized");

    let state = api::AppState {
        etcd_client: etcd_client.clone(),
        node_id: node_id.clone(),
        peer_id: Some(peer_id),
        ping_actor: Some(ping_actor),
        sharding_coordinator: Some(sharding_coordinator),
        readiness,
    };

    let app = api::create_router(state);

    info!(bind_addr = %bind_addr, "Starting API server");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    let _server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "Server error");
        }
    });

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {
            info!("Received SIGTERM, initiating shutdown");
        }
        _ = sigint.recv() => {
            info!("Received SIGINT, initiating shutdown");
        }
    }

    debug!("Signaling DiscoveryController to shutdown");
    let _ = shutdown_tx.send(());

    // Allow time for graceful shutdown (lease revocation and DELETE propagation)
    tokio::time::sleep(tokio::time::Duration::from_secs(7)).await;

    info!("Shutdown complete");
    Ok(())
}
