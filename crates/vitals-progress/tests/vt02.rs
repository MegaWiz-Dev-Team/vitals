//! The record grows, and everything already anchored keeps verifying.
//!
//! `encode()` has carried a four-byte version tag since it was written, with the note that a
//! future encoding should be tellable apart from this one. This is that future encoding. The tag
//! is what makes it an addition rather than a rewrite: a verifier reads the last four bytes and
//! knows which layout it holds, so a `vt01` leaf anchored today still verifies in ten years
//! against code that has never heard of commitments.
//!
//! What the tag does *not* excuse is moving a field inside a version. `vt02` therefore keeps bytes
//! 0..133 exactly where `vt01` put them, and appends.

use vitals_progress::record::{AttemptRecord, Outcome};
use vitals_progress::Difficulty;

fn base() -> AttemptRecord {
    AttemptRecord {
        player: [1; 32],
        sce_hash: [2; 32],
        case: [3; 32],
        difficulty: Difficulty::Resident,
        exam_mode: true,
        outcome: Outcome::WinDischarge,
        harm_count: 2,
        run_hash: [4; 32],
        commitment: [5; 32],
        committed_slot: 7,
        rubric_hash: [6; 32],
        det_score: 38,
        det_max: 40,
        judged_score: 51,
        judged_max: 60,
    }
}

#[test]
fn the_new_encoding_is_217_bytes_and_says_so_at_the_end() {
    let e = base().encode();
    assert_eq!(e.len(), 217);
    assert_eq!(&e[213..217], b"vt02");
}

#[test]
fn every_byte_vt01_defined_is_still_where_it_was() {
    // The half of backward compatibility a version tag cannot buy. Moving a field inside a layout
    // is what breaks readers; appending after it is not.
    let r = base();
    let e = r.encode();
    assert_eq!(&e[0..32], &r.player);
    assert_eq!(&e[32..64], &r.sce_hash);
    assert_eq!(&e[64..96], &r.case);
    assert_eq!(&e[96..128], &r.run_hash);
    assert_eq!(e[128], 2, "difficulty: Resident");
    assert_eq!(e[129], 1, "exam_mode");
    assert_eq!(e[130], r.outcome as u8);
    assert_eq!(&e[131..133], &2u16.to_le_bytes());
}

#[test]
fn the_new_fields_land_where_the_spec_says() {
    let r = base();
    let e = r.encode();
    assert_eq!(&e[133..165], &r.commitment);
    assert_eq!(&e[165..173], &7u64.to_le_bytes());
    assert_eq!(&e[173..205], &r.rubric_hash);
    assert_eq!(&e[205..207], &38u16.to_le_bytes());
    assert_eq!(&e[207..209], &40u16.to_le_bytes());
    assert_eq!(&e[209..211], &51u16.to_le_bytes());
    assert_eq!(&e[211..213], &60u16.to_le_bytes());
}

#[test]
fn a_leaf_anchored_under_vt01_still_hashes_to_what_it_did() {
    // The promise the tag was added to keep. A byte-for-byte reproduction of the old encoding: if
    // the shared prefix ever drifts, leaves already on chain become unverifiable and no amount of
    // new code brings them back.
    let r = base();
    let mut old = [0u8; 137];
    old[0..32].copy_from_slice(&r.player);
    old[32..64].copy_from_slice(&r.sce_hash);
    old[64..96].copy_from_slice(&r.case);
    old[96..128].copy_from_slice(&r.run_hash);
    old[128] = 2;
    old[129] = 1;
    old[130] = r.outcome as u8;
    old[131..133].copy_from_slice(&r.harm_count.to_le_bytes());
    old[133..137].copy_from_slice(b"vt01");

    assert_eq!(&old[0..133], &r.encode()[0..133], "the shared prefix drifted");
    assert_ne!(&old[133..137], &r.encode()[213..217], "the versions claim to be each other");
}

#[test]
fn a_story_mode_leaf_says_nothing_here_needed_a_witness() {
    // judged_max = 0 is not a missing value. It is the record stating that no part of this score
    // rests on anyone's judgement — something a deterministic run should be able to say about
    // itself without the reader having to know which mode produced it.
    let r = AttemptRecord { det_score: 85, det_max: 100, judged_score: 0, judged_max: 0, ..base() };
    let e = r.encode();
    assert_eq!(&e[207..209], &100u16.to_le_bytes());
    assert_eq!(&e[211..213], &0u16.to_le_bytes());
}

#[test]
fn two_runs_differing_only_in_commitment_are_different_leaves() {
    // If the commitment did not reach the hash it would be decoration: a committed run and an
    // uncommitted one would be indistinguishable to anyone checking afterwards.
    let a = base();
    let b = AttemptRecord { commitment: [9; 32], ..base() };
    assert_ne!(a.leaf(), b.leaf());
}

#[test]
fn the_scores_reach_the_hash_separately() {
    // Summing them at any layer makes "does the deterministic part alone predict passing"
    // permanently unanswerable. Swapping them leaves any sum identical, so a leaf that ignored the
    // split would hash the same.
    let a = base();
    let b = AttemptRecord { det_score: 51, judged_score: 38, ..base() };
    assert_ne!(a.leaf(), b.leaf());
}
