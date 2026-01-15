//! Application State
//!
//! Shared state passed to all API handlers.

use crate::actors::host_actor::HostActor;
use crate::network::health::HealthActor;
use etcd_client::Client;
use kameo::prelude::*;
use libp2p::PeerId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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
