//! P2P Network Module
//!
//! Provides peer-to-peer networking capabilities using Kameo remote actors
//! and libp2p for peer discovery and communication within a Kubernetes cluster.
//!
//! This module contains:
//! - `init`: Network initialization and swarm event loop
//! - `behaviour`: libp2p NetworkBehaviour configuration
//! - `discovery_controller`: Hybrid etcd-Kademlia peer discovery state machine
//! - `test_actor`: Simple ping/pong actor for health checking

pub mod behaviour;
pub mod discovery_controller;
pub mod init;
pub mod test_actor;

pub use behaviour::NeolaasNetworkBehaviour;
pub use discovery_controller::{DiscoveryCommand, DiscoveryController, DiscoveryState};
pub use test_actor::{PingActor, PingMessage};

use libp2p::PeerId;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, trace};

/// Information about a discovered peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub pod_name: String,
    pub address: IpAddr,
    pub port: u16,
}

/// Connection status of a peer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Failed,
}

/// Tracks health and connection state for a single peer
#[derive(Debug, Clone)]
pub struct PeerConnectionState {
    pub peer_id: PeerId,
    pub peer_info: PeerInfo,
    pub status: ConnectionStatus,
    pub consecutive_failures: u32,
    pub last_seen: Instant,
    pub last_failure: Option<Instant>,
    pub connected_at: Option<Instant>,
}

impl PeerConnectionState {
    pub fn new(peer_id: PeerId, peer_info: PeerInfo) -> Self {
        Self {
            peer_id,
            peer_info,
            status: ConnectionStatus::Disconnected,
            consecutive_failures: 0,
            last_seen: Instant::now(),
            last_failure: None,
            connected_at: None,
        }
    }

    /// Check if peer should be excluded from pings based on failure count
    pub fn should_skip_ping(&self) -> bool {
        match self.status {
            ConnectionStatus::Connected => false,
            ConnectionStatus::Disconnected => {
                // Skip if we have too many failures
                if self.consecutive_failures > 3 {
                    // Use exponential backoff
                    if let Some(last_failure) = self.last_failure {
                        let backoff_duration = Duration::from_secs(2u64.pow(self.consecutive_failures.min(6)));
                        last_failure.elapsed() < backoff_duration
                    } else {
                        true
                    }
                } else {
                    false
                }
            }
            ConnectionStatus::Failed => true,
        }
    }

    pub fn mark_connected(&mut self) {
        self.status = ConnectionStatus::Connected;
        self.consecutive_failures = 0;
        self.last_seen = Instant::now();
        self.connected_at = Some(Instant::now());
        self.last_failure = None;
    }

    pub fn mark_disconnected(&mut self) {
        self.status = ConnectionStatus::Disconnected;
        self.last_seen = Instant::now();
        self.connected_at = None;
    }

    pub fn mark_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());
        self.last_seen = Instant::now();

        // If too many failures, mark as failed
        if self.consecutive_failures >= 10 {
            self.status = ConnectionStatus::Failed;
        } else {
            self.status = ConnectionStatus::Disconnected;
        }
    }

    pub fn mark_success(&mut self) {
        self.consecutive_failures = 0;
        self.last_seen = Instant::now();
        self.last_failure = None;
        if self.status == ConnectionStatus::Disconnected {
            self.status = ConnectionStatus::Connected;
        }
    }
}

/// Shared network state for tracking peers and connection status
#[derive(Clone)]
pub struct NetworkState {
    pub local_peer_id: PeerId,
    pub peers: Arc<RwLock<HashMap<PeerId, PeerConnectionState>>>,
}

impl NetworkState {
    pub fn new(local_peer_id: PeerId) -> Self {
        Self {
            local_peer_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_peer(&self, peer_id: PeerId, peer_info: PeerInfo) {
        let mut peers = self.peers.write().await;
        if let std::collections::hash_map::Entry::Vacant(e) = peers.entry(peer_id) {
            debug!(
                peer_id = %peer_id,
                peer_id_short = &peer_id.to_base58()[46..],
                address = %peer_info.address,
                "Adding peer to network state"
            );
            e.insert(PeerConnectionState::new(peer_id, peer_info));
        }
    }

    pub async fn mark_connected(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            trace!(
                peer_id = %peer_id,
                peer_id_short = &peer_id.to_base58()[46..],
                "Marking peer as connected"
            );
            peer.mark_connected();
        }
    }

    pub async fn mark_disconnected(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            trace!(
                peer_id = %peer_id,
                peer_id_short = &peer_id.to_base58()[46..],
                consecutive_failures = peer.consecutive_failures,
                "Marking peer as disconnected"
            );
            peer.mark_disconnected();
        }
    }

    pub async fn mark_failure(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.mark_failure();
            debug!(
                peer_id = %peer_id,
                peer_id_short = &peer_id.to_base58()[46..],
                consecutive_failures = peer.consecutive_failures,
                status = ?peer.status,
                "Peer communication failure recorded"
            );
        }
    }

    pub async fn mark_success(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.mark_success();
        }
    }

    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers
            .read()
            .await
            .values()
            .map(|state| state.peer_info.clone())
            .collect()
    }

    pub async fn get_connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .read()
            .await
            .iter()
            .filter(|(_, state)| state.status == ConnectionStatus::Connected)
            .map(|(peer_id, _)| *peer_id)
            .collect()
    }

    pub async fn should_skip_ping(&self, peer_id: &PeerId) -> bool {
        self.peers
            .read()
            .await
            .get(peer_id)
            .map(|state| state.should_skip_ping())
            .unwrap_or(true) // Skip if peer doesn't exist (might have been removed)
    }

    /// Check if a peer exists in the network state
    pub async fn has_peer(&self, peer_id: &PeerId) -> bool {
        self.peers.read().await.contains_key(peer_id)
    }

    pub async fn remove_peer(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if peers.remove(peer_id).is_some() {
            debug!(
                peer_id = %peer_id,
                peer_id_short = &peer_id.to_base58()[46..],
                "Removed peer from network state"
            );
        }
    }

    /// Remove peers that have been in failed state longer than max_age.
    /// Called periodically to prevent unbounded memory growth from dead peers.
    pub async fn cleanup_stale_peers(&self, max_age: Duration) {
        let mut peers = self.peers.write().await;
        let stale_peers: Vec<PeerId> = peers
            .iter()
            .filter(|(_, state)| {
                state.status == ConnectionStatus::Failed && state.last_seen.elapsed() > max_age
            })
            .map(|(peer_id, _)| *peer_id)
            .collect();

        for peer_id in stale_peers {
            trace!(
                peer_id = %peer_id,
                peer_id_short = &peer_id.to_base58()[46..],
                "Cleaning up stale failed peer"
            );
            peers.remove(&peer_id);
        }
    }
}
