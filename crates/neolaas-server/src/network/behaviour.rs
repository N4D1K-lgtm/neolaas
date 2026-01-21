//! libp2p NetworkBehaviour Configuration
//!
//! Combines multiple libp2p protocols into a single network behaviour:
//! - Kameo: Remote actor messaging over libp2p
//! - Identify: Peer information exchange (protocol version, listen addresses)
//! - mDNS: Automatic local network peer discovery (works well in Kubernetes)
//! - Kademlia: DHT for peer routing (configured for LAN/datacenter use)

use super::config::NetworkConfig;
use crate::version::{PROTOCOL_VERSION, full_version};
use kameo::remote;
use libp2p::{PeerId, identify, kad, mdns, swarm::NetworkBehaviour};

/// Combined network behaviour for P2P communication.
#[derive(NetworkBehaviour)]
pub struct NeolaasNetworkBehaviour {
    pub kameo: remote::Behaviour,
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

impl NeolaasNetworkBehaviour {
    pub fn new(
        local_peer_id: PeerId,
        local_public_key: libp2p::identity::PublicKey,
        config: &NetworkConfig,
    ) -> Result<Self, std::io::Error> {
        let kameo = remote::Behaviour::new(
            local_peer_id,
            remote::messaging::Config::default()
                .with_request_timeout(config.kameo_request_timeout)
                .with_max_concurrent_streams(config.kameo_max_streams as usize),
        );

        let identify = identify::Behaviour::new(
            identify::Config::new(PROTOCOL_VERSION.to_string(), local_public_key.clone())
                .with_agent_version(full_version()),
        );

        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        // Kademlia configured for LAN/datacenter environment with faster timeouts
        // and manual routing table control (etcd is the source of truth)
        let store = kad::store::MemoryStore::new(local_peer_id);
        let mut kad_config = kad::Config::default();

        kad_config
            .set_query_timeout(config.kademlia_query_timeout)
            .set_parallelism(config.kademlia_parallelism)
            .set_publication_interval(Some(config.kademlia_publication_interval))
            .set_provider_record_ttl(Some(config.kademlia_provider_ttl))
            .set_kbucket_inserts(kad::BucketInserts::Manual); // etcd controls routing

        let mut kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);
        kademlia.set_mode(Some(kad::Mode::Server));

        Ok(Self {
            kameo,
            identify,
            mdns,
            kademlia,
        })
    }
}
