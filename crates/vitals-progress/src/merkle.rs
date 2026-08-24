//! Incremental Merkle tree — the append-only structure the program keeps, and the proof
//! verification a claim has to survive.
//!
//! Deliberately the classic fixed-depth incremental tree rather than compressed state: the
//! program stores one root and `DEPTH` filled subtrees, so appending is O(depth) and costs no
//! account growth, and a proof is checkable with nothing but hashes. Production scale belongs on
//! Bubblegum — but Bubblegum needs an indexer to read, and a demo that cannot be verified on a
//! laptop with no network is not a demo of verifiability.

/// sha256, from whichever implementation is cheapest here.
///
/// On BPF the pure-Rust one costs more than an entire transaction's compute budget — the first
/// version of this file spent all 200,000 CUs and died — so on-chain builds use the syscall.
/// Both are sha256 and must agree; [`tests::both_backends_agree_with_the_standard`] pins that
/// against published vectors so a future swap cannot diverge quietly.
#[cfg(feature = "solana")]
fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    solana_program::hash::hashv(parts).to_bytes()
}

#[cfg(not(feature = "solana"))]
fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// 4,096 leaves per tree. Small enough that the account and the proofs stay cheap, large enough
/// that a cohort's practice runs fit in one. Rolling to a new tree per cohort-year is the
/// intended scaling story until compression lands.
pub const DEPTH: usize = 12;
pub const MAX_LEAVES: u64 = 1 << DEPTH;

/// Domain-separated so a leaf hash can never be mistaken for an interior node — the classic
/// second-preimage attack on naive Merkle trees.
pub fn hash_leaf(bytes: &[u8]) -> [u8; 32] {
    sha256(&[&[0x00], bytes])
}

pub fn hash_node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    sha256(&[&[0x01], left, right])
}

/// The empty-subtree hash at every level, built once.
///
/// Computing these on demand was O(DEPTH^2) hashes per append, which is a rounding error off
/// chain and a budget overrun on it. One table, DEPTH hashes.
pub fn zero_table() -> [[u8; 32]; DEPTH + 1] {
    let mut z = [[0u8; 32]; DEPTH + 1];
    z[0] = hash_leaf(b"vitals.empty");
    for level in 1..=DEPTH {
        z[level] = hash_node(&z[level - 1], &z[level - 1]);
    }
    z
}

/// The empty-subtree hash at one level. Prefer [`zero_table`] in a loop.
pub fn zero_hash(level: usize) -> [u8; 32] {
    zero_table()[level]
}

/// The append-only tree, as the program stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tree {
    pub root: [u8; 32],
    pub next_index: u64,
    pub filled: [[u8; 32]; DEPTH],
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Tree {
        let zeros = zero_table();
        let mut filled = [[0u8; 32]; DEPTH];
        filled.copy_from_slice(&zeros[..DEPTH]);
        // The root of an entirely empty tree is the empty-subtree hash at the top level.
        Tree { root: zeros[DEPTH], next_index: 0, filled }
    }

    /// Append a leaf. Returns its index, or `None` when the tree is full — a full tree is not an
    /// error to paper over, it is the signal to roll to the next one.
    pub fn append(&mut self, leaf: [u8; 32]) -> Option<u64> {
        if self.next_index >= MAX_LEAVES {
            return None;
        }
        let zeros = zero_table();
        let index = self.next_index;
        let mut current = leaf;
        let mut i = index;
        // clippy suggests iterating `filled` directly, which cannot be done here: the body reads
        // `zeros[level]` as well, and the index is what ties the two arrays to the same level of
        // the tree. Naming the level once is what makes that correspondence readable.
        #[allow(clippy::needless_range_loop)]
        for level in 0..DEPTH {
            if i.is_multiple_of(2) {
                // We are the left child; the right sibling is still empty, so remember our hash
                // for when it arrives.
                self.filled[level] = current;
                current = hash_node(&current, &zeros[level]);
            } else {
                current = hash_node(&self.filled[level], &current);
            }
            i /= 2;
        }
        self.root = current;
        self.next_index += 1;
        Some(index)
    }
}

