//! OpenTelemetry Distributed Tracing
//!
//! Sets up distributed tracing with OpenTelemetry, supporting:
//! - OTLP export to Grafana Tempo or any OTLP-compatible collector
//! - W3C TraceContext propagation for remote actor messages
//! - Configurable via environment variables
//!
//! Environment variables:
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` - OTLP endpoint (e.g., `http://tempo.infra.svc.cluster.local:4317`)
//! - `OTEL_SERVICE_NAME` - Service name (default: `neolaas-server`)
//! - `LOG_FORMAT` - Set to `json` for JSON output (default: `text`)

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
    Resource,
};
use std::sync::OnceLock;
use tracing::level_filters::LevelFilter;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Global tracer provider for shutdown
static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// Configuration for tracing initialization
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// OTLP endpoint for trace export (None = disabled)
    pub otlp_endpoint: Option<String>,
    /// Service name for traces
    pub service_name: String,
    /// Log format: "text" or "json"
    pub log_format: String,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            service_name: std::env::var("OTEL_SERVICE_NAME")
                .unwrap_or_else(|_| "neolaas-server".to_string()),
            log_format: std::env::var("LOG_FORMAT").unwrap_or_else(|_| "text".to_string()),
        }
    }
}

impl TracingConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self::default()
    }
}

/// W3C TraceContext for propagation across remote actor messages.
///
/// This struct carries trace context between nodes, allowing distributed
/// traces to be correlated across the cluster.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TraceContext {
    /// W3C traceparent header value (trace_id)
    pub trace_id: Option<String>,
    /// Span ID within the trace
    pub span_id: Option<String>,
}

impl TraceContext {
    /// Create an empty TraceContext
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a TraceContext with specific IDs
    pub fn with_ids(trace_id: String, span_id: String) -> Self {
        Self {
            trace_id: Some(trace_id),
            span_id: Some(span_id),
        }
    }

    /// Check if this context has valid trace information
    pub fn is_valid(&self) -> bool {
        self.trace_id.is_some() && self.span_id.is_some()
    }
}

/// Initialize the tracing subscriber with optional OpenTelemetry export.
///
/// This function sets up:
/// 1. Console logging (text or JSON format)
/// 2. Environment-based log filtering via RUST_LOG
/// 3. OpenTelemetry export if OTEL_EXPORTER_OTLP_ENDPOINT is set
pub fn init_tracing(config: TracingConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Choose log format
    let is_json = config.log_format.to_lowercase() == "json";

    // Setup OpenTelemetry if endpoint is configured
    if let Some(endpoint) = &config.otlp_endpoint {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;

        let resource = Resource::builder()
            .with_service_name(config.service_name.clone())
            .build();

        let provider = SdkTracerProvider::builder()
            .with_sampler(Sampler::AlwaysOn)
            .with_id_generator(RandomIdGenerator::default())
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();

        let tracer = provider.tracer("neolaas-server");

        // Store provider for shutdown
        let _ = TRACER_PROVIDER.set(provider);

        let otel_layer = OpenTelemetryLayer::new(tracer);

        // Build env filter fresh for each branch to avoid move issues
        let env_filter = EnvFilter::builder()
            .with_default_directive(LevelFilter::INFO.into())
            .from_env_lossy();

        if is_json {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(fmt::layer().json())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(fmt::layer())
                .init();
        }

        tracing::info!(
            endpoint = %endpoint,
            service_name = %config.service_name,
            "OpenTelemetry tracing initialized"
        );
    } else {
        // No OTLP endpoint - just console logging
        let env_filter = EnvFilter::builder()
            .with_default_directive(LevelFilter::INFO.into())
            .from_env_lossy();

        if is_json {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer())
                .init();
        }

        tracing::debug!("Tracing initialized (no OTLP export)");
    }

    Ok(())
}

/// Shutdown the tracer provider gracefully.
///
/// This should be called during application shutdown to ensure
/// all pending traces are exported.
pub fn shutdown_tracing() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        if let Err(e) = provider.shutdown() {
            tracing::warn!(error = %e, "Error shutting down tracer provider");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_default() {
        let ctx = TraceContext::default();
        assert!(ctx.trace_id.is_none());
        assert!(ctx.span_id.is_none());
        assert!(!ctx.is_valid());
    }

    #[test]
    fn test_tracing_config_default() {
        let config = TracingConfig::default();
        assert_eq!(config.service_name, "neolaas-server");
        assert_eq!(config.log_format, "text");
    }
}
