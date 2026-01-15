//! Machine Actor Module
//!
//! Provides MachineActors that manage individual physical machines,
//! coordinated by the MachineActorManager using Maglev consistent hashing.
//!
//! ## Architecture
//!
//! ```text
//! etcd watch (/neolaas/machines/*)
//!        ↓
//! MachineActorManager
//!        ↓
//! ShardingCoordinator.ask(LookupMachine)
//!        ↓
//!    ┌───┴───┐
//!    ↓       ↓
//! is_local  remote
//!    ↓
//! spawn MachineActor
//! ```
//!
//! ## Usage
//!
//! The MachineActorManager is spawned during network initialization and
//! automatically watches etcd for machine changes. It queries the
//! ShardingCoordinator to determine which machines this node should manage.
//!
//! ```ignore
//! let topology_tx = spawn_machine_actor_manager(
//!     local_peer_id,
//!     sharding_ref,
//!     etcd_client,
//! );
//!
//! // Notify of topology changes
//! topology_tx.send(()).ok();
//! ```

mod actor;
mod manager;
mod messages;

pub use actor::{MachineActor, MachineSpec};
pub use manager::{spawn_machine_actor_manager, MachineActorManager};
pub use messages::{
    CheckOwnership, DeprovisionMachine, DeprovisionResult, GetMachineStatus, MachineState,
    MachineStatus, OwnershipCheckResult, ProvisionConfig, ProvisionMachine, ProvisionResult,
    UpdateAck, UpdateMachineSpec,
};
