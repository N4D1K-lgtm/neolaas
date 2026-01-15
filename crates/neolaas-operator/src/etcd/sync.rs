//! etcd sync implementation

use anyhow::{Context, Result};
use etcd_client::Client;
use serde::Serialize;

/// Keys used in etcd for neolaas resources.
pub mod keys {
    pub const MACHINES_PREFIX: &str = "/neolaas/machines/";
    pub const SWITCHES_PREFIX: &str = "/neolaas/switches/";
    pub const VLANS_PREFIX: &str = "/neolaas/vlans/";
}

/// etcd synchronization client.
#[derive(Clone)]
pub struct EtcdSync {
    client: Client,
}

impl EtcdSync {
    pub async fn new(endpoints: &[String]) -> Result<Self> {
        let client = Client::connect(endpoints, None)
            .await
            .context("Failed to connect to etcd")?;
        Ok(Self { client })
    }

    /// Put a machine to etcd.
    pub async fn put_machine(&mut self, name: &str, spec: &impl Serialize) -> Result<()> {
        self.put(&format!("{}{}", keys::MACHINES_PREFIX, name), spec)
            .await
    }

    /// Delete a machine from etcd.
    pub async fn delete_machine(&mut self, name: &str) -> Result<()> {
        self.delete(&format!("{}{}", keys::MACHINES_PREFIX, name))
            .await
    }

    /// Put a switch to etcd.
    pub async fn put_switch(&mut self, name: &str, spec: &impl Serialize) -> Result<()> {
        self.put(&format!("{}{}", keys::SWITCHES_PREFIX, name), spec)
            .await
    }

    /// Delete a switch from etcd.
    pub async fn delete_switch(&mut self, name: &str) -> Result<()> {
        self.delete(&format!("{}{}", keys::SWITCHES_PREFIX, name))
            .await
    }

    /// Put a VLAN to etcd.
    pub async fn put_vlan(&mut self, name: &str, spec: &impl Serialize) -> Result<()> {
        self.put(&format!("{}{}", keys::VLANS_PREFIX, name), spec)
            .await
    }

    /// Delete a VLAN from etcd.
    pub async fn delete_vlan(&mut self, name: &str) -> Result<()> {
        self.delete(&format!("{}{}", keys::VLANS_PREFIX, name))
            .await
    }

    async fn put(&mut self, key: &str, value: &impl Serialize) -> Result<()> {
        let json = serde_json::to_vec(value).context("Failed to serialize to JSON")?;
        self.client
            .put(key, json, None)
            .await
            .context("Failed to put to etcd")?;
        Ok(())
    }

    async fn delete(&mut self, key: &str) -> Result<()> {
        self.client
            .delete(key, None)
            .await
            .context("Failed to delete from etcd")?;
        Ok(())
    }
}
