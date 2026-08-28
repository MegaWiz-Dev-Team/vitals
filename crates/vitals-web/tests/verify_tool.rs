//! The verification tool must not be able to rot quietly.
//!
//! `verify_player` is the binary the landing page calls "an independent verification tool, public
//! and reproducible", and it is the first thing a technical reviewer runs. It shipped with a
//! hardcoded `TREE_ID` and a hardcoded player, and both went stale: the demo server rotates its
//! Merkle tree, `ProvenAttempt` gained two fields, and within a week there was no argument that
//! made the tool print a result at all. It answered the reviewer with a borsh panic.
//!
//! The fix is that the defaults are looked up at runtime — `/api/chain` for the tree, the chain
//! itself for the players. One constant survives, `TREE_ID_FALLBACK`, for a reviewer with no route
//! to the demo server. That constant is the only thing left that can go stale, so this file makes
//! going stale noisy:
//!
//! * offline, on every `cargo test`: the fallback must be written down in `VERIFICATION.md`, the
//!   walkthrough must exist, and the runtime lookup must still be wired in. Bump the constant and
//!   forget the doc, or quietly revert to a bare hardcoded default, and the workspace goes red.
//! * `--ignored`, when a reviewer or CI has network: the fallback must equal the tree the server
//!   is anchoring to right now.
//!
//! The offline half deliberately reads the *source* rather than importing anything. A binary's
//! constants are not visible to an integration test, and the property under test is a property of
//! what is written in the file — which is exactly what a future editor will change.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/vitals-web.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn tool_source() -> String {
    let p = root().join("crates/vitals-web/src/bin/verify_player.rs");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} unreadable: {e}", p.display()))
}

fn walkthrough() -> String {
    let p = root().join("VERIFICATION.md");
    std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "VERIFICATION.md is missing from the repository root ({e}).\n\
             verify_player's own doc-comment points a reviewer at it, and the landing page sells \
             the tool as reproducible. A walkthrough that does not exist is worse than none, \
             because the reader concludes the rest is theatre too.\n\
             looked at: {}",
            p.display()
        )
    })
}

/// The fallback tree id, as the source actually writes it: `488_905_120`.
fn fallback_tree_id() -> u64 {
    let src = tool_source();
    let after = src
        .split("const TREE_ID_FALLBACK: u64 =")
        .nth(1)
        .expect("verify_player must still declare TREE_ID_FALLBACK");
    let lit: String = after
        .chars()
        .take_while(|c| *c != ';')
        .filter(|c| c.is_ascii_digit())
        .collect();
    lit.parse().expect("TREE_ID_FALLBACK must be a u64 literal")
}

#[test]
fn the_walkthrough_the_tool_points_at_exists() {
    let doc = walkthrough();
    // Not just present — usable. These are the four things a stranger cannot proceed without.
    for needle in [
        "cargo build",
        "verify_player",
        "/api/chain",
        "/api/sce/",
    ] {
        assert!(doc.contains(needle), "VERIFICATION.md never mentions {needle}");
    }
}

#[test]
fn the_fallback_tree_is_written_down_where_a_reader_can_check_it() {
    let id = fallback_tree_id();
    let doc = walkthrough();
    // Both spellings, because the doc shows command output (488905120) and prose may group it.
    let plain = id.to_string();
    assert!(
        doc.contains(&plain),
        "verify_player falls back to tree #{plain}, and VERIFICATION.md does not say so.\n\
         Whoever bumps that constant must also update the walkthrough — otherwise the document \
         a reviewer follows describes a tree the tool no longer uses, which is the failure this \
         whole change exists to end.\n\
         Refresh both from the live server:\n\
         \x20   curl -s https://devnet.vitals.academy/api/chain | tr ',' '\\n' | grep tree_id"
    );
}

#[test]
fn the_tree_id_is_looked_up_before_it_is_assumed() {
    let src = tool_source();
    // The defect was a bare constant with no way to be right tomorrow. Keep all three routes.
    assert!(
        src.contains("VITALS_TREE_ID"),
        "the tree id must stay overridable by environment"
    );
    assert!(
        src.contains("/api/chain"),
        "the tree id must still be readable from the live server — a compiled-in default is a \
         guess about a number that changes"
    );
    assert!(
        src.contains("fn live_tree_id"),
        "live_tree_id() is the lookup that keeps this tool from rotting; do not remove it"
    );
}

#[test]
fn the_decode_failure_is_explained_rather_than_panicked() {
    let src = tool_source();
    assert!(
        src.contains("fn explain_stale_layout"),
        "records written by an older ProvenAttempt layout must be explained, not panicked at"
    );
    // The specific regression: `.expect()` on the borsh decode put a panic and a line number in
    // front of an independent reviewer following the landing page's instructions.
    assert!(
        !src.contains("ClaimAccount decode"),
        "the ClaimAccount decode must not go back to .expect()"
    );
}

/// The half that needs the network, so it is `--ignored` and the offline gates stay offline.
///
/// ```text
/// cargo test -p vitals-web --test verify_tool -- --ignored
/// ```
#[test]
#[ignore]
fn the_fallback_tree_is_still_the_live_one() {
    let body: serde_json::Value = ureq::get("https://devnet.vitals.academy/api/chain")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .expect("GET /api/chain")
        .into_json()
        .expect("/api/chain is JSON");
    let live = body["tree_id"].as_u64().expect("/api/chain has tree_id");
    assert_eq!(
        fallback_tree_id(),
        live,
        "the demo server has rotated its tree. verify_player's offline fallback is stale — \
         update TREE_ID_FALLBACK and the number in VERIFICATION.md to {live}."
    );
}
