//! Peer Information Types

use std::net::IpAddr;

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
