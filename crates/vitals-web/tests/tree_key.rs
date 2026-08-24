//! Two deployments must not share a leaf list.
//!
//! The tree lived at the constant key `tree/current`. Nothing about that key says which relay
//! filled it or which chain it was anchored to, so any two servers sharing a store shared the
//! list — and the list is the thing every Merkle proof is rebuilt from. Losing it leaves the
//! anchor on chain and nothing able to prove anything against it.
//!
//! This is the same defect that was fixed on chain this morning, where the tree account was
//! addressed by a globally-guessable id, reappearing one layer down in storage.

use vitals_web::store::tree_key;

const RELAY_A: &str = "535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG";
const RELAY_B: &str = "4MNV22NAmms2YR5P2GMxVST5ZSeQvsn9wx2MHa4ZBuQF";
const PROGRAM: &str = "535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG";
const DEVNET: &str = "https://api.devnet.solana.com";
const MAINNET: &str = "https://api.mainnet-beta.solana.com";

#[test]
fn two_operators_do_not_share_a_leaf_list() {
    assert_ne!(tree_key(RELAY_A, PROGRAM, DEVNET), tree_key(RELAY_B, PROGRAM, DEVNET));
}

#[test]
fn one_operator_on_two_chains_does_not_share_a_leaf_list() {
    // The same relay key can serve devnet and mainnet. Their leaves are anchored into different
    // chains and must never land in one list.
    assert_ne!(tree_key(RELAY_A, PROGRAM, DEVNET), tree_key(RELAY_A, PROGRAM, MAINNET));
}

#[test]
fn a_restart_finds_the_list_it_was_filling() {
    // The other half of the requirement, and the easier one to break while fixing the first: a
    // server that comes back must resume its own tree, not start an empty one and lose the ability
    // to prove everything it anchored before the restart.
    assert_eq!(tree_key(RELAY_A, PROGRAM, DEVNET), tree_key(RELAY_A, PROGRAM, DEVNET));
}

#[test]
fn the_key_is_a_plain_name_the_store_will_accept() {
    // It becomes a Firestore document id and a filename. A slash would address a different
    // collection and read back as missing data rather than as a bad key.
    let k = tree_key(RELAY_A, PROGRAM, DEVNET);
    assert!(!k.is_empty());
    assert!(k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{k:?}");
}
