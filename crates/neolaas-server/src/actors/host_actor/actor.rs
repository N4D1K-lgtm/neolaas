//! Host Actor
//!
//! Manages the lifecycle of a single physical host allocation, including
//! provisioning state transitions persisted to etcd.

use crate::models::{HostAllocation, ProvisionState};
use etcd_client::Client;
use kameo::{
    message::{Context, Message},
    Actor, Reply,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;
use uuid::Uuid;

/// Actor managing a single host allocation.
#[derive(Actor)]
pub struct HostActor {
    pub host_id: String,
    pub allocation: HostAllocation,
    etcd_client: Arc<RwLock<Client>>,
    #[allow(dead_code)]
    node_id: String,
}

impl HostActor {
    pub fn new(
        host_id: String,
        allocation: HostAllocation,
        etcd_client: Arc<RwLock<Client>>,
        node_id: String,
    ) -> Self {
        Self {
            host_id,
            allocation,
            etcd_client,
            node_id,
        }
    }

    async fn update_provision_state(&mut self, state: ProvisionState) -> Result<(), String> {
        let allocation_key = format!("/neolaas/host_allocations/{}", self.allocation.id);
        let mut client = self.etcd_client.write().await;

        // Read current allocation
        let resp = client
            .get(allocation_key.as_bytes(), None)
            .await
            .map_err(|e| e.to_string())?;

        if resp.kvs().is_empty() {
            return Err("Allocation not found".to_string());
        }

        let mut allocation: HostAllocation =
            serde_json::from_slice(resp.kvs()[0].value()).map_err(|e| e.to_string())?;

        allocation.provision_state = state;

        let new_value = serde_json::to_vec(&allocation).map_err(|e| e.to_string())?;
        client
            .put(allocation_key.as_bytes(), new_value, None)
            .await
            .map_err(|e| e.to_string())?;

        self.allocation.provision_state = state;

        Ok(())
    }
}

// ============================================================================
// Messages
// ============================================================================

/// Provision host with image
#[derive(Clone, Serialize, Deserialize)]
pub struct ProvisionHost {
    pub image: String,
    pub config: serde_json::Value,
}

impl Message<ProvisionHost> for HostActor {
    type Reply = Result<String, String>;

    async fn handle(
        &mut self,
        msg: ProvisionHost,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        debug!(host_id = %self.host_id, image = %msg.image, "Provisioning host");

        self.update_provision_state(ProvisionState::Provisioning)
            .await?;

        // Simulate provisioning work (placeholder for actual provisioning logic)
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        self.update_provision_state(ProvisionState::Ready).await?;

        Ok(format!(
            "Host {} provisioned with {}",
            self.host_id, msg.image
        ))
    }
}

/// Get host status
#[derive(Clone, Serialize, Deserialize)]
pub struct GetHostStatus;

#[derive(Clone, Serialize, Deserialize, Reply)]
pub struct HostStatus {
    pub host_id: String,
    pub allocation_id: Uuid,
    pub booking_id: Uuid,
    pub provision_state: ProvisionState,
    pub allocated_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl Message<GetHostStatus> for HostActor {
    type Reply = HostStatus;

    async fn handle(
        &mut self,
        _msg: GetHostStatus,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        HostStatus {
            host_id: self.host_id.clone(),
            allocation_id: self.allocation.id,
            booking_id: self.allocation.booking_id,
            provision_state: self.allocation.provision_state,
            allocated_at: self.allocation.allocated_at,
            expires_at: self.allocation.expires_at,
        }
    }
}
