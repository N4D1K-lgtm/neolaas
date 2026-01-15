//! MachineActor Message Types
//!
//! Messages for machine lifecycle management and status queries.

use kameo::Reply;
use serde::{Deserialize, Serialize};

/// Get the current status of a machine.
#[derive(Debug, Clone)]
pub struct GetMachineStatus;

/// Machine status response.
#[derive(Debug, Clone, Serialize, Deserialize, Reply)]
pub struct MachineStatus {
    /// Machine identifier (name from K8s)
    pub machine_id: String,
    /// Current operational state
    pub state: MachineState,
    /// Last observed admin state from spec
    pub admin_state: String,
    /// Cluster this machine belongs to
    pub cluster: String,
}

/// Operational state of a machine actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineState {
    /// Actor just started, loading state
    Initializing,
    /// Machine is idle and available
    Idle,
    /// Machine is currently booked/in-use
    Booked,
    /// Machine is being provisioned
    Provisioning,
    /// Machine is being deprovisioned
    Deprovisioning,
    /// Machine encountered an error
    Error,
}

impl std::fmt::Display for MachineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MachineState::Initializing => write!(f, "initializing"),
            MachineState::Idle => write!(f, "idle"),
            MachineState::Booked => write!(f, "booked"),
            MachineState::Provisioning => write!(f, "provisioning"),
            MachineState::Deprovisioning => write!(f, "deprovisioning"),
            MachineState::Error => write!(f, "error"),
        }
    }
}

/// Update machine spec (sent when etcd watch detects change).
#[derive(Debug, Clone)]
pub struct UpdateMachineSpec {
    /// Updated machine spec as JSON
    pub spec_json: String,
}

/// Acknowledgement of spec update.
#[derive(Debug, Clone, Reply)]
pub struct UpdateAck {
    /// Whether the update was applied successfully
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Request to provision a machine for a booking.
#[derive(Debug, Clone)]
pub struct ProvisionMachine {
    /// Booking identifier
    pub booking_id: String,
    /// Requested configuration (e.g., OS image, network settings)
    pub config: ProvisionConfig,
}

/// Provisioning configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionConfig {
    /// OS image to deploy
    pub image: String,
    /// Network VLAN assignment
    pub vlan_id: Option<u16>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Result of provisioning request.
#[derive(Debug, Clone, Reply)]
pub struct ProvisionResult {
    /// Whether provisioning started successfully
    pub success: bool,
    /// Error message if failed to start
    pub error: Option<String>,
}

/// Request to deprovision a machine.
#[derive(Debug, Clone)]
pub struct DeprovisionMachine {
    /// Booking identifier to end
    pub booking_id: String,
}

/// Result of deprovisioning request.
#[derive(Debug, Clone, Reply)]
pub struct DeprovisionResult {
    /// Whether deprovisioning started successfully
    pub success: bool,
    /// Error message if failed to start
    pub error: Option<String>,
}

/// Internal message for rebalancing check.
/// Sent by MachineActorManager when topology changes.
#[derive(Debug, Clone)]
pub struct CheckOwnership {
    /// Current owner peer ID from fresh sharding lookup
    pub is_still_local: bool,
}

/// Result of ownership check.
#[derive(Debug, Clone, Reply)]
pub struct OwnershipCheckResult {
    /// Should this actor shut down?
    pub should_shutdown: bool,
}
