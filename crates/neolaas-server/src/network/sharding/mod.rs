//! Sharding Coordinator Module
//!
//! Implements Rendezvous consistent hashing for distributing MachineActors
//! across cluster nodes with minimal disruption during topology changes.
//!
//! ## Architecture
//!
//! The ShardingCoordinator receives topology updates from the DiscoveryController
//! and maintains a hash table that maps machine IDs to owner nodes.
//!
//! ```text
//! etcd watch event (PUT/DELETE)
//!        ↓
//! DiscoveryPhases.process_watch_event()
//!        ↓
//!    ┌───┴───┐
//!    ↓       ↓
//! swarm_tx   sharding_tx
//!    ↓       ↓
//! Kademlia   ShardingCoordinator
//!            ↓
//!            RendezvousHasher.rebuild()
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! // Query ownership
//! let result = sharding_ref.ask(LookupMachine { machine_id }).send().await?;
//! if result.is_local {
//!     // Spawn actor locally
//! } else {
//!     // Route to remote node via result.owner_peer
//! }
//! ```

mod actor;
mod messages;
mod rendezvous;

pub use actor::ShardingCoordinator;
pub use messages::{GetShardStats, LookupMachine, LookupResult, ShardStats, TopologyAck, TopologyChanged};
pub use rendezvous::RendezvousHasher;
