//! MachineActor Implementation
//!
//! Each MachineActor manages the lifecycle of a single physical machine.
//! Actors are spawned by the MachineActorManager based on sharding assignments.

use super::messages::{
    CheckOwnership, DeprovisionMachine, DeprovisionResult, GetMachineStatus, MachineState,
    MachineStatus, OwnershipCheckResult, ProvisionMachine, ProvisionResult, UpdateAck,
    UpdateMachineSpec,
};
use crate::observability::events;
use kameo::{
    message::{Context, Message},
    Actor,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};
use tracing::{debug, info, warn};

/// Simplified machine spec for actor use.
/// This mirrors the essential fields from the operator's MachineSpec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSpec {
    pub admin_state: String,
    pub cluster: String,
    #[serde(default)]
    pub reserved_for: Option<String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

/// Actor managing a single physical machine's lifecycle.
///
/// Responsible for:
/// - Tracking machine state (idle, booked, provisioning, etc.)
/// - Handling provision/deprovision requests
/// - Responding to spec updates from etcd
/// - Graceful shutdown on ownership transfer
#[derive(Actor)]
pub struct MachineActor {
    /// Machine identifier (name from K8s metadata)
    machine_id: String,
    /// Current machine specification
    spec: MachineSpec,
    /// Current operational state
    state: MachineState,
    /// Previous state (for event emission)
    previous_state: MachineState,
    /// Active booking ID if machine is booked
    current_booking: Option<String>,
    /// Last activity timestamp (Unix seconds)
    last_activity: AtomicI64,
    /// Node ID for event context
    #[allow(dead_code)]
    node_id: String,
}

impl MachineActor {
    /// Create a new MachineActor for the given machine.
    pub fn new(machine_id: String, spec_json: &str) -> Result<Self, serde_json::Error> {
        Self::with_node_id(machine_id, spec_json, "unknown".to_string())
    }

    /// Create a new MachineActor with node ID for event context.
    pub fn with_node_id(
        machine_id: String,
        spec_json: &str,
        node_id: String,
    ) -> Result<Self, serde_json::Error> {
        let spec: MachineSpec = serde_json::from_str(spec_json)?;

        info!(
            machine_id = %machine_id,
            admin_state = %spec.admin_state,
            cluster = %spec.cluster,
            "Creating MachineActor"
        );

        Ok(Self {
            machine_id,
            spec,
            state: MachineState::Initializing,
            previous_state: MachineState::Initializing,
            current_booking: None,
            last_activity: AtomicI64::new(chrono::Utc::now().timestamp()),
            node_id,
        })
    }

    /// Create with an already-parsed spec.
    pub fn with_spec(machine_id: String, spec: MachineSpec) -> Self {
        Self::with_spec_and_node_id(machine_id, spec, "unknown".to_string())
    }

    /// Create with an already-parsed spec and node ID.
    pub fn with_spec_and_node_id(machine_id: String, spec: MachineSpec, node_id: String) -> Self {
        info!(
            machine_id = %machine_id,
            admin_state = %spec.admin_state,
            cluster = %spec.cluster,
            "Creating MachineActor"
        );

        Self {
            machine_id,
            spec,
            state: MachineState::Initializing,
            previous_state: MachineState::Initializing,
            current_booking: None,
            last_activity: AtomicI64::new(chrono::Utc::now().timestamp()),
            node_id,
        }
    }

    /// Get the machine ID.
    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }

    /// Transition to idle state after initialization.
    fn initialize(&mut self) {
        if self.state == MachineState::Initializing {
            self.transition_to(MachineState::Idle);
            debug!(machine_id = %self.machine_id, "Machine initialized to idle state");
        }
    }

    /// Transition to a new state, emitting events and updating metrics.
    fn transition_to(&mut self, new_state: MachineState) {
        if self.state != new_state {
            self.previous_state = self.state;
            self.state = new_state;
            self.update_activity();

            events::machine_state_changed(
                &self.machine_id,
                &self.previous_state.to_string(),
                &self.state.to_string(),
                &self.spec.admin_state,
                &self.spec.cluster,
            );
        }
    }

    /// Update the last activity timestamp.
    fn update_activity(&self) {
        self.last_activity
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    }
}

impl Message<GetMachineStatus> for MachineActor {
    type Reply = MachineStatus;

    async fn handle(
        &mut self,
        _msg: GetMachineStatus,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        // Ensure we're initialized
        self.initialize();

        MachineStatus {
            machine_id: self.machine_id.clone(),
            state: self.state,
            admin_state: self.spec.admin_state.clone(),
            cluster: self.spec.cluster.clone(),
            current_booking: self.current_booking.clone(),
            last_activity: Some(self.last_activity.load(Ordering::Relaxed)),
        }
    }
}

