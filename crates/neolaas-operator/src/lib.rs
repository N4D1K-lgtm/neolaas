//! Neolaas Operator Library
//!
//! Kubernetes operator for managing static hardware inventory configuration.
//! Defines CRDs for Machines, Switches, and VLANs that are synced to etcd
//! for the distributed work distribution system.
//!
//! This operator manages **static configuration only** - runtime state like
//! booking status, power state, etc. is managed by neolaas-server.

pub mod controllers;
pub mod crds;
pub mod etcd;

pub use crds::{Machine, MachineSpec, Switch, SwitchSpec, Vlan, VlanSpec};
