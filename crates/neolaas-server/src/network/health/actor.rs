//! Health Monitoring Actor
//!
//! Actor that handles ping/pong exchanges and broadcast messages for health monitoring.

use super::messages::{
    AckReply, BroadcastMessage, GetStatsMessage, PingMessage, PongReply, StatsReply,
};
use kameo::{
    message::{Context, Message},
    prelude::*,
    Actor,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::trace;

/// Actor that handles ping/pong health checks and broadcast messages.
#[derive(Actor, RemoteActor)]
pub struct HealthActor {
    /// Identifier for this node (used in responses)
    pub node_id: String,
    pub ping_count: Arc<RwLock<u64>>,
    pub pong_count: Arc<RwLock<u64>>,
}

impl HealthActor {
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

#[remote_message]
impl Message<PingMessage> for HealthActor {
    type Reply = PongReply;

    async fn handle(
        &mut self,
        msg: PingMessage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        *self.ping_count.write().await += 1;

        trace!(
            from_node = %msg.from_node,
            sequence = msg.sequence,
            "Received ping"
        );

        let from_node = self.node_id.clone();
        *self.pong_count.write().await += 1;

        PongReply {
            from_node,
            received_sequence: msg.sequence,
        }
    }
}

#[remote_message]
impl Message<BroadcastMessage> for HealthActor {
    type Reply = AckReply;

    async fn handle(
        &mut self,
        msg: BroadcastMessage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        trace!(
            from_node = %msg.from_node,
            content = %msg.content,
            "Received broadcast"
        );

        AckReply {
            from_node: self.node_id.clone(),
            success: true,
        }
    }
}

#[remote_message]
impl Message<GetStatsMessage> for HealthActor {
    type Reply = StatsReply;

    async fn handle(
        &mut self,
        _msg: GetStatsMessage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let pings_received = *self.ping_count.read().await;
        let pongs_sent = *self.pong_count.read().await;

        StatsReply {
            node_id: self.node_id.clone(),
            pings_received,
            pongs_sent,
        }
    }
}

/// Deprecated alias for backward compatibility
#[deprecated(since = "0.2.0", note = "Use HealthActor instead")]
pub type PingActor = HealthActor;
