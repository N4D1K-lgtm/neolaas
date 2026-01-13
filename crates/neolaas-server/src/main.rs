//! Neolaas Server - Distributed Host Allocator

use etcd_client::Client;
use neolaas_server::api;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    info!("Starting Neolaas Server...");

    // Get configuration from environment
    let etcd_endpoints = std::env::var("ETCD_ENDPOINTS")
        .unwrap_or_else(|_| "http://127.0.0.1:2379".to_string())
        .split(',')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let node_id = std::env::var("NODE_ID").unwrap_or_else(|_| {
        format!("neolaas-node-{}", uuid::Uuid::new_v4())
    });

    info!("Node ID: {}", node_id);
    info!("Etcd endpoints: {:?}", etcd_endpoints);
    info!("Bind address: {}", bind_addr);

    // Connect to etcd
    info!("Connecting to etcd...");
    let etcd_client = Client::connect(&etcd_endpoints, None).await?;
    let etcd_client = Arc::new(RwLock::new(etcd_client));
    info!("Connected to etcd");

    // Initialize P2P network
    info!("Initializing P2P network...");
    let (peer_id, ping_actor) = neolaas_server::network::init::initialize_p2p_network(node_id.clone()).await?;
    info!("P2P network initialized with peer ID: {}", peer_id);

    // Create shared state
    let state = api::AppState {
        etcd_client: etcd_client.clone(),
        node_id: node_id.clone(),
        actors: Arc::new(RwLock::new(HashMap::new())),
        peer_id: Some(peer_id),
        ping_actor: Some(ping_actor),
    };

    // Create API router
    let app = api::create_router(state);

    // Start server
    info!("Starting API server on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
