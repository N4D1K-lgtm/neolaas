//! Sharding Coordinator Module
//!
//! Implements Maglev consistent hashing for distributing MachineActors
//! across cluster nodes with minimal disruption during topology changes.
//!
//! ## Architecture
//!
//! The ShardingCoordinator receives topology updates from the DiscoveryController
//! and maintains a Maglev hash table that maps machine IDs to owner nodes.
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
//!            MaglevHasher.rebuild()
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
mod maglev;
mod messages;

pub use actor::ShardingCoordinator;
pub use maglev::MaglevHasher;
pub use messages::{GetShardStats, LookupMachine, LookupResult, ShardStats, TopologyAck, TopologyChanged};
