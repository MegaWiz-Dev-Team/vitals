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

// ── the check the tool tells a stranger to run ──────────────────────────────────────────────────
//
// For every attempt it printed, `verify_player` used to print
//
//     curl -s https://devnet.vitals.academy/api/sce/<case> | shasum -a 256   # → <case>
//
// and that command stopped working the day `/api/sce` was narrowed to retired scenarios. A file
// that can still be sat is a mark sheet — every matcher keyword, every `(HARM)`, the thresholds
// that decide the outcome, `_note` fields naming the diagnosis — so the endpoint refuses it, and
// every hash anchored on devnet today names a live case. The reader followed our own instruction,
// hashed a 404 body, got a digest nothing like the one printed beside it, and had every reason to
// conclude the proof was theatre. A verification tool that prints a failing command argues against
// the thing it exists to prove, exactly as the stale tree id did.
//
// The replacement needs no server: the archive is committed, append-only and named by digest, so
// the file a leaf points at is already in the clone the reader built this binary from. These tests
// keep the printed command runnable and keep it agreeing with `VERIFICATION.md` §5.

/// Every hash filed in the committed archive.
fn archived_hashes() -> Vec<String> {
    let dir = root().join(vitals_web::archive::DIR);
    let rd = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} is not readable: {e}", dir.display()));
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(str::to_string))
        .filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()))
        .collect();
    v.sort();
    v
}

#[test]
fn the_scenario_check_is_hashed_from_the_clone_not_fetched_from_the_endpoint() {
    let src = tool_source();
    assert!(
        !src.contains("api/sce/{h}"),
        "verify_player must not print a curl of /api/sce/<hash> as *the* check. That route serves \
         retired scenarios only — a case still on the playable shelf is withheld, because the file \
         is its own mark sheet — and every hash anchored on devnet today names a live case. The \
         reader would hash a 404 body and read the mismatch as the proof being fake."
    );
    assert!(
        src.contains("vitals_web::archive::DIR"),
        "the archive path the tool prints must come from the server's own constant, so the command \
         handed to a stranger cannot drift from the directory that holds the files"
    );
    assert!(
        src.contains("shasum -a 256"),
        "the tool must still print a hash-it-yourself command — the round trip from a leaf to a \
         file you can read is the last link in the argument"
    );
}

/// Not "the string looks plausible": the path the tool prints must name a file that is really in
/// the clone, and hashing it must really give back the name. Checked for every hash the tool
/// could ever print, which is every version the archive holds.
#[test]
fn the_printed_command_reproduces_the_hash_it_promises() {
    use sha2::{Digest, Sha256};
    let dir = root().join(vitals_web::archive::DIR);
    let all = archived_hashes();
    assert!(
        all.len() >= 17,
        "only {} versions in {} — the archive is append-only and a run anchored against a missing \
         one has no route left at all, because the endpoint will not serve a live case either",
        all.len(),
        dir.display()
    );
    for h in &all {
        // Exactly the path `print_scenario_check` formats.
        let p = dir.join(format!("{h}.json"));
        let bytes = std::fs::read(&p)
            .unwrap_or_else(|e| panic!("verify_player prints `shasum -a 256 {}/{h}.json` and that file is not there: {e}", vitals_web::archive::DIR));
        let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            got, *h,
            "the command verify_player prints for {h} does not print {h} back — it prints {got}"
        );
    }
}

/// The tool and the walkthrough have to send a reader down the same road. §5 of `VERIFICATION.md`
/// leads with the clone, and the tool now prints the clone; if either is rewritten to lead with
/// the endpoint again, this is where it is caught.
#[test]
fn the_walkthrough_leads_with_the_same_check_the_tool_prints() {
    let doc = walkthrough();
    let dir = vitals_web::archive::DIR;
    assert!(
        doc.contains(&format!("shasum -a 256 {dir}/")),
        "VERIFICATION.md must show the same `shasum -a 256 {dir}/<hash>.json` check that \
         verify_player prints — a walkthrough describing a different route than the tool is the \
         rot this file exists to catch"
    );
    assert!(
        doc.contains("retired"),
        "VERIFICATION.md must say that GET /api/sce/<hash> publishes a scenario only once its case \
         is retired. Without that, a reader hits the 404 and concludes the chain is lying."
    );
}
