//! Health Monitoring Message Types

use kameo::Reply;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

/// Ping message sent from one peer to another
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PingMessage {
    pub from_node: String,
    pub from_peer: PeerId,
    pub sequence: u64,
    pub timestamp: i64,
}

/// Response to a ping message
#[derive(Serialize, Deserialize, Debug, Clone, Reply)]
pub struct PongReply {
    pub from_node: String,
    pub received_sequence: u64,
}

/// Broadcast message to all peers
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BroadcastMessage {
    pub from_node: String,
    pub from_peer: PeerId,
    pub content: String,
    pub timestamp: i64,
}

/// Acknowledgment of broadcast receipt
#[derive(Serialize, Deserialize, Debug, Clone, Reply)]
pub struct AckReply {
    pub from_node: String,
    pub success: bool,
}

/// Request for actor statistics
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetStatsMessage;

/// Statistics response
#[derive(Serialize, Deserialize, Debug, Clone, Reply)]
pub struct StatsReply {
    pub node_id: String,
    pub pings_received: u64,
    pub pongs_sent: u64,
}
