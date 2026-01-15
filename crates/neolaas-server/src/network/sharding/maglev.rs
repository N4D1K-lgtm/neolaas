//! Maglev Consistent Hashing Implementation
//!
//! Based on Google's Maglev paper for consistent hashing with
//! minimal disruption during topology changes.
//!
//! Reference: https://research.google/pubs/pub44824/

use libp2p::PeerId;
use std::hash::{Hash, Hasher};

/// Default Maglev table size (must be prime for good distribution)
const DEFAULT_TABLE_SIZE: usize = 65537;

/// Maglev consistent hash table
///
/// Provides O(1) lookups and minimal key redistribution when
/// the node set changes.
#[derive(Debug, Clone)]
pub struct MaglevHasher {
    /// Lookup table: slot index → node index
    lookup: Vec<usize>,
    /// Ordered list of nodes (order matters for determinism)
    nodes: Vec<PeerId>,
    /// Table size (must be prime)
    table_size: usize,
}

impl MaglevHasher {
    /// Create a new Maglev hasher with the given nodes.
    ///
    /// Nodes are sorted by their base58 representation to ensure
    /// deterministic ordering across all cluster members.
    pub fn new(mut nodes: Vec<PeerId>) -> Self {
        // Sort nodes for deterministic ordering across all peers
        nodes.sort_by(|a, b| a.to_base58().cmp(&b.to_base58()));

        let table_size = DEFAULT_TABLE_SIZE;
        let lookup = Self::build_lookup_table(&nodes, table_size);

        Self {
            lookup,
            nodes,
            table_size,
        }
    }

    /// Look up which node owns the given key.
    ///
    /// Returns None if the node set is empty.
    pub fn lookup(&self, key: &str) -> Option<&PeerId> {
        if self.nodes.is_empty() {
            return None;
        }

        let hash = Self::hash_key(key);
        let slot = (hash as usize) % self.table_size;
        let node_idx = self.lookup[slot];

        self.nodes.get(node_idx)
    }

    /// Check if a key is owned by the local node.
    pub fn is_local(&self, key: &str, local_peer: &PeerId) -> bool {
        self.lookup(key) == Some(local_peer)
    }

