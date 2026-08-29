//! Losing a session and losing the leaf list are not the same accident.
//!
//! The store treats every record the same: one `sweep(kind, age)` call deletes by modification
//! time, and the kind is a string the caller passes in. Sessions are meant to expire that way.
//! The tree is not — the Merkle root is anchored on chain, but the *path* to a leaf is rebuilt
//! from this list, so a server that expires it keeps a root it can no longer prove anything
//! against. Nothing in the type system says which is which.

use std::time::Duration;
use vitals_web::store::{self, Class, Store};

/// A state directory nobody else is using.
///
/// The pid is the point. This was a fixed `vitals-dur-{name}`, so two `cargo test` runs against
/// one checkout — a gate and a developer, or two agents — shared these directories and each
/// `remove_dir_all`'d the other's mid-write. The suite then failed for a reason that had nothing
/// to do with the change being gated, which is the one thing a gate must never do. Every other
/// harness in this repo already carries `process::id()`; this one now does too.
fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("vitals-dur-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

#[test]
fn the_tree_is_the_one_thing_that_cannot_be_lost() {
    assert_eq!(store::class_of("tree"), Class::Durable);
}

#[test]
fn a_run_in_progress_is_allowed_to_expire() {
    assert_eq!(store::class_of("sess"), Class::Ephemeral);
}

#[test]
fn a_kind_nobody_classified_is_kept_not_deleted() {
    // A new kind added later reaches sweep before anyone thinks about it. Defaulting to
    // "delete after an hour" turns forgetting into data loss; defaulting to "keep" turns it
    // into disk usage, which is noticed and is fixable.
    assert_eq!(store::class_of("whatever-comes-next"), Class::Durable);
}

#[test]
fn sweep_will_not_expire_the_tree_however_old_it_is() {
    let root = tmp("tree");
    let store = Store::open(root.clone()).unwrap();
    store.put("tree", "current", &serde_json::json!({ "tree_id": 7, "leaves": [] }))
        .expect("write the tree");

    let gone = store.sweep("tree", Duration::from_secs(0));

    assert_eq!(gone, 0, "sweep reported deleting a durable record");
    assert!(
        store.get::<serde_json::Value>("tree", "current").is_some(),
        "the leaf list was expired — every run this server anchored is now unprovable"
    );
}

#[test]
fn sweep_still_expires_sessions() {
    let root = tmp("sess");
    let store = Store::open(root.clone()).unwrap();
    store.put("sess", "abc", &serde_json::json!({ "ep": "ep1" })).expect("write the session");

    let gone = store.sweep("sess", Duration::from_secs(0));

    assert_eq!(gone, 1);
    assert!(store.get::<serde_json::Value>("sess", "abc").is_none());
}
