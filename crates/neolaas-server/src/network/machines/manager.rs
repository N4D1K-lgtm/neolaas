//! MachineActorManagerActor
//!
//! Coordinates MachineActors across the cluster using sharding.
//! Watches etcd for machine changes and spawns/stops actors based on ownership.
//! Implements supervisor tree patterns for fault-tolerant actor management.

use super::actor::MachineActor;
use super::messages::{
    GetAllMachineStatuses, GetMachineStatus, MachineStatus, SyncAllMachines, SyncResult,
    UpdateMachineSpec,
};
use crate::network::sharding::{LookupMachine, ShardingCoordinator};
use crate::observability::{events, metrics, RestartPolicy, RestartTracker};
use etcd_client::{Client, EventType, GetOptions, WatchOptions};
use kameo::prelude::*;
use libp2p::PeerId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Prefix for machine keys in etcd (matches operator's key structure)
const MACHINES_PREFIX: &str = "/neolaas/machines/";

/// Internal message to trigger topology rebalance
#[derive(Debug, Clone)]
pub struct RebalanceActors;

/// Internal message for etcd watch events
#[derive(Debug, Clone)]
pub struct EtcdMachineEvent {
    pub machine_id: String,
    pub event_type: EtcdEventType,
    pub spec_json: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EtcdEventType {
    Put,
    Delete,
}

/// Manages MachineActors for this node as a supervised actor.
///
/// Responsibilities:
/// - Watch etcd for machine changes
/// - Query ShardingCoordinator to determine ownership
/// - Spawn MachineActors for locally-owned machines
/// - Stop actors when ownership transfers away
/// - Handle topology changes (rebalancing)
/// - Supervise child actors and handle panics
#[derive(Actor)]
pub struct MachineActorManagerActor {
    /// Local peer ID
    local_peer_id: PeerId,
    /// Node ID for event context
    node_id: String,
    /// Reference to the ShardingCoordinator actor
    sharding_ref: ActorRef<ShardingCoordinator>,
    /// Active machine actors keyed by machine_id
    actors: HashMap<String, ActorRef<MachineActor>>,
    /// Cached machine specs for rebalancing
    machine_specs: HashMap<String, String>,
    /// etcd client for watching machines
    etcd_client: Arc<RwLock<Client>>,
    /// Restart trackers for each machine actor
    restart_trackers: HashMap<String, RestartTracker>,
    /// Default restart policy for child actors
    restart_policy: RestartPolicy,
}

impl MachineActorManagerActor {
    /// Create a new MachineActorManagerActor.
    pub fn new(
        local_peer_id: PeerId,
        node_id: String,
        sharding_ref: ActorRef<ShardingCoordinator>,
        etcd_client: Arc<RwLock<Client>>,
    ) -> Self {
        Self {
            local_peer_id,
            node_id,
            sharding_ref,
            actors: HashMap::new(),
            machine_specs: HashMap::new(),
            etcd_client,
            restart_trackers: HashMap::new(),
            restart_policy: RestartPolicy::default(),
        }
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

    /// Handle a machine PUT event (create or update).
    async fn handle_machine_put(&mut self, machine_id: String, spec_json: String) {
        debug!(machine_id = %machine_id, "Processing machine PUT");

        // Cache the spec for potential rebalancing
        self.machine_specs
            .insert(machine_id.clone(), spec_json.clone());

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
                let previous_owner = self.local_peer_id.to_base58();
                let new_owner = lookup_result
                    .owner_peer
                    .map(|p| p.to_base58())
                    .unwrap_or_else(|| "unknown".to_string());

                debug!(
                    machine_id = %machine_id,
                    new_owner = &new_owner[46..],
                    "Ownership transferred, stopping local actor"
                );

                events::ownership_transferred(&machine_id, &previous_owner[46..], &new_owner[46..]);
                self.stop_machine_actor(&machine_id, "ownership_transfer")
                    .await;
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
            self.stop_machine_actor(machine_id, "machine_deleted").await;
        }
    }

    /// Spawn a new MachineActor for a locally-owned machine.
    async fn spawn_machine_actor(&mut self, machine_id: String, spec_json: String) {
        match MachineActor::with_node_id(machine_id.clone(), &spec_json, self.node_id.clone()) {
            Ok(actor) => {
                let actor_ref = MachineActor::spawn(actor);

                info!(
                    machine_id = %machine_id,
                    local_peer = &self.local_peer_id.to_base58()[46..],
                    "Spawned MachineActor"
                );

                // Record metrics and emit event
                metrics::record_actor_spawned("MachineActor");
                events::actor_spawned("MachineActor", &machine_id, &self.node_id);

                // Initialize restart tracker for this actor
                self.restart_trackers.insert(
                    machine_id.clone(),
                    RestartTracker::new(self.restart_policy.clone()),
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
    async fn stop_machine_actor(&mut self, machine_id: &str, reason: &str) {
        if let Some(actor_ref) = self.actors.remove(machine_id) {
            info!(machine_id = %machine_id, reason = %reason, "Stopping MachineActor");

            // Record metrics and emit event
            metrics::record_actor_stopped("MachineActor", reason);
            events::actor_stopped("MachineActor", machine_id, reason, &self.node_id);

            // Remove restart tracker
            self.restart_trackers.remove(machine_id);

            // Kill the actor
            actor_ref.stop_gracefully().await.ok();
        }
    }

    /// Handle a child actor panic/failure.
    /// TODO: Wire this up to kameo's actor lifecycle hooks when available.
    #[allow(dead_code)]
    async fn handle_actor_failure(&mut self, machine_id: &str, error: &str) {
        // Check restart policy
        let should_restart = if let Some(tracker) = self.restart_trackers.get_mut(machine_id) {
            tracker.record_restart().is_some()
        } else {
            false
        };

        // Emit panic event
        events::actor_panicked("MachineActor", machine_id, error, &self.node_id, should_restart);

        if should_restart {
            // Get the cached spec and restart
            if let Some(spec_json) = self.machine_specs.get(machine_id).cloned() {
                info!(machine_id = %machine_id, "Restarting MachineActor after panic");
                self.actors.remove(machine_id);
                self.spawn_machine_actor(machine_id.to_string(), spec_json)
                    .await;
            }
        } else {
            error!(
                machine_id = %machine_id,
                "Restart limit exceeded for MachineActor, not restarting"
            );
            self.actors.remove(machine_id);
            self.restart_trackers.remove(machine_id);
            metrics::record_actor_stopped("MachineActor", "restart_limit_exceeded");
        }
    }

    /// Rebalance actors after topology change.
    async fn rebalance_actors(&mut self) {
        info!(
            current_actors = self.actors.len(),
            cached_machines = self.machine_specs.len(),
            "Starting actor rebalance"
        );

        // Collect machine IDs to check (both current actors and cached specs)
        let machine_ids: Vec<String> = self.machine_specs.keys().cloned().collect();

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
            self.stop_machine_actor(&machine_id, "topology_rebalance")
                .await;
        }

        for (machine_id, spec_json) in to_spawn {
            debug!(machine_id = %machine_id, "Spawning actor due to rebalance");
            self.spawn_machine_actor(machine_id, spec_json).await;
        }

        info!(final_actors = self.actors.len(), "Rebalance complete");
    }

    /// Get count of locally-managed actors.
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }
}

// Message handler for GetAllMachineStatuses
impl Message<GetAllMachineStatuses> for MachineActorManagerActor {
    type Reply = Vec<MachineStatus>;

    #[tracing::instrument(skip(self, _ctx), name = "GetAllMachineStatuses")]
    async fn handle(
        &mut self,
        _msg: GetAllMachineStatuses,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
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
}

// Message handler for rebalancing
impl Message<RebalanceActors> for MachineActorManagerActor {
    type Reply = ();

    #[tracing::instrument(skip(self, _ctx), name = "RebalanceActors")]
    async fn handle(
        &mut self,
        _msg: RebalanceActors,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.rebalance_actors().await;
    }
}

// Message handler for etcd events
impl Message<EtcdMachineEvent> for MachineActorManagerActor {
    type Reply = ();

    #[tracing::instrument(skip(self, _ctx), name = "EtcdMachineEvent", fields(machine_id = %msg.machine_id))]
    async fn handle(
        &mut self,
        msg: EtcdMachineEvent,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match msg.event_type {
            EtcdEventType::Put => {
                if let Some(spec_json) = msg.spec_json {
                    self.handle_machine_put(msg.machine_id, spec_json).await;
                }
            }
            EtcdEventType::Delete => {
                self.handle_machine_delete(&msg.machine_id).await;
            }
        }
    }
}

// Message handler for initial sync (after convergence)
impl Message<SyncAllMachines> for MachineActorManagerActor {
    type Reply = SyncResult;

    #[tracing::instrument(skip(self, _ctx), name = "SyncAllMachines")]
    async fn handle(
        &mut self,
        _msg: SyncAllMachines,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match self.sync_all_machines().await {
            Ok(()) => SyncResult {
                total_machines: self.machine_specs.len(),
                local_actors: self.actors.len(),
            },
            Err(e) => {
                error!(error = %e, "Initial machine sync failed");
                SyncResult {
                    total_machines: 0,
                    local_actors: 0,
                }
            }
        }
    }
}

/// Spawn the MachineActorManagerActor and start the etcd watch loop.
///
/// Returns the actor reference that can be used to query machine states.
///
/// Note: Initial machine sync is deferred until after discovery convergence completes,
/// ensuring all nodes have the same topology view before spawning actors.
pub async fn spawn_machine_actor_manager_actor(
    local_peer_id: PeerId,
    node_id: String,
    sharding_ref: ActorRef<ShardingCoordinator>,
    etcd_client: Arc<RwLock<Client>>,
    mut topology_rx: mpsc::UnboundedReceiver<()>,
    convergence_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<ActorRef<MachineActorManagerActor>, anyhow::Error> {
    let manager = MachineActorManagerActor::new(
        local_peer_id,
        node_id,
        sharding_ref,
        etcd_client.clone(),
    );

    // Spawn the manager actor first (before sync)
    let manager_ref = MachineActorManagerActor::spawn(manager);
    let manager_ref_for_topology = manager_ref.clone();
    let manager_ref_for_watch = manager_ref.clone();
    let manager_ref_for_sync = manager_ref.clone();

    // Spawn topology change listener
    tokio::spawn(async move {
        while topology_rx.recv().await.is_some() {
            info!("Received topology change notification, rebalancing actors");
            if let Err(e) = manager_ref_for_topology.ask(RebalanceActors).send().await {
                warn!(error = %e, "Failed to send rebalance message to manager");
            }
        }
        debug!("Topology receiver closed");
    });

    // Wait for convergence before initial sync (ensures correct topology)
    tokio::spawn(async move {
        match convergence_rx.await {
            Ok(()) => {
                info!("Convergence complete, starting initial machine sync");
                if let Err(e) = manager_ref_for_sync.ask(SyncAllMachines).send().await {
                    error!(error = %e, "Failed to trigger initial machine sync");
                }
            }
            Err(_) => {
                warn!("Convergence channel closed before signal received");
            }
        }
    });

    // Spawn etcd watch loop
    let etcd_client_for_watch = etcd_client.clone();
    tokio::spawn(async move {
        loop {
            // Create watch on machines prefix
            let watch_result = {
                let mut client = etcd_client_for_watch.write().await;
                client
                    .watch(MACHINES_PREFIX, Some(WatchOptions::new().with_prefix()))
                    .await
            };

            let mut watch_stream = match watch_result {
                Ok(stream) => stream,
                Err(e) => {
                    error!(error = %e, "Failed to start etcd watch, retrying...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            info!("Started watching {} for machine changes", MACHINES_PREFIX);

            loop {
                match watch_stream.message().await {
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
                                let machine_id = key
                                    .strip_prefix(MACHINES_PREFIX)
                                    .unwrap_or(key)
                                    .to_string();

                                let etcd_event = match event.event_type() {
                                    EventType::Put => EtcdMachineEvent {
                                        machine_id,
                                        event_type: EtcdEventType::Put,
                                        spec_json: kv.value_str().ok().map(|s| s.to_string()),
                                    },
                                    EventType::Delete => EtcdMachineEvent {
                                        machine_id,
                                        event_type: EtcdEventType::Delete,
                                        spec_json: None,
                                    },
                                };

                                if let Err(e) =
                                    manager_ref_for_watch.tell(etcd_event).send().await
                                {
                                    warn!(error = %e, "Failed to send etcd event to manager");
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

            // Brief delay before reconnecting
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    });

    Ok(manager_ref)
}

