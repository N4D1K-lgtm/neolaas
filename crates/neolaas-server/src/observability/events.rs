//! Structured Events
//!
//! Provides structured event logging with consistent fields across the application.
//! Each event type has a dedicated function that ensures consistent field naming
//! and emits both tracing spans and logs.
//!
//! Event types:
//! - `machine_state_changed` - Machine state transitions
//! - `topology_changed` - Cluster topology updates
//! - `actor_spawned` - Actor lifecycle start
//! - `actor_stopped` - Actor lifecycle end
//! - `actor_panicked` - Actor crash with restart info
//! - `booking_started` - Machine booking began
//! - `booking_ended` - Machine booking completed
//! - `peer_joined` - New peer discovered
//! - `peer_departed` - Peer left the cluster
//! - `ownership_transferred` - Machine ownership moved between nodes

use tracing::{error, info, warn};

/// Emit a machine state changed event
pub fn machine_state_changed(
    machine_id: &str,
    previous_state: &str,
    new_state: &str,
    admin_state: &str,
    cluster: &str,
) {
    info!(
        event_type = "machine_state_changed",
        machine_id = %machine_id,
        previous_state = %previous_state,
        new_state = %new_state,
        admin_state = %admin_state,
        cluster = %cluster,
        "Machine state changed"
    );
}

/// Emit a topology changed event
pub fn topology_changed(previous_peer_count: usize, new_peer_count: usize, local_peer_id: &str) {
    info!(
        event_type = "topology_changed",
        previous_peer_count = previous_peer_count,
        new_peer_count = new_peer_count,
        local_peer_id = %local_peer_id,
        "Topology changed"
    );
}

/// Emit an actor spawned event
pub fn actor_spawned(actor_type: &str, actor_id: &str, node_id: &str) {
    info!(
        event_type = "actor_spawned",
        actor_type = %actor_type,
        actor_id = %actor_id,
        node_id = %node_id,
        "Actor spawned"
    );
}

/// Emit an actor stopped event
pub fn actor_stopped(actor_type: &str, actor_id: &str, reason: &str, node_id: &str) {
    info!(
        event_type = "actor_stopped",
        actor_type = %actor_type,
        actor_id = %actor_id,
        reason = %reason,
        node_id = %node_id,
        "Actor stopped"
    );
}

/// Emit an actor panicked event
pub fn actor_panicked(
    actor_type: &str,
    actor_id: &str,
    error: &str,
    node_id: &str,
    will_restart: bool,
) {
    error!(
        event_type = "actor_panicked",
        actor_type = %actor_type,
        actor_id = %actor_id,
        error = %error,
        node_id = %node_id,
        will_restart = will_restart,
        "Actor panicked"
    );
}

/// Emit a booking started event
pub fn booking_started(machine_id: &str, booking_id: &str, cluster: &str) {
    info!(
        event_type = "booking_started",
        machine_id = %machine_id,
        booking_id = %booking_id,
        cluster = %cluster,
        "Booking started"
    );
}

/// Emit a booking ended event
pub fn booking_ended(machine_id: &str, booking_id: &str, cluster: &str, duration_secs: u64) {
    info!(
        event_type = "booking_ended",
        machine_id = %machine_id,
        booking_id = %booking_id,
        cluster = %cluster,
        duration_secs = duration_secs,
        "Booking ended"
    );
}

/// Emit a peer joined event
pub fn peer_joined(peer_id: &str, address: &str, cluster_id: &str) {
    info!(
        event_type = "peer_joined",
        peer_id = %peer_id,
        address = %address,
        cluster_id = %cluster_id,
        "Peer joined"
    );
}

/// Emit a peer departed event
pub fn peer_departed(peer_id: &str, cluster_id: &str) {
    warn!(
        event_type = "peer_departed",
        peer_id = %peer_id,
        cluster_id = %cluster_id,
        "Peer departed"
    );
}

/// Emit an ownership transferred event
pub fn ownership_transferred(machine_id: &str, previous_owner: &str, new_owner: &str) {
    info!(
        event_type = "ownership_transferred",
        machine_id = %machine_id,
        previous_owner = %previous_owner,
        new_owner = %new_owner,
        "Ownership transferred"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_functions_dont_panic() {
        // These should not panic
        machine_state_changed("machine-1", "idle", "booked", "available", "default");
        topology_changed(3, 5, "peer-123");
        actor_spawned("MachineActor", "machine-1", "node-1");
        actor_stopped("MachineActor", "machine-1", "ownership_transfer", "node-1");
        actor_panicked("MachineActor", "machine-1", "test error", "node-1", true);
        booking_started("machine-1", "booking-123", "default");
        booking_ended("machine-1", "booking-123", "default", 3600);
        peer_joined("peer-123", "/ip4/10.0.0.1/tcp/4001", "cluster-1");
        peer_departed("peer-123", "cluster-1");
        ownership_transferred("machine-1", "peer-123", "peer-456");
    }
}