    /// Rebuild the lookup table with a new set of nodes.
    ///
    /// Call this when the cluster topology changes.
    pub fn rebuild(&mut self, mut nodes: Vec<PeerId>) {
        // Sort for deterministic ordering
        nodes.sort_by(|a, b| a.to_base58().cmp(&b.to_base58()));

        self.lookup = Self::build_lookup_table(&nodes, self.table_size);
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

    /// Build the Maglev lookup table using the permutation algorithm.
    ///
    /// Each node gets a permutation sequence based on (offset, skip).
    /// We fill the table by having each node claim slots in turn.
    fn build_lookup_table(nodes: &[PeerId], table_size: usize) -> Vec<usize> {
        if nodes.is_empty() {
            return vec![0; table_size];
        }

        let n = nodes.len();

        // Calculate offset and skip for each node
        // offset = h1(node) mod M
        // skip = h2(node) mod (M-1) + 1
        let permutations: Vec<(usize, usize)> = nodes
            .iter()
            .map(|node| {
                let node_str = node.to_base58();
                let h1 = Self::hash_key(&node_str) as usize;
                let h2 = Self::hash_key(&format!("{node_str}:skip")) as usize;
                let offset = h1 % table_size;
                let skip = (h2 % (table_size - 1)) + 1;
                (offset, skip)
            })
            .collect();

        // Build lookup table using Maglev algorithm
        let mut lookup = vec![usize::MAX; table_size];
        let mut next = vec![0usize; n]; // Track position in each node's permutation
        let mut filled = 0;

        // Round-robin through nodes, each claiming their next available slot
        while filled < table_size {
            for (i, (offset, skip)) in permutations.iter().enumerate() {
                // Find next unclaimed slot for this node
                let mut c = (offset + next[i] * skip) % table_size;

                while lookup[c] != usize::MAX {
                    next[i] += 1;
                    c = (offset + next[i] * skip) % table_size;
                }

                // Claim this slot
                lookup[c] = i;
                next[i] += 1;
                filled += 1;

                if filled >= table_size {
                    break;
                }
            }
        }

        lookup
    }

    /// Hash a key using SipHash (via DefaultHasher).
    fn hash_key(key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    fn generate_peer_ids(count: usize) -> Vec<PeerId> {
        (0..count)
            .map(|_| {
                let keypair = Keypair::generate_ed25519();
                PeerId::from(keypair.public())
            })
            .collect()
    }

    #[test]
    fn test_empty_nodes_returns_none() {
        let hasher = MaglevHasher::new(vec![]);
        assert!(hasher.lookup("any-key").is_none());
        assert_eq!(hasher.node_count(), 0);
    }

    #[test]
    fn test_single_node_owns_all_keys() {
        let peers = generate_peer_ids(1);
        let hasher = MaglevHasher::new(peers.clone());

        // All keys should map to the single node
        for i in 0..100 {
            let key = format!("machine-{i}");
            assert_eq!(hasher.lookup(&key), Some(&peers[0]));
        }
    }

    #[test]
    fn test_deterministic_hashing() {
        let peers = generate_peer_ids(5);

        let hasher1 = MaglevHasher::new(peers.clone());
        let hasher2 = MaglevHasher::new(peers.clone());

        // Same input should produce same output
        for i in 0..100 {
            let key = format!("machine-{i}");
            assert_eq!(hasher1.lookup(&key), hasher2.lookup(&key));
        }
    }

    #[test]
    fn test_distribution_is_roughly_even() {
        let peers = generate_peer_ids(4);
        let hasher = MaglevHasher::new(peers.clone());

        let mut counts = vec![0usize; 4];
        let num_keys = 10000;

        for i in 0..num_keys {
            let key = format!("machine-{i}");
            if let Some(owner) = hasher.lookup(&key) {
                if let Some(idx) = peers.iter().position(|p| p == owner) {
                    counts[idx] += 1;
                }
            }
        }

        // Each node should get roughly 25% (allow 15% deviation)
        let expected = num_keys / 4;
        let tolerance = expected * 15 / 100;

        for (i, count) in counts.iter().enumerate() {
            assert!(
                (*count as i64 - expected as i64).unsigned_abs() < tolerance as u64,
                "Node {i} got {count} keys, expected ~{expected} (tolerance {tolerance})"
            );
        }
    }

    #[test]
    fn test_minimal_disruption_on_node_removal() {
        let peers = generate_peer_ids(4);
        let hasher = MaglevHasher::new(peers.clone());

        // Record initial assignments
        let mut initial_assignments = Vec::new();
        for i in 0..1000 {
            let key = format!("machine-{i}");
            initial_assignments.push((key.clone(), hasher.lookup(&key).cloned()));
        }

        // Remove one node
        let remaining: Vec<_> = peers.iter().skip(1).cloned().collect();
        let removed_peer = &peers[0];
        let new_hasher = MaglevHasher::new(remaining.clone());

        // Count how many keys changed owner
        let mut moved = 0;
        let mut stayed = 0;

        for (key, old_owner) in &initial_assignments {
            let new_owner = new_hasher.lookup(key);

            match old_owner {
                Some(old) if old == removed_peer => {
                    // Keys from removed node must move
                    moved += 1;
                }
                Some(old) => {
                    // Keys from remaining nodes should mostly stay
                    if new_owner != Some(old) {
                        moved += 1;
                    } else {
                        stayed += 1;
                    }
                }
                None => {}
            }
        }

        // Ideally only 1/n keys should move (those owned by removed node)
        // With 4 nodes, ~25% should move. Allow some tolerance.
        let total = moved + stayed;
        let move_percentage = (moved as f64 / total as f64) * 100.0;

        // Should be roughly 25% (1/4) moved, allow up to 35%
        assert!(
            move_percentage < 35.0,
            "Too many keys moved: {move_percentage:.1}% (expected ~25%)"
        );
    }

    #[test]
    fn test_is_local() {
        let peers = generate_peer_ids(3);
        let hasher = MaglevHasher::new(peers.clone());

        // Find a key owned by each peer
        for peer in &peers {
            // Search for a key this peer owns
            let mut found = false;
            for i in 0..1000 {
                let key = format!("test-{i}");
                if hasher.is_local(&key, peer) {
                    found = true;
                    break;
                }
            }
            assert!(found, "Could not find any key owned by peer {peer}");
        }
    }

    #[test]
    fn test_rebuild_maintains_consistency() {
        let peers = generate_peer_ids(3);
        let mut hasher = MaglevHasher::new(peers.clone());

        // Get initial assignment
        let key = "test-key";
        let initial_owner = hasher.lookup(key).cloned();

        // Rebuild with same peers
        hasher.rebuild(peers.clone());
        let after_rebuild = hasher.lookup(key).cloned();

        // Should be the same
        assert_eq!(initial_owner, after_rebuild);
    }
}
