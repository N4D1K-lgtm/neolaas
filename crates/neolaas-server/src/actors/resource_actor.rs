//! Resource actor utilities (simplified for now)
//!
//! This module will contain shared utilities for resource actors

use etcd_client::Client;

/// Shared utilities for resource actors
pub struct ResourceUtils;

impl ResourceUtils {
    /// Acquire etcd lock with lease
    pub async fn acquire_lock(
        client: &mut Client,
        resource_type: &str,
        resource_id: &str,
        node_id: &str,
        ttl_seconds: i64,
    ) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
        // Create lease
        let lease = client.lease_grant(ttl_seconds, None).await?;
        let lease_id = lease.id();

        let lock_key = format!("/neolaas/resource_locks/{}/{}", resource_type, resource_id);
        let lock_value = serde_json::json!({
            "holder": node_id,
            "acquired_at": chrono::Utc::now(),
            "resource_type": resource_type,
            "resource_id": resource_id,
        })
        .to_string();

        // Put lock with lease
        let put_opts = etcd_client::PutOptions::new().with_lease(lease_id);
        client
            .put(lock_key.as_bytes(), lock_value.as_bytes(), Some(put_opts))
            .await?;

        tracing::info!(
            "Acquired lock for {}:{} (lease: {})",
            resource_type,
            resource_id,
            lease_id
        );

        Ok(lease_id)
    }

    /// Release etcd lock
    pub async fn release_lock(
        client: &mut Client,
        lease_id: i64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        client.lease_revoke(lease_id).await?;
        tracing::info!("Released lock (lease: {})", lease_id);
        Ok(())
    }
}
