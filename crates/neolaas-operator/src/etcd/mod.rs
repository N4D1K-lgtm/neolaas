//! etcd synchronization utilities
//!
//! Syncs CRD data to etcd for neolaas-server to consume.

mod sync;

pub use sync::EtcdSync;
