//! Vlan CRD
//!
//! Defines the Vlan custom resource for allocatable VLAN pool.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use strum::{Display, EnumString};

/// Vlan represents an allocatable VLAN in the lab.
///
/// VLANs are a limited resource - there's a finite pool of publicly
/// accessible VLANs that can be allocated to bookings.
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "neolaas.io",
    version = "v1alpha1",
    kind = "Vlan",
    namespaced,
    printcolumn = r#"{"name":"VLAN ID", "type":"integer", "jsonPath":".spec.vlanId"}"#,
    printcolumn = r#"{"name":"Public", "type":"boolean", "jsonPath":".spec.public"}"#,
    printcolumn = r#"{"name":"Age", "type":"date", "jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct VlanSpec {
    /// VLAN ID (1-4094).
    pub vlan_id: VlanId,

    /// Whether this VLAN has public/external connectivity.
    #[serde(default)]
    pub public: bool,

    /// Optional subnet for this VLAN (informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subnet: Option<String>,

    /// Optional gateway for this VLAN (informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,

    /// Administrative state.
    #[serde(default)]
    pub admin_state: VlanAdminState,

    /// Arbitrary labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// VLAN ID (1-4094).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(try_from = "u16", into = "u16")]
pub struct VlanId(u16);

impl VlanId {
    pub fn new(id: u16) -> Result<Self, &'static str> {
        if id == 0 || id > 4094 {
            Err("VLAN ID must be between 1 and 4094")
        } else {
            Ok(Self(id))
        }
    }

    pub fn get(&self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for VlanId {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<VlanId> for u16 {
    fn from(vlan: VlanId) -> Self {
        vlan.0
    }
}

impl std::fmt::Display for VlanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Administrative state for VLANs.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum VlanAdminState {
    /// Available for allocation.
    #[default]
    Available,
    /// Reserved/not allocatable.
    Reserved,
    /// Disabled.
    Disabled,
}

impl VlanAdminState {
    pub fn is_allocatable(&self) -> bool {
        matches!(self, VlanAdminState::Available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vlan_id_validation() {
        assert!(VlanId::new(1).is_ok());
        assert!(VlanId::new(4094).is_ok());
        assert!(VlanId::new(0).is_err());
        assert!(VlanId::new(4095).is_err());
    }
}
