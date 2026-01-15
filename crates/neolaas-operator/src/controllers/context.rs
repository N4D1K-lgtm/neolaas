//! Shared controller context

use crate::etcd::EtcdSync;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared context for all controllers.
pub struct Context {
    pub etcd: Arc<RwLock<EtcdSync>>,
}

impl Context {
    pub fn new(etcd: EtcdSync) -> Self {
        Self {
            etcd: Arc::new(RwLock::new(etcd)),
        }
    }
}
