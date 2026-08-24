//! The store has to survive a machine with no disk.
//!
//! Runs and the anchoring tree were files on a persistent volume, which works in the cluster and
//! not at all on Cloud Run — a container there has no disk that outlives the request, and two
//! instances share nothing. Firestore is what the rest of this company already uses for the same
//! problem, over REST, with each record kept as one JSON string. These tests pin the parts of
//! that which do not need a network: how a document is addressed, how a record is encoded, and
//! which backend a given environment selects.

use vitals_web::store::{Backend, Store};

#[test]
fn a_missing_project_means_disk() {
    let b = Backend::from_env(None, "/tmp/whatever");
    assert!(matches!(b, Backend::Disk { .. }), "no project id, so nowhere to talk to");
}

#[test]
fn a_project_means_firestore() {
    let b = Backend::from_env(Some("cloud-super-hero-dev"), "/tmp/whatever");
    assert!(matches!(b, Backend::Firestore { .. }));
}

/// A document lives under a collection named for the kind. Getting this wrong writes sessions
/// where the tree lives, and the failure looks like data loss rather than a bad path.
#[test]
fn documents_are_addressed_by_kind_and_key() {
    let b = Backend::from_env(Some("proj"), "/tmp/x");
    assert_eq!(
        b.doc_path("sessions", "abc123").as_deref(),
        Some("https://firestore.googleapis.com/v1/projects/proj/databases/(default)/documents/sessions/abc123")
    );
    assert_eq!(
        b.doc_path("tree", "current").as_deref(),
        Some("https://firestore.googleapis.com/v1/projects/proj/databases/(default)/documents/tree/current")
    );
}

/// Keys arrive from the network and become part of a URL. A slash would silently address a
/// different collection; the disk backend already refuses these and Firestore must too.
#[test]
fn unsafe_keys_are_refused_by_both_backends() {
    let fs = Backend::from_env(Some("proj"), "/tmp/x");
    let disk = Backend::from_env(None, "/tmp/x");
    for bad in ["../escape", "a/b", "", "with space", &"x".repeat(65)] {
        assert!(fs.doc_path("sessions", bad).is_none(), "firestore accepted {bad:?}");
        assert!(disk.doc_path("sessions", bad).is_none(), "disk accepted {bad:?}");
    }
}

/// One JSON string per record, the convention the rest of the company already uses. A record has
/// to survive the trip through Firestore's wrapper unchanged.
#[test]
fn a_record_round_trips_through_the_document_wrapper() {
    let payload = serde_json::json!({ "ep": "ep1", "tape": [{"tick": 30.0}], "anchored": true });
    let doc = Store::wrap(&serde_json::to_string(&payload).unwrap());
    let back = Store::unwrap(&doc).expect("a json field");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&back).unwrap(), payload);
}

/// Thai and emoji in a patient's dialogue must not come back mangled.
#[test]
fn unicode_survives_the_wrapper() {
    let s = "เธอหายใจไม่ออก · 🔥 · \"quoted\" · back\\slash";
    assert_eq!(Store::unwrap(&Store::wrap(s)).as_deref(), Some(s));
}

/// A document written by something else, or by an older build, must not take the server down.
#[test]
fn a_document_we_did_not_write_is_skipped_not_fatal() {
    for junk in [
        serde_json::json!({}),
        serde_json::json!({ "fields": {} }),
        serde_json::json!({ "fields": { "json": {} } }),
        serde_json::json!({ "fields": { "other": { "stringValue": "x" } } }),
    ] {
        assert_eq!(Store::unwrap(&junk), None, "accepted {junk}");
    }
}
