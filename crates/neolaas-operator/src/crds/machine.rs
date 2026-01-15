//! Machine CRD
//!
//! Defines the Machine custom resource for physical server inventory.
//! This represents **static configuration only** - no runtime state.

use kube::CustomResource;
use macaddr::MacAddr6;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use strum::{Display, EnumString};

/// Serde module for MAC addresses as colon-separated strings.
mod mac_address_string {
    use macaddr::MacAddr6;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(mac: &MacAddr6, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&mac.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MacAddr6, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        MacAddr6::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Machine represents a physical server in the lab inventory.
///
/// This CRD captures static configuration that rarely changes:
/// - Network interfaces and their hardwired switch port connections
/// - BMC/IPMI configuration (dedicated or in-band)
/// - Administrative controls (maintenance mode, cluster assignment)
///
/// Runtime state (power status, booking status, provisioning progress)
/// is managed separately by neolaas-server.
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "neolaas.io",
    version = "v1alpha1",
    kind = "Machine",
    namespaced,
    printcolumn = r#"{"name":"Admin State", "type":"string", "jsonPath":".spec.adminState"}"#,
    printcolumn = r#"{"name":"Cluster", "type":"string", "jsonPath":".spec.cluster"}"#,
    printcolumn = r#"{"name":"Age", "type":"date", "jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct MachineSpec {
    /// Network interfaces and their hardwired switch port connections.
    /// This represents the physical cabling - each NIC is connected to a specific switch port.
    pub interfaces: Vec<NetworkInterface>,

    /// BMC/IPMI configuration for out-of-band management.
    pub bmc: BmcConfig,

    /// Administrative state controlling machine availability.
    #[serde(default)]
    pub admin_state: AdminState,

    /// Cluster assignment for multi-cluster deployments.
    #[serde(default = "default_cluster")]
    pub cluster: String,

    /// Optional reservation - prevents booking unless by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_for: Option<String>,

    /// Arbitrary labels for filtering and organization.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

fn default_cluster() -> String {
    "default".to_string()
}

/// A physical network interface and its hardwired switch port connection.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    /// MAC address of this interface.
    #[schemars(with = "String")]
    #[serde(with = "mac_address_string")]
    pub mac: MacAddr6,

    /// The switch and port this interface is physically cabled to.
    pub switch_port: SwitchPortRef,

    /// Whether this is the PXE boot interface.
    #[serde(default)]
    pub pxe: bool,
}

/// Reference to a specific port on a switch.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchPortRef {
    /// Name of the Switch CR.
    pub switch: String,

    /// Port identifier (e.g., "Ethernet1/1", "Gi0/1").
    pub port: String,
}

/// BMC configuration for out-of-band management.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BmcConfig {
    /// BMC IP address.
    #[schemars(with = "String")]
    pub address: IpAddr,

    /// Management protocol.
    #[serde(default)]
    pub protocol: BmcProtocol,

    /// Reference to Secret with `username` and `password` keys.
    pub credentials_secret: String,

    /// Whether BMC uses in-band management (no dedicated port).
    #[serde(default)]
    pub in_band: bool,
}

/// BMC protocol types.
#[derive(
    Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Display, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum BmcProtocol {
    #[default]
    Redfish,
    Ipmi,
    /// HPE iLO
    Ilo,
    /// Dell iDRAC
    Idrac,
}

/// Administrative state for the machine.
#[derive(
    Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Display, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum AdminState {
    /// Available for booking.
    #[default]
    Available,
    /// Under maintenance - not bookable.
    Maintenance,
    /// Reserved for specific use (see `reserved_for`).
    Reserved,
    /// Being decommissioned.
    Decommissioned,
}

impl AdminState {
    pub fn is_bookable(&self) -> bool {
        matches!(self, AdminState::Available | AdminState::Reserved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_state_bookable() {
        assert!(AdminState::Available.is_bookable());
        assert!(AdminState::Reserved.is_bookable());
        assert!(!AdminState::Maintenance.is_bookable());
        assert!(!AdminState::Decommissioned.is_bookable());
    }

    #[test]
    fn test_network_interface_deserialize() {
        let json = r#"{
            "mac": "00:de:11:64:00:01",
            "switchPort": { "switch": "tor-a1", "port": "Ethernet1/1" },
            "pxe": true
        }"#;
        let iface: NetworkInterface = serde_json::from_str(json).unwrap();
        assert_eq!(iface.mac.to_string(), "00:DE:11:64:00:01");
        assert!(iface.pxe);
    }

    #[test]
    fn test_network_interface_serialize() {
        let iface = NetworkInterface {
            mac: "00:de:11:64:00:01".parse().unwrap(),
            switch_port: SwitchPortRef {
                switch: "tor-a1".to_string(),
                port: "Ethernet1/1".to_string(),
            },
            pxe: true,
        };
        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("00:DE:11:64:00:01") || json.contains("00:de:11:64:00:01"));
    }
}
