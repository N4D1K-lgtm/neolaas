//! Data models for neolaas-server

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Host allocation (part of a booking)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostAllocation {
    /// Allocation ID
    pub id: Uuid,
    /// Host identifier
    pub host_id: String,
    /// Booking this host belongs to
    pub booking_id: Uuid,
    /// Who owns this allocation
    pub owner: String,
    /// When allocated
    pub allocated_at: DateTime<Utc>,
    /// When allocation expires (booking end time)
    pub expires_at: DateTime<Utc>,
    /// Current provisioning state
    pub provision_state: ProvisionState,
    /// Allocation metadata
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionState {
    /// Host is being provisioned
    Provisioning,
    /// Host is ready to use
    Ready,
    /// Host is being reprovisioned
    Reprovisioning,
    /// Host is being released
    Releasing,
    /// Host has been released
    Released,
}

impl HostAllocation {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn is_active(&self) -> bool {
        !self.is_expired() && self.provision_state != ProvisionState::Released
    }
}

/// VLAN allocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlanAllocation {
    /// Allocation ID
    pub id: Uuid,
    /// VLAN number (auto-assigned)
    pub vlan_id: u16,
    /// Booking this VLAN belongs to
    pub booking_id: Uuid,
    /// Purpose of this VLAN
    pub purpose: String,
    /// When allocated
    pub allocated_at: DateTime<Utc>,
    /// When allocation expires
    pub expires_at: DateTime<Utc>,
}

/// Booking (collection of hosts and VLANs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Booking {
    /// Booking ID
    pub id: Uuid,
    /// Booking owner
    pub owner: String,
    /// Booking start time
    pub start_time: DateTime<Utc>,
    /// Booking end time (can be extended)
    pub end_time: DateTime<Utc>,
    /// Hosts in this booking
    pub host_allocations: Vec<Uuid>,
    /// VLANs in this booking
    pub vlan_allocations: Vec<Uuid>,
    /// Booking status
    pub status: BookingStatus,
    /// Booking metadata
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookingStatus {
    /// Booking is scheduled for future
    Scheduled,
    /// Booking is active
    Active,
    /// Booking is being extended
    Extending,
    /// Booking has expired
    Expired,
    /// Booking was cancelled
    Cancelled,
}

impl Booking {
    pub fn is_active(&self) -> bool {
        let now = Utc::now();
        now >= self.start_time && now < self.end_time && self.status == BookingStatus::Active
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.end_time
    }
}
