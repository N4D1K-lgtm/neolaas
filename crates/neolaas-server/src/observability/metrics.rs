//! Prometheus Metrics
//!
//! Defines and initializes all Prometheus metrics for the neolaas server.
//!
//! Metrics tracked:
//! - `neolaas_actor_count` - gauge of active actors by type
//! - `neolaas_actor_spawned_total` - counter of spawned actors
//! - `neolaas_actor_stopped_total` - counter of stopped actors with reason
//! - `neolaas_message_processing_duration_seconds` - histogram of message handling times
//! - `neolaas_topology_changes_total` - counter of topology changes
//! - `neolaas_peer_count` - gauge of connected peers
//! - `neolaas_peer_ping_duration_seconds` - histogram of ping latencies
//! - `neolaas_machines_by_state` - gauge of machines by operational state

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::time::Duration;

/// State containing the Prometheus handle for metrics export
#[derive(Clone)]
pub struct MetricsState {
    pub prometheus_handle: PrometheusHandle,
}

/// Initialize Prometheus metrics and return the handle for exporting.
///
/// This function:
/// 1. Sets up the Prometheus recorder
/// 2. Registers all metric descriptions
/// 3. Returns a handle that can be used to render metrics
pub fn init_metrics() -> Result<MetricsState, Box<dyn std::error::Error + Send + Sync>> {
    let handle = PrometheusBuilder::new().install_recorder()?;

    // Register metric descriptions
    register_metric_descriptions();

    Ok(MetricsState {
        prometheus_handle: handle,
    })
}

/// Register descriptions for all metrics
fn register_metric_descriptions() {
    // Actor metrics
    describe_gauge!(
        "neolaas_actor_count",
        "Number of active actors by type"
    );
    describe_counter!(
        "neolaas_actor_spawned_total",
        "Total number of actors spawned"
    );
    describe_counter!(
        "neolaas_actor_stopped_total",
        "Total number of actors stopped"
    );
    describe_histogram!(
        "neolaas_message_processing_duration_seconds",
        "Duration of message processing in seconds"
    );

    // Topology metrics
    describe_counter!(
        "neolaas_topology_changes_total",
        "Total number of topology changes"
    );
    describe_gauge!(
        "neolaas_peer_count",
        "Number of connected peers"
    );
    describe_histogram!(
        "neolaas_peer_ping_duration_seconds",
        "Duration of peer ping round-trips in seconds"
    );

    // Machine metrics
    describe_gauge!(
        "neolaas_machines_by_state",
        "Number of machines by operational state"
    );
}

/// Record that an actor was spawned
pub fn record_actor_spawned(actor_type: &str) {
    counter!("neolaas_actor_spawned_total", "actor_type" => actor_type.to_string()).increment(1);
    gauge!("neolaas_actor_count", "actor_type" => actor_type.to_string()).increment(1.0);
}

/// Record that an actor was stopped
pub fn record_actor_stopped(actor_type: &str, reason: &str) {
    counter!(
        "neolaas_actor_stopped_total",
        "actor_type" => actor_type.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
    gauge!("neolaas_actor_count", "actor_type" => actor_type.to_string()).decrement(1.0);
}

/// Record message processing duration
pub fn record_message_duration(actor_type: &str, message_type: &str, duration: Duration) {
    histogram!(
        "neolaas_message_processing_duration_seconds",
        "actor_type" => actor_type.to_string(),
        "message_type" => message_type.to_string()
    )
    .record(duration.as_secs_f64());
}

/// Record a topology change
pub fn record_topology_change() {
    counter!("neolaas_topology_changes_total").increment(1);
}

/// Update the peer count gauge
pub fn set_peer_count(count: usize) {
    gauge!("neolaas_peer_count").set(count as f64);
}

/// Record a ping duration
pub fn record_ping_duration(peer_id: &str, duration: Duration) {
    histogram!(
        "neolaas_peer_ping_duration_seconds",
        "peer_id" => peer_id.to_string()
    )
    .record(duration.as_secs_f64());
}

/// Update the machines by state gauge
pub fn set_machines_by_state(state: &str, count: usize) {
    gauge!(
        "neolaas_machines_by_state",
        "state" => state.to_string()
    )
    .set(count as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_recording() {
        // These functions should not panic when called
        record_actor_spawned("test_actor");
        record_actor_stopped("test_actor", "normal");
        record_message_duration("test_actor", "test_message", Duration::from_millis(100));
        record_topology_change();
        set_peer_count(5);
        record_ping_duration("test_peer", Duration::from_millis(50));
        set_machines_by_state("idle", 10);
    }
}
