//! Application State
//!
//! Shared state passed to all API handlers.

use crate::network::health::HealthActor;
use crate::network::sharding::ShardingCoordinator;
use etcd_client::Client;
use kameo::prelude::*;
use libp2p::PeerId;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub etcd_client: Arc<RwLock<Client>>,
    pub node_id: String,
    pub peer_id: Option<PeerId>,
    pub ping_actor: Option<ActorRef<HealthActor>>,
    pub sharding_coordinator: Option<ActorRef<ShardingCoordinator>>,
    pub readiness: Arc<std::sync::atomic::AtomicBool>,
}
