//! Test actor for P2P messaging
//!
//! Provides a simple ping/pong actor for testing peer-to-peer communication

use kameo::{
    message::{Context, Message},
    prelude::*,
    Actor, Reply,
};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Simple test actor for P2P messaging
#[derive(Actor, RemoteActor)]
pub struct PingActor {
    pub node_id: String,
    pub ping_count: Arc<RwLock<u64>>,
    pub pong_count: Arc<RwLock<u64>>,
}

impl PingActor {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            ping_count: Arc::new(RwLock::new(0)),
            pong_count: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn get_stats(&self) -> (u64, u64) {
        let ping = *self.ping_count.read().await;
        let pong = *self.pong_count.read().await;
        (ping, pong)
    }
}

/// Ping message sent from one peer to another
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PingMessage {
    pub from_node: String,
    pub from_peer: PeerId,
    pub sequence: u64,
    pub timestamp: i64,
}

#[remote_message]
impl Message<PingMessage> for PingActor {
    type Reply = PongReply;

    async fn handle(
        &mut self,
        msg: PingMessage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        // Increment ping received count
        let mut ping_count = self.ping_count.write().await;
        *ping_count += 1;

        info!(
            target: "neolaas::p2p::actor",
            local_node = %self.node_id,
            remote_node = %msg.from_node,
            remote_peer = %msg.from_peer,
            sequence = msg.sequence,
            message_type = "PING",
            "Received PING message"
        );

        PongReply {
            from_node: self.node_id.clone(),
            original_sequence: msg.sequence,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

/// Pong reply sent back to the ping sender
#[derive(Serialize, Deserialize, Debug, Clone, Reply)]
pub struct PongReply {
    pub from_node: String,
    pub original_sequence: u64,
    pub timestamp: i64,
}

/// Broadcast message sent to all peers
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BroadcastMessage {
    pub from_node: String,
    pub from_peer: PeerId,
    pub content: String,
    pub timestamp: i64,
}

#[remote_message]
impl Message<BroadcastMessage> for PingActor {
    type Reply = AckReply;

    async fn handle(
        &mut self,
        msg: BroadcastMessage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        info!(
            target: "neolaas::p2p::actor",
            local_node = %self.node_id,
            remote_node = %msg.from_node,
            remote_peer = %msg.from_peer,
            content = %msg.content,
            message_type = "BROADCAST",
            "Received BROADCAST message"
        );

        AckReply {
            success: true,
            from_node: self.node_id.clone(),
        }
    }
}

/// Simple acknowledgment reply
#[derive(Serialize, Deserialize, Debug, Clone, Reply)]
pub struct AckReply {
    pub success: bool,
    pub from_node: String,
}

/// Get stats message for querying actor statistics
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetStatsMessage;

#[remote_message]
impl Message<GetStatsMessage> for PingActor {
    type Reply = StatsReply;

    async fn handle(
        &mut self,
        _msg: GetStatsMessage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let (ping, pong) = self.get_stats().await;
        StatsReply {
            node_id: self.node_id.clone(),
            pings_received: ping,
            pongs_sent: pong,
        }
    }
}

/// Statistics reply
#[derive(Serialize, Deserialize, Debug, Clone, Reply)]
pub struct StatsReply {
    pub node_id: String,
    pub pings_received: u64,
    pub pongs_sent: u64,
}
