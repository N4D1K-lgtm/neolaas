//! MachineActorManager
//!
//! Coordinates MachineActors across the cluster using sharding.
//! Watches etcd for machine changes and spawns/stops actors based on ownership.

use super::actor::MachineActor;
use super::messages::{GetMachineStatus, MachineStatus, UpdateMachineSpec};
use crate::network::sharding::{LookupMachine, ShardingCoordinator};
use etcd_client::{Client, EventType, GetOptions, WatchOptions};
use kameo::prelude::*;
use libp2p::PeerId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Prefix for machine keys in etcd (matches operator's key structure)
const MACHINES_PREFIX: &str = "/neolaas/machines/";

/// Manages MachineActors for this node.
///
/// Responsibilities:
/// - Watch etcd for machine changes
/// - Query ShardingCoordinator to determine ownership
/// - Spawn MachineActors for locally-owned machines
/// - Stop actors when ownership transfers away
/// - Handle topology changes (rebalancing)
pub struct MachineActorManager {
    /// Local peer ID
    local_peer_id: PeerId,
    /// Reference to the ShardingCoordinator actor
    sharding_ref: ActorRef<ShardingCoordinator>,
    /// Active machine actors keyed by machine_id
    actors: HashMap<String, ActorRef<MachineActor>>,
    /// Cached machine specs for rebalancing
    machine_specs: HashMap<String, String>,
    /// etcd client for watching machines
    etcd_client: Arc<RwLock<Client>>,
    /// Channel to receive topology change notifications
    topology_rx: mpsc::UnboundedReceiver<()>,
}

impl MachineActorManager {
    /// Create a new MachineActorManager.
    pub fn new(
        local_peer_id: PeerId,
        sharding_ref: ActorRef<ShardingCoordinator>,
        etcd_client: Arc<RwLock<Client>>,
        topology_rx: mpsc::UnboundedReceiver<()>,
    ) -> Self {
        Self {
            local_peer_id,
            sharding_ref,
            actors: HashMap::new(),
            machine_specs: HashMap::new(),
            etcd_client,
            topology_rx,
        }
    }

    /// Start the manager - runs the main event loop.
    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        info!(
            local_peer = %self.local_peer_id.to_base58()[46..],
            "Starting MachineActorManager"
        );

        // Initial sync: fetch all existing machines
        self.sync_all_machines().await?;

        // Start watching for changes
        self.watch_loop().await
    }

    /// Fetch all existing machines from etcd and spawn actors for locally-owned ones.
    async fn sync_all_machines(&mut self) -> Result<(), anyhow::Error> {
        info!("Syncing all machines from etcd");

        // Collect machine data while holding the lock
        let machines: Vec<(String, String)> = {
            let mut client = self.etcd_client.write().await;
            let resp = client
                .get(MACHINES_PREFIX, Some(GetOptions::new().with_prefix()))
                .await?;

            let count = resp.kvs().len();
            debug!(count = count, "Found machines in etcd");

            let mut machines = Vec::with_capacity(count);
            for kv in resp.kvs() {
                let key = kv.key_str()?;
                let machine_id = key.strip_prefix(MACHINES_PREFIX).unwrap_or(key);
                let spec_json = kv.value_str()?;
                machines.push((machine_id.to_string(), spec_json.to_string()));
            }
            machines
        };
        // Lock is dropped here

        let count = machines.len();

        // Process machines without holding the lock
        for (machine_id, spec_json) in machines {
            self.handle_machine_put(machine_id, spec_json).await;
        }

        info!(
            total_machines = count,
            local_actors = self.actors.len(),
            "Initial machine sync complete"
        );

        Ok(())
    }

    /// Main watch loop - watches etcd for machine changes and topology updates.
    async fn watch_loop(&mut self) -> Result<(), anyhow::Error> {
        loop {
            // Create watch on machines prefix
            let (mut watcher, mut watch_stream) = {
                let mut client = self.etcd_client.write().await;
                client
                    .watch(MACHINES_PREFIX, Some(WatchOptions::new().with_prefix()))
                    .await?
            };

            info!("Started watching {} for machine changes", MACHINES_PREFIX);

            loop {
                tokio::select! {
                    // Handle etcd watch events
                    watch_result = watch_stream.message() => {
                        match watch_result {
                            Ok(Some(resp)) => {
                                for event in resp.events() {
                                    if let Some(kv) = event.kv() {
                                        let key = match kv.key_str() {
                                            Ok(k) => k,
                                            Err(e) => {
                                                warn!(error = %e, "Invalid key encoding");
                                                continue;
                                            }
                                        };
                                        let machine_id = key.strip_prefix(MACHINES_PREFIX)
                                            .unwrap_or(key)
                                            .to_string();

                                        match event.event_type() {
                                            EventType::Put => {
                                                if let Ok(spec_json) = kv.value_str() {
                                                    self.handle_machine_put(machine_id, spec_json.to_string()).await;
                                                }
                                            }
                                            EventType::Delete => {
                                                self.handle_machine_delete(&machine_id).await;
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                warn!("Watch stream ended, reconnecting...");
                                break;
                            }
                            Err(e) => {
                                error!(error = %e, "Watch error, reconnecting...");
                                break;
                            }
                        }
                    }

                    // Handle topology change notifications
                    Some(()) = self.topology_rx.recv() => {
                        info!("Received topology change notification, rebalancing actors");
                        self.rebalance_actors().await;
                    }
                }
            }

            // Cancel the watcher before reconnecting
            let _ = watcher.cancel().await;

            // Brief delay before reconnecting
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    /// Handle a machine PUT event (create or update).
    async fn handle_machine_put(&mut self, machine_id: String, spec_json: String) {
        debug!(machine_id = %machine_id, "Processing machine PUT");

        // Cache the spec for potential rebalancing
        self.machine_specs.insert(machine_id.clone(), spec_json.clone());

        // Query sharding to determine ownership
        let lookup_result = match self
            .sharding_ref
            .ask(LookupMachine {
                machine_id: machine_id.clone(),
            })
            .send()
            .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!(
                    machine_id = %machine_id,
                    error = %e,
                    "Failed to query sharding coordinator"
                );
                return;
            }
        };

        if lookup_result.is_local {
            // We own this machine
            if let Some(actor_ref) = self.actors.get(&machine_id) {
                // Actor exists, send update
                debug!(machine_id = %machine_id, "Updating existing actor");
                if let Err(e) = actor_ref
                    .ask(UpdateMachineSpec { spec_json })
                    .send()
                    .await
                {
                    warn!(
                        machine_id = %machine_id,
                        error = %e,
                        "Failed to update machine actor"
                    );
                }
            } else {
                // Spawn new actor
                self.spawn_machine_actor(machine_id, spec_json).await;
            }
        } else {
            // We don't own this machine
            if self.actors.contains_key(&machine_id) {
                // We have an actor but shouldn't - ownership transferred
                debug!(
                    machine_id = %machine_id,
                    new_owner = ?lookup_result.owner_peer.map(|p| p.to_base58()[46..].to_string()),
                    "Ownership transferred, stopping local actor"
                );
                self.stop_machine_actor(&machine_id).await;
            }
        }
    }

    /// Handle a machine DELETE event.
    async fn handle_machine_delete(&mut self, machine_id: &str) {
        debug!(machine_id = %machine_id, "Processing machine DELETE");

        // Remove from cache
        self.machine_specs.remove(machine_id);

        // Stop actor if we have one
        if self.actors.contains_key(machine_id) {
            self.stop_machine_actor(machine_id).await;
        }
    }

    /// Spawn a new MachineActor for a locally-owned machine.
    async fn spawn_machine_actor(&mut self, machine_id: String, spec_json: String) {
        match MachineActor::new(machine_id.clone(), &spec_json) {
            Ok(actor) => {
                let actor_ref = MachineActor::spawn(actor);
                info!(
                    machine_id = %machine_id,
                    local_peer = %self.local_peer_id.to_base58()[46..],
                    "Spawned MachineActor"
                );
                self.actors.insert(machine_id, actor_ref);
            }
            Err(e) => {
                error!(
                    machine_id = %machine_id,
                    error = %e,
                    "Failed to create MachineActor"
                );
            }
        }
    }

    /// Stop a MachineActor.
    async fn stop_machine_actor(&mut self, machine_id: &str) {
        if let Some(actor_ref) = self.actors.remove(machine_id) {
            info!(machine_id = %machine_id, "Stopping MachineActor");
            // Kill the actor
            actor_ref.stop_gracefully().await.ok();
        }
    }

    /// Rebalance actors after topology change.
    ///
    /// Checks ownership of all current actors and spawns/stops as needed.
    async fn rebalance_actors(&mut self) {
        info!(
            current_actors = self.actors.len(),
            cached_machines = self.machine_specs.len(),
            "Starting actor rebalance"
        );

        // Collect machine IDs to check (both current actors and cached specs)
        let machine_ids: Vec<String> = self
            .machine_specs
            .keys()
            .cloned()
            .collect();

        let mut to_stop = Vec::new();
        let mut to_spawn = Vec::new();

        for machine_id in machine_ids {
            let lookup_result = match self
                .sharding_ref
                .ask(LookupMachine {
                    machine_id: machine_id.clone(),
                })
                .send()
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    warn!(
                        machine_id = %machine_id,
                        error = %e,
                        "Failed to lookup machine during rebalance"
                    );
                    continue;
                }
            };

            let has_actor = self.actors.contains_key(&machine_id);

            if lookup_result.is_local && !has_actor {
                // Should own but don't have actor
                if let Some(spec_json) = self.machine_specs.get(&machine_id).cloned() {
                    to_spawn.push((machine_id, spec_json));
                }
            } else if !lookup_result.is_local && has_actor {
                // Shouldn't own but have actor
                to_stop.push(machine_id);
            }
        }

        // Apply changes
        for machine_id in to_stop {
            debug!(machine_id = %machine_id, "Stopping actor due to rebalance");
            self.stop_machine_actor(&machine_id).await;
        }

        for (machine_id, spec_json) in to_spawn {
            debug!(machine_id = %machine_id, "Spawning actor due to rebalance");
            self.spawn_machine_actor(machine_id, spec_json).await;
        }

        info!(
            final_actors = self.actors.len(),
            "Rebalance complete"
        );
    }

    /// Get status of all locally-managed machines.
    pub async fn get_all_statuses(&self) -> Vec<MachineStatus> {
        let mut statuses = Vec::new();
        for (machine_id, actor_ref) in &self.actors {
            match actor_ref.ask(GetMachineStatus).send().await {
                Ok(status) => statuses.push(status),
                Err(e) => {
                    warn!(
                        machine_id = %machine_id,
                        error = %e,
                        "Failed to get status from actor"
                    );
                }
            }
        }
        statuses
    }

    /// Get count of locally-managed actors.
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }
}

/// Spawn the MachineActorManager as a background task.
///
/// Returns a sender that can be used to notify of topology changes.
pub fn spawn_machine_actor_manager(
    local_peer_id: PeerId,
    sharding_ref: ActorRef<ShardingCoordinator>,
    etcd_client: Arc<RwLock<Client>>,
) -> mpsc::UnboundedSender<()> {
    let (topology_tx, topology_rx) = mpsc::unbounded_channel();

    let manager = MachineActorManager::new(local_peer_id, sharding_ref, etcd_client, topology_rx);

    tokio::spawn(async move {
        if let Err(e) = manager.run().await {
            error!(error = %e, "MachineActorManager failed");
        }
    });

    topology_tx
}
