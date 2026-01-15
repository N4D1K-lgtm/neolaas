//! Custom Resource Definitions
//!
//! Kubernetes CRDs for hardware inventory management.

pub mod machine;
pub mod switch;
pub mod vlan;

pub use machine::{Machine, MachineSpec};
pub use switch::{Switch, SwitchSpec};
pub use vlan::{Vlan, VlanSpec};