/// Recompute a root from a leaf and its sibling path. Pure — the program compares the result
/// against the stored root and nothing else.
pub fn root_from_proof(leaf: [u8; 32], index: u64, path: &[[u8; 32]]) -> Option<[u8; 32]> {
    if path.len() != DEPTH || index >= MAX_LEAVES {
        return None;
    }
    let mut current = leaf;
    let mut i = index;
    for sibling in path.iter().take(DEPTH) {
        current = if i.is_multiple_of(2) { hash_node(&current, sibling) } else { hash_node(sibling, &current) };
        i /= 2;
    }
    Some(current)
}

/// Build the proof for a leaf already in the tree. Off-chain only — the program never needs it.
#[cfg(feature = "std")]
pub fn prove(leaves: &[[u8; 32]], index: u64) -> Option<[[u8; 32]; DEPTH]> {
    if index as usize >= leaves.len() {
        return None;
    }
    let zeros = zero_table();
    let mut level_nodes: Vec<[u8; 32]> = leaves.to_vec();
    let mut path = [[0u8; 32]; DEPTH];
    let mut i = index as usize;
    for (level, slot) in path.iter_mut().enumerate() {
        let sibling = i ^ 1;
        *slot = level_nodes.get(sibling).copied().unwrap_or(zeros[level]);
        let mut next = Vec::with_capacity(level_nodes.len().div_ceil(2));
        let mut k = 0;
        while k < level_nodes.len() {
            let l = level_nodes[k];
            let r = level_nodes.get(k + 1).copied().unwrap_or(zeros[level]);
            next.push(hash_node(&l, &r));
            k += 2;
        }
        level_nodes = next;
        i /= 2;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(n: u8) -> [u8; 32] {
        hash_leaf(&[n])
    }

    #[test]
    fn append_then_prove_every_leaf() {
        let mut tree = Tree::new();
        let mut leaves = Vec::new();
        for n in 0..9u8 {
            let l = leaf(n);
            leaves.push(l);
            tree.append(l).expect("room");
        }
        for (i, l) in leaves.iter().enumerate() {
            let path = prove(&leaves, i as u64).expect("proof");
            assert_eq!(
                root_from_proof(*l, i as u64, &path),
                Some(tree.root),
                "leaf {i} does not prove against the appended root"
            );
        }
    }

    #[test]
    fn a_leaf_that_was_never_appended_does_not_prove() {
        let mut tree = Tree::new();
        let leaves: Vec<_> = (0..4u8).map(leaf).collect();
        for l in &leaves {
            tree.append(*l);
        }
        let path = prove(&leaves, 1).expect("proof");
        assert_ne!(root_from_proof(leaf(200), 1, &path), Some(tree.root));
    }

    #[test]
    fn a_leaf_cannot_prove_at_the_wrong_index() {
        let mut tree = Tree::new();
        let leaves: Vec<_> = (0..4u8).map(leaf).collect();
        for l in &leaves {
            tree.append(*l);
        }
        let path = prove(&leaves, 1).expect("proof");
        assert_ne!(root_from_proof(leaves[1], 2, &path), Some(tree.root));
    }

    #[test]
    fn leaf_and_node_hashes_are_domain_separated() {
        // Without the 0x00/0x01 prefixes an attacker could present an interior node as a leaf.
        let a = hash_leaf(b"x");
        let b = hash_node(&[0u8; 32], &[0u8; 32]);
        assert_ne!(a, b);
    }

    #[test]
    fn both_backends_agree_with_the_standard() {
        // Published sha256 vectors. Whichever backend is compiled in must reproduce them, so the
        // on-chain syscall and the off-chain crate can never disagree about a leaf.
        assert_eq!(
            hex(&super::sha256(&[b"abc"])),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&super::sha256(&[b"", b""])),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // Split arguments must hash as one stream, or a leaf built in two pieces off chain would
        // not match the same leaf built in two pieces on chain.
        assert_eq!(super::sha256(&[b"ab", b"c"]), super::sha256(&[b"abc"]));
    }

    fn hex(b: &[u8; 32]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn tree_reports_full_rather_than_wrapping() {
        let mut tree = Tree::new();
        tree.next_index = MAX_LEAVES;
        assert_eq!(tree.append(leaf(1)), None);
    }
}
