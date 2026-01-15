//! Switch CRD
//!
//! Defines the Switch custom resource for network switch inventory.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use strum::{Display, EnumString};

/// Switch represents a network switch in the lab.
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "neolaas.io",
    version = "v1alpha1",
    kind = "Switch",
    namespaced,
    printcolumn = r#"{"name":"Management IP", "type":"string", "jsonPath":".spec.managementIp"}"#,
    printcolumn = r#"{"name":"Backend", "type":"string", "jsonPath":".spec.backend"}"#,
    printcolumn = r#"{"name":"Age", "type":"date", "jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSpec {
    /// Management IP address.
    #[schemars(with = "String")]
    pub management_ip: IpAddr,

    /// Automation backend/driver.
    #[serde(default)]
    pub backend: SwitchBackend,

    /// Reference to Secret with `username` and `password` keys.
    pub credentials_secret: String,

    /// Port override (defaults based on backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Arbitrary labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// Switch automation backend.
#[derive(
    Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Display, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum SwitchBackend {
    /// Cisco NX-OS (Nexus) - NX-API
    #[default]
    Nxos,
    /// Cisco IOS-XE - RESTCONF
    Iosxe,
    /// Cisco IOS (legacy) - SSH
    Ios,
    /// Arista EOS - eAPI
    Eos,
    /// Juniper Junos - NETCONF
    Junos,
    /// Generic SSH
    Ssh,
}

impl SwitchBackend {
    pub fn default_port(&self) -> u16 {
        match self {
            SwitchBackend::Nxos => 443,
            SwitchBackend::Iosxe => 443,
            SwitchBackend::Ios => 22,
            SwitchBackend::Eos => 443,
            SwitchBackend::Junos => 830,
            SwitchBackend::Ssh => 22,
        }
    }
}
