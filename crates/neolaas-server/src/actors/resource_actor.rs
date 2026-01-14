//! Resource Locking Utilities
//!
//! Provides distributed locking via etcd leases for resource coordination.
//! Currently unused but available for future resource management features.

use etcd_client::Client;

/// Utilities for distributed resource locking via etcd.
#[allow(dead_code)]
pub struct ResourceUtils;

#[allow(dead_code)]
impl ResourceUtils {
    /// Acquire a distributed lock on a resource using an etcd lease.
    /// The lock is automatically released if the holder crashes (lease expiration).
    pub async fn acquire_lock(
        client: &mut Client,
        resource_type: &str,
        resource_id: &str,
        node_id: &str,
        ttl_seconds: i64,
    ) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
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

        let put_opts = etcd_client::PutOptions::new().with_lease(lease_id);
        client
            .put(lock_key.as_bytes(), lock_value.as_bytes(), Some(put_opts))
            .await?;

        tracing::debug!(
            resource_type = %resource_type,
            resource_id = %resource_id,
            lease_id = lease_id,
            "Acquired resource lock"
        );

        Ok(lease_id)
    }

    /// Release a distributed lock by revoking its lease.
    pub async fn release_lock(
        client: &mut Client,
        lease_id: i64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        client.lease_revoke(lease_id).await?;
        tracing::debug!(lease_id = lease_id, "Released resource lock");
        Ok(())
    }
}
