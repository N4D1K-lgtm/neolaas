//! Observability Module
//!
//! Provides comprehensive observability for the neolaas distributed actor system:
//! - `metrics`: Prometheus metrics for actors, peers, and machines
//! - `events`: Structured event logging with consistent fields
//! - `tracing`: OpenTelemetry distributed tracing setup
//! - `supervision`: Restart policies and tracking for actor fault tolerance

pub mod events;
pub mod metrics;
pub mod supervision;
pub mod tracing;

pub use events::*;
pub use metrics::{init_metrics, record_ping_duration, MetricsState};
pub use supervision::{RestartPolicy, RestartTracker};
pub use tracing::{init_tracing, shutdown_tracing, TraceContext, TracingConfig};
