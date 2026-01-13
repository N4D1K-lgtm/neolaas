//! Kubernetes-based peer discovery
//!
//! Discovers peer pods using Kubernetes headless service DNS queries.
//! Works with any deployment pattern (StatefulSet, Deployment, DaemonSet, etc.)

use std::net::IpAddr;
use tracing::{debug, error, info};

/// Information about a discovered peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub pod_name: String,
    pub address: IpAddr,
    pub port: u16,
}

impl PeerInfo {
    pub fn multiaddr(&self) -> String {
        format!("/ip4/{}/tcp/{}", self.address, self.port)
    }
}

/// Kubernetes DNS-based peer discovery
///
/// Discovers peers using Kubernetes headless service DNS resolution.
/// Works with any Kubernetes deployment pattern (StatefulSet, Deployment, DaemonSet, etc.)
pub struct KubernetesPeerDiscovery {
    /// Headless service DNS name (e.g., "neolaas-headless.infra.svc.cluster.local")
    service_dns: String,
    /// Port for P2P communication
    p2p_port: u16,
    /// Current pod IP (to exclude self from discovered peers)
    self_pod_ip: Option<String>,
}

impl KubernetesPeerDiscovery {
    pub fn new(service_dns: String, p2p_port: u16, self_pod_ip: Option<String>) -> Self {
        Self {
            service_dns,
            p2p_port,
            self_pod_ip,
        }
    }

    /// Create from environment variables with sensible defaults
    ///
    /// Required environment variables:
    /// - `HEADLESS_SERVICE_DNS`: Full DNS name of headless service
    ///   OR
    /// - `HEADLESS_SERVICE_NAME` + `NAMESPACE`: Service name and namespace (will construct DNS)
    ///
    /// Optional environment variables:
    /// - `P2P_PORT`: Port for P2P communication (default: 9000)
    /// - `POD_IP`: Current pod IP address (used to exclude self from peers)
    pub fn from_env() -> Result<Self, anyhow::Error> {
        // Get service DNS (either full DNS or construct from name + namespace)
        let service_dns = if let Ok(dns) = std::env::var("HEADLESS_SERVICE_DNS") {
            dns
        } else {
            let namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".to_string());
            let service_name = std::env::var("HEADLESS_SERVICE_NAME")
                .unwrap_or_else(|_| "neolaas-headless".to_string());
            format!("{}.{}.svc.cluster.local", service_name, namespace)
        };

        let p2p_port = std::env::var("P2P_PORT")
            .unwrap_or_else(|_| "9000".to_string())
            .parse::<u16>()?;

        let self_pod_ip = std::env::var("POD_IP").ok();

        Ok(Self::new(service_dns, p2p_port, self_pod_ip))
    }

    /// Discover all peer pods using headless service DNS lookup
    ///
    /// This method works with any Kubernetes deployment pattern (StatefulSet, Deployment, etc.)
    /// by querying the headless service which returns all pod IPs.
    pub async fn discover_peers(&self) -> Result<Vec<PeerInfo>, anyhow::Error> {
        info!(
            target: "neolaas::p2p::discovery",
            service_dns = %self.service_dns,
            self_pod_ip = ?self.self_pod_ip,
            "Starting peer discovery via headless service DNS"
        );

        // Perform DNS lookup on the headless service
        match tokio::net::lookup_host(&format!("{}:0", self.service_dns)).await {
            Ok(addresses) => {
                let mut peers = Vec::new();

                for addr in addresses {
                    let ip = addr.ip();

                    // Skip self if we know our own IP
                    if let Some(ref self_ip) = self.self_pod_ip {
                        if ip.to_string() == *self_ip {
                            debug!(
                                target: "neolaas::p2p::discovery",
                                ip = %ip,
                                "Skipping self IP"
                            );
                            continue;
                        }
                    }

                    debug!(
                        target: "neolaas::p2p::discovery",
                        ip = %ip,
                        "Discovered peer IP"
                    );

                    peers.push(PeerInfo {
                        pod_name: format!("pod-{}", ip),
                        address: ip,
                        port: self.p2p_port,
                    });
                }

                info!(
                    target: "neolaas::p2p::discovery",
                    peer_count = peers.len(),
                    "Peer discovery completed via DNS"
                );
                Ok(peers)
            }
            Err(e) => {
                error!(
                    target: "neolaas::p2p::discovery",
                    service_dns = %self.service_dns,
                    error = %e,
                    "Failed to resolve headless service DNS"
                );
                Err(e.into())
            }
        }
    }
}

impl Clone for KubernetesPeerDiscovery {
    fn clone(&self) -> Self {
        Self {
            service_dns: self.service_dns.clone(),
            p2p_port: self.p2p_port,
            self_pod_ip: self.self_pod_ip.clone(),
        }
    }
}
