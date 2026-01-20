//! Rendezvous Hashing (Highest Random Weight)
//!
//! Simple, production-quality consistent hashing with perfect distribution.
//! O(n) lookup where n = number of nodes. Ideal for clusters of 3-200 nodes.
//!
//! Properties:
//! - Perfectly even distribution regardless of cluster size
//! - Minimal disruption: only keys from added/removed node move
//! - Deterministic: same key always maps to same node given same topology
//! - Simple: ~30 lines of core logic

use libp2p::PeerId;
use std::hash::{Hash, Hasher};

/// Rendezvous consistent hasher
#[derive(Debug, Clone)]
pub struct RendezvousHasher {
    /// Nodes in the cluster (sorted for determinism)
    nodes: Vec<PeerId>,
}

impl RendezvousHasher {
    /// Create a new hasher with the given nodes.
    pub fn new(mut nodes: Vec<PeerId>) -> Self {
        nodes.sort_by(|a, b| a.to_base58().cmp(&b.to_base58()));
        Self { nodes }
    }

    /// Look up which node owns the given key.
    ///
    /// Returns the node with the highest hash(key, node) score.
    pub fn lookup(&self, key: &str) -> Option<&PeerId> {
        if self.nodes.is_empty() {
            return None;
        }

        self.nodes
            .iter()
            .max_by_key(|node| Self::score(key, node))
    }

    /// Check if a key is owned by the local node.
    pub fn is_local(&self, key: &str, local_peer: &PeerId) -> bool {
        self.lookup(key) == Some(local_peer)
    }

    /// Rebuild with a new set of nodes.
    pub fn rebuild(&mut self, mut nodes: Vec<PeerId>) {
        nodes.sort_by(|a, b| a.to_base58().cmp(&b.to_base58()));
        self.nodes = nodes;
    }

    /// Get current node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the list of nodes.
    pub fn nodes(&self) -> &[PeerId] {
        &self.nodes
    }

    /// Compute the score for a (key, node) pair.
    /// Higher score wins ownership.
    fn score(key: &str, node: &PeerId) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        node.to_base58().hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    fn generate_peer_ids(count: usize) -> Vec<PeerId> {
        (0..count)
            .map(|_| PeerId::from(Keypair::generate_ed25519().public()))
            .collect()
    }

    #[test]
    fn test_empty_returns_none() {
        let hasher = RendezvousHasher::new(vec![]);
        assert!(hasher.lookup("any-key").is_none());
    }

    #[test]
    fn test_single_node_owns_all() {
        let peers = generate_peer_ids(1);
        let hasher = RendezvousHasher::new(peers.clone());

        for i in 0..100 {
            assert_eq!(hasher.lookup(&format!("machine-{i}")), Some(&peers[0]));
        }
    }

    #[test]
    fn test_deterministic() {
        let peers = generate_peer_ids(5);
        let h1 = RendezvousHasher::new(peers.clone());
        let h2 = RendezvousHasher::new(peers);

        for i in 0..100 {
            let key = format!("machine-{i}");
            assert_eq!(h1.lookup(&key), h2.lookup(&key));
        }
    }

    #[test]
    fn test_even_distribution() {
        let peers = generate_peer_ids(4);
        let hasher = RendezvousHasher::new(peers.clone());

        let mut counts = vec![0usize; 4];
        let num_keys = 10000;

        for i in 0..num_keys {
            if let Some(owner) = hasher.lookup(&format!("machine-{i}")) {
                if let Some(idx) = peers.iter().position(|p| p == owner) {
                    counts[idx] += 1;
                }
            }
        }

        // Each node should get ~25% (allow 10% deviation)
        let expected = num_keys / 4;
        let tolerance = expected / 10;

        for (i, &count) in counts.iter().enumerate() {
            let diff = (count as i64 - expected as i64).unsigned_abs() as usize;
            assert!(
                diff < tolerance,
                "Node {i}: got {count}, expected ~{expected} (±{tolerance})"
            );
        }
    }

    #[test]
    fn test_minimal_disruption() {
        let peers = generate_peer_ids(4);
        let hasher = RendezvousHasher::new(peers.clone());

        // Record initial assignments
        let assignments: Vec<_> = (0..1000)
            .map(|i| {
                let key = format!("machine-{i}");
                (key.clone(), hasher.lookup(&key).cloned())
            })
            .collect();

        // Remove first node
        let remaining: Vec<_> = peers.iter().skip(1).cloned().collect();
        let new_hasher = RendezvousHasher::new(remaining);

        // Count moves
        let mut moved = 0;
        for (key, old_owner) in &assignments {
            let new_owner = new_hasher.lookup(key);
            if old_owner.as_ref() != new_owner {
                moved += 1;
            }
        }

        // Only ~25% should move (keys from removed node)
        let move_pct = (moved as f64 / 1000.0) * 100.0;
        assert!(
            move_pct < 35.0,
            "Too many keys moved: {move_pct:.1}% (expected ~25%)"
        );
    }

    #[test]
    fn test_rebuild_consistency() {
        let peers = generate_peer_ids(3);
        let mut hasher = RendezvousHasher::new(peers.clone());

        let key = "test-key";
        let before = hasher.lookup(key).cloned();

        hasher.rebuild(peers);
        let after = hasher.lookup(key).cloned();

        assert_eq!(before, after);
    }
}