impl Message<UpdateMachineSpec> for MachineActor {
    type Reply = UpdateAck;

    async fn handle(
        &mut self,
        msg: UpdateMachineSpec,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match serde_json::from_str::<MachineSpec>(&msg.spec_json) {
            Ok(new_spec) => {
                debug!(
                    machine_id = %self.machine_id,
                    old_admin_state = %self.spec.admin_state,
                    new_admin_state = %new_spec.admin_state,
                    "Updating machine spec"
                );
                self.spec = new_spec;
                UpdateAck {
                    success: true,
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    machine_id = %self.machine_id,
                    error = %e,
                    "Failed to parse updated spec"
                );
                UpdateAck {
                    success: false,
                    error: Some(e.to_string()),
                }
            }
        }
    }
}

impl Message<ProvisionMachine> for MachineActor {
    type Reply = ProvisionResult;

    async fn handle(
        &mut self,
        msg: ProvisionMachine,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.initialize();

        // Check if machine is available for provisioning
        if self.state != MachineState::Idle {
            return ProvisionResult {
                success: false,
                error: Some(format!(
                    "Machine is not idle (current state: {})",
                    self.state
                )),
            };
        }

        // Check admin state allows booking
        if self.spec.admin_state != "available" && self.spec.admin_state != "reserved" {
            return ProvisionResult {
                success: false,
                error: Some(format!(
                    "Machine admin state '{}' does not allow booking",
                    self.spec.admin_state
                )),
            };
        }

        // If reserved, check reservation
        if self.spec.admin_state == "reserved" {
            if let Some(ref reserved_for) = self.spec.reserved_for {
                // In a real implementation, we'd check if the booking is for the reserved entity
                debug!(
                    machine_id = %self.machine_id,
                    reserved_for = %reserved_for,
                    booking_id = %msg.booking_id,
                    "Machine is reserved, proceeding with provision"
                );
            }
        }

        info!(
            machine_id = %self.machine_id,
            booking_id = %msg.booking_id,
            image = %msg.config.image,
            "Starting provisioning"
        );

        self.current_booking = Some(msg.booking_id.clone());
        self.transition_to(MachineState::Provisioning);

        // Emit booking started event
        events::booking_started(&self.machine_id, &msg.booking_id, &self.spec.cluster);

        // TODO: Actually trigger BMC/PXE provisioning flow
        // For now, just transition to booked state
        self.transition_to(MachineState::Booked);

        ProvisionResult {
            success: true,
            error: None,
        }
    }
}

impl Message<DeprovisionMachine> for MachineActor {
    type Reply = DeprovisionResult;

    async fn handle(
        &mut self,
        msg: DeprovisionMachine,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.initialize();

        // Check if this is the right booking
        if self.current_booking.as_ref() != Some(&msg.booking_id) {
            return DeprovisionResult {
                success: false,
                error: Some(format!(
                    "Booking ID mismatch: expected {:?}, got {}",
                    self.current_booking, msg.booking_id
                )),
            };
        }

        if self.state != MachineState::Booked {
            return DeprovisionResult {
                success: false,
                error: Some(format!(
                    "Machine is not booked (current state: {})",
                    self.state
                )),
            };
        }

        info!(
            machine_id = %self.machine_id,
            booking_id = %msg.booking_id,
            "Starting deprovisioning"
        );

        self.transition_to(MachineState::Deprovisioning);

        // TODO: Actually trigger cleanup/wipe flow
        // For now, just transition back to idle
        self.transition_to(MachineState::Idle);

        // Emit booking ended event (duration would be calculated from actual timestamps)
        events::booking_ended(&self.machine_id, &msg.booking_id, &self.spec.cluster, 0);

        self.current_booking = None;

        DeprovisionResult {
            success: true,
            error: None,
        }
    }
}

impl Message<CheckOwnership> for MachineActor {
    type Reply = OwnershipCheckResult;

    async fn handle(
        &mut self,
        msg: CheckOwnership,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if !msg.is_still_local {
            info!(
                machine_id = %self.machine_id,
                state = %self.state,
                "Ownership transferred, actor will shut down"
            );

            // If we're in the middle of something, we might want to handle this differently
            // For now, we'll allow shutdown regardless
            return OwnershipCheckResult {
                should_shutdown: true,
            };
        }

        OwnershipCheckResult {
            should_shutdown: false,
        }
    }
}
