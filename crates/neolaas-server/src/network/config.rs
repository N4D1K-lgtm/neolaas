//! Network Configuration
//!
//! Centralized configuration for all network-related settings with environment
//! variable overrides. All hardcoded values from the network module have been
//! extracted here for better maintainability.

use std::num::NonZeroUsize;
use std::time::Duration;

/// Network configuration with sensible defaults
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    // Connection settings
    /// Idle connection timeout before closing
    pub connection_idle_timeout: Duration,

    // Port configuration
    /// P2P listen port (env: P2P_PORT)
    pub p2p_port: u16,

    // Cluster configuration
    /// Cluster identifier for namespace isolation (env: CLUSTER_ID)
    pub cluster_id: String,

    /// Etcd endpoints (env: ETCD_ENDPOINTS, comma-separated)
    pub etcd_endpoints: Vec<String>,

    /// Pod IP address (env: POD_IP)
    pub pod_ip: String,

    // Initialization settings
    /// Brief delay to allow swarm and discovery controller to initialize
    pub initialization_delay: Duration,

    // Cleanup settings
    /// Interval for periodic cleanup of stale failed peers
    pub cleanup_interval: Duration,

    /// Maximum age for failed peers before cleanup
    pub stale_peer_max_age: Duration,

    // Health monitoring settings
    /// Interval for periodic ping loop
    pub ping_interval: Duration,

    /// Initial delay before starting ping loop
    pub ping_initial_delay: Duration,

    // Failure thresholds
    /// Number of failures before applying exponential backoff
    pub failure_threshold: u32,

    /// Maximum consecutive failures before marking peer as failed
    pub max_failures: u32,

    /// Base for exponential backoff calculation
    pub backoff_base: u64,

    /// Maximum power for exponential backoff (2^max_backoff_power)
    pub max_backoff_power: u32,

    // Etcd discovery settings
    /// TTL for etcd leases (seconds)
    pub lease_ttl: i64,

    /// Interval for lease keep-alive (1/3 of TTL recommended)
    pub keepalive_interval: Duration,

    /// Wait time for DELETE event propagation after lease revocation
    pub delete_propagation_wait: Duration,

    /// Delay before reconnecting after watch stream failure
    pub reconnect_delay: Duration,

    // Etcd backoff settings
    /// Initial interval for etcd connection retry
    pub etcd_backoff_initial: Duration,

    /// Maximum interval for etcd connection retry
    pub etcd_backoff_max: Duration,

    /// Maximum elapsed time for etcd connection retries
    pub etcd_backoff_max_elapsed: Duration,

    /// Multiplier for etcd backoff
    pub etcd_backoff_multiplier: f64,

    // Kameo remote actor settings
    /// Request timeout for remote actor messaging
    pub kameo_request_timeout: Duration,

    /// Maximum concurrent streams for kameo
    pub kameo_max_streams: u32,

    // Kademlia DHT settings
    /// Query timeout for Kademlia DHT
    pub kademlia_query_timeout: Duration,

    /// Parallelism for Kademlia queries
    pub kademlia_parallelism: NonZeroUsize,

    /// Publication interval for Kademlia
    pub kademlia_publication_interval: Duration,

    /// Provider record TTL for Kademlia
    pub kademlia_provider_ttl: Duration,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            // Connection settings (from init.rs:60)
            connection_idle_timeout: Duration::from_secs(300),

            // Port configuration (from init.rs:76-78)
            p2p_port: 9000,

            // Cluster configuration (from init.rs:94-100)
            cluster_id: "default".to_string(),
            etcd_endpoints: vec!["http://etcd.infra.svc.cluster.local:2379".to_string()],
            pod_ip: "127.0.0.1".to_string(),

            // Initialization settings (from init.rs:355)
            initialization_delay: Duration::from_millis(500),

            // Cleanup settings (from init.rs:345, 349)
            cleanup_interval: Duration::from_secs(60),
            stale_peer_max_age: Duration::from_secs(300),

            // Health monitoring settings (from init.rs:370, 374)
            ping_interval: Duration::from_secs(5),
            ping_initial_delay: Duration::from_secs(2),

            // Failure thresholds (from mod.rs:76, 112, 79)
            failure_threshold: 3,
            max_failures: 10,
            backoff_base: 2,
            max_backoff_power: 6,

            // Etcd discovery settings (from discovery_controller.rs:28, 31, 441, 479)
            lease_ttl: 15,
            keepalive_interval: Duration::from_secs(5),
            delete_propagation_wait: Duration::from_secs(5),
            reconnect_delay: Duration::from_secs(5),

            // Etcd backoff settings (from discovery_controller.rs:124-130)
            etcd_backoff_initial: Duration::from_secs(1),
            etcd_backoff_max: Duration::from_secs(10),
            etcd_backoff_max_elapsed: Duration::from_secs(60),
            etcd_backoff_multiplier: 2.0,

            // Kameo remote actor settings (from behaviour.rs:29, 30)
            kameo_request_timeout: Duration::from_secs(30),
            kameo_max_streams: 100,

            // Kademlia DHT settings (from behaviour.rs:46-49)
            kademlia_query_timeout: Duration::from_secs(3),
            kademlia_parallelism: NonZeroUsize::new(10).unwrap(),
            kademlia_publication_interval: Duration::from_secs(30 * 60),
            kademlia_provider_ttl: Duration::from_secs(60 * 60),
        }
    }
}

impl NetworkConfig {
    /// Create configuration from environment variables with defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Override from environment variables
        if let Ok(port) = std::env::var("P2P_PORT") {
            if let Ok(parsed) = port.parse::<u16>() {
                config.p2p_port = parsed;
            }
        }

        if let Ok(cluster_id) = std::env::var("CLUSTER_ID") {
            config.cluster_id = cluster_id;
        }

        if let Ok(endpoints) = std::env::var("ETCD_ENDPOINTS") {
            config.etcd_endpoints = endpoints.split(',').map(String::from).collect();
        }

        if let Ok(pod_ip) = std::env::var("POD_IP") {
            config.pod_ip = pod_ip;
        }

        config
    }
}
