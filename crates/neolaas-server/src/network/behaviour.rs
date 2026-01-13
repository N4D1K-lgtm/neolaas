//! Production-quality libp2p NetworkBehaviour combining Kameo remote actors
//! with standard peer discovery protocols (mDNS, Kademlia, Identify)

use kameo::remote;
use libp2p::{identify, kad, mdns, swarm::NetworkBehaviour, PeerId};

/// Production-grade network behaviour with automatic peer discovery
///
/// Combines:
/// - **Kameo**: Remote actor messaging
/// - **Identify**: Peer information exchange
/// - **mDNS**: Local network peer discovery (automatic, event-driven)
/// - **Kademlia**: Distributed hash table for peer routing
#[derive(NetworkBehaviour)]
pub struct NeolaasNetworkBehaviour {
    /// Kameo remote actor capabilities
    pub kameo: remote::Behaviour,
    /// Identify protocol for peer information exchange
    pub identify: identify::Behaviour,
    /// mDNS for local network peer discovery (great for Kubernetes)
    pub mdns: mdns::tokio::Behaviour,
    /// Kademlia DHT for distributed peer discovery and routing
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

impl NeolaasNetworkBehaviour {
    pub fn new(
        local_peer_id: PeerId,
        local_public_key: libp2p::identity::PublicKey,
    ) -> Result<Self, std::io::Error> {
        // Kameo for remote actor messaging
        let kameo = remote::Behaviour::new(
            local_peer_id,
            remote::messaging::Config::default()
                .with_request_timeout(std::time::Duration::from_secs(30))
                .with_max_concurrent_streams(100),
        );

        // Identify protocol for peer information exchange
        let identify = identify::Behaviour::new(identify::Config::new(
            "/neolaas/1.0.0".to_string(),
            local_public_key.clone(),
        ));

        // mDNS for automatic local network peer discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        // Kademlia DHT for distributed peer discovery
        let mut kademlia =
            kad::Behaviour::new(local_peer_id, kad::store::MemoryStore::new(local_peer_id));

        // Set Kademlia to server mode (can serve requests)
        kademlia.set_mode(Some(kad::Mode::Server));

        Ok(Self {
            kameo,
            identify,
            mdns,
            kademlia,
        })
    }
}
