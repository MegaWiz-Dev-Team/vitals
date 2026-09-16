//! **The tape that is anchored is the tape that is kept.**
//!
//! A hand shift on Abebe Tadesse, driven through a real browser on staging 00016, anchored
//! cleanly — and left a patient nobody can open. The chain carries leaf `0379…`; the only tape
//! filed for him is `04978b…`, thirteen steps, written when `/api/handover` reduced the shift.
//! Between that reduction and the anchor the page's clock went on posting ticks to `/api/step`,
//! so `record_for` reduced a longer tape and put *its* hash on chain. Two hashes for one shift,
//! and the chain got the one nobody kept.
//!
//! Three rules come out of it, and this file holds all three:
//!
//!   * a shift that has been handed over takes no more steps;
//!   * the tape is kept under the hash that is about to go on chain, by the code that builds the
//!     instruction — one truth, in the place that writes the chain;
//!   * and a leaf already on chain with no tape here is repaired if any stored session reduces to
//!     it, rather than leaving a bed on the board that no stranger can take.

use vitals_replay::Step;
use vitals_web::store::Store;
use vitals_web::ward_chain::{keep_tape, tape_by_hash, StoredTape};

fn store(tag: &str) -> Store {
    let dir = std::env::temp_dir().join(format!("vitals-tape-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Store::open(dir).expect("a store")
}

fn ep1() -> String {
    std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json"),
    )
    .expect("ep1 is in the repository")
}

/// **The hand-over is the end of the shift**, and a step after it is a step onto a tape that has
/// already been reduced.
#[test]
fn a_shift_that_has_been_handed_over_takes_no_more_steps() {
    use vitals_web::ward::may_step;
    assert!(may_step(true, true, false).is_ok(), "a declared shift, still running");
    let refused = may_step(true, true, true).expect_err("handed over");
    assert!(refused.contains("handed over"), "and it says which end it is: {refused}");
    // The season's bay ends at the bell and has its own guard there; nothing about a ward
    // hand-over reaches it.
    assert!(may_step(false, false, true).is_ok(), "a run that is not a shift is not frozen by this");
}

/// **One hash, written by the code that writes the chain.**
///
/// `keep_for_anchor` takes the record the instruction will carry and files the tape under *that*
/// record's own `run_hash`. There is no second reduction and no second hash: whatever goes on
/// chain is what a reader will find here.
#[test]
fn the_tape_is_kept_under_the_hash_that_goes_on_chain() {
    use vitals_web::ward_chain::keep_for_anchor;

    let sce = ep1();
    let tape = vec![Step::Do("oxygen".into()), Step::Tick(30.0), Step::Do("adrenaline im".into())];
    let r = vitals_replay::replay(&sce, &tape).expect("replay");
    let rec = vitals_replay::record_for(
        [7u8; 32],
        vitals_replay::sce_hash(&sce),
        vitals_replay::sce_hash(&sce),
        vitals_progress::Difficulty::Intern,
        false,
        &tape,
        &r,
        [9u8; 32],
        42,
    )
    .expect("a record");

    let st = store("anchor");
    let hash = keep_for_anchor(&st, 1789538329, &rec, &tape).expect("the tape is kept");
    assert_eq!(hash, vitals_web::ward_chain::hex32(&rec.run_hash),
               "the hash it is filed under is the record's own — not one computed a second time");
    assert_eq!(tape_by_hash(&st, &hash).as_deref(), Some(&tape[..]),
               "and what comes back is what was played");
}

/// **A leaf on chain with no tape here is a bed nobody can take.**
///
/// The repair looks through the sessions this server still holds: a stored tape that reduces to
/// the missing leaf *is* that shift, whatever went wrong at the time, and filing it makes the
/// patient openable again. Abebe's own session still held the longer tape when this was written.
#[test]
fn an_anchored_leaf_with_no_tape_is_repaired_from_a_session_that_reduces_to_it() {
    use vitals_web::ward_chain::recover_tape;

    let sce = ep1();
    let short = vec![Step::Do("oxygen".into()), Step::Tick(30.0)];
    let long: Vec<Step> = short.iter().cloned().chain([Step::Tick(2.0), Step::Tick(2.0)]).collect();

    let leaf_of = |t: &[Step]| {
        let r = vitals_replay::replay(&sce, t).expect("replay");
        vitals_web::ward_chain::hex32(&vitals_replay::leaf(&vitals_replay::sce_hash(&sce), t, &r))
    };
    let anchored = leaf_of(&long);
    assert_ne!(anchored, leaf_of(&short), "two ticks are a different shift, which is the whole bug");

    let st = store("repair");
    // What the ward filed at hand-over: the short one, under its own hash.
    keep_tape(&st, &StoredTape {
        patient_id: 1789538329,
        run_hash: leaf_of(&short),
        steps: short.clone(),
    })
    .expect("kept");

    // The sessions this server is still holding, tape and scenario — the only place the longer
    // one survives.
    let held = vec![(sce.clone(), long.clone()), (sce.clone(), short.clone())];
    let found = recover_tape(&st, 1789538329, &anchored, &held).expect("the longer tape reduces to it");
    assert_eq!(found, long, "the repair files the tape that actually produced the leaf");
    assert_eq!(tape_by_hash(&st, &anchored).as_deref(), Some(&long[..]),
               "and the patient can be rebuilt from here on");

    // Nothing that reduces to it: the honest answer is none, and the caller says so on the board
    // rather than filing something that is not her shift.
    let other = store("repair-miss");
    assert!(recover_tape(&other, 1, &anchored, &[(sce, short)]).is_none(),
            "a tape that reduces to a different leaf is a different shift");
}

/// **A zero run hash is not a shift.**
///
/// `/api/shift/000…0` resolved to a real-looking receipt on staging: a proof-tool leaf sat in the
/// shift cache with an all-zero `run_hash`, and a lookup by hash found it. A hash of nothing is
/// what an uninitialised record deserialises to, never what a tape hashes to — so it is refused at
/// the door rather than reasoned about downstream, and the cached row that produced it is skipped
/// wherever it appears.
#[test]
fn a_hash_of_nothing_names_no_shift() {
    use vitals_web::ward_chain::is_shift_hash;
    assert!(!is_shift_hash(&"0".repeat(64)), "all zeroes is an empty record, not a shift");
    assert!(!is_shift_hash(""), "nor is nothing at all");
    assert!(!is_shift_hash(&"a".repeat(63)), "nor a hash of the wrong length");
    assert!(!is_shift_hash(&format!("{}z", "a".repeat(63))), "nor one that is not hex");
    assert!(is_shift_hash(&format!("{}1", "0".repeat(63))), "but one real byte makes it a hash");
    assert!(is_shift_hash(&"a".repeat(64)));
}

/// **The session that will not replay is the one holding the tape.**
///
/// A ward session is restored by rebuilding the patient from the chain, so a session whose own
/// shift is the one with the missing tape fails to restore — and until now the boot loop then
/// deleted it. That is the only copy of a tape for a leaf already on chain, thrown away by the
/// code that found the problem; on staging it is why Abebe's tape was gone before the repair
/// existed to look for it.
///
/// So the repair runs **before** anything is dropped, and it is the same function the ticker uses.
#[test]
fn the_repair_offers_every_stored_tape_before_any_session_is_dropped() {
    use vitals_web::ward_chain::{missing_tapes, recover_tape};

    let sce = ep1();
    let played = vec![Step::Do("oxygen".into()), Step::Tick(30.0), Step::Tick(2.0)];
    let r = vitals_replay::replay(&sce, &played).expect("replay");
    let anchored = vitals_web::ward_chain::hex32(&vitals_replay::leaf(
        &vitals_replay::sce_hash(&sce),
        &played,
        &r,
    ));

    let st = store("boot");
    // The chain says this leaf exists; nothing here has its tape.
    let shifts = vec![vitals_web::ward::ShiftOnChain {
        patient_id: 1789538329,
        signer: [3; 32],
        slot: 499_153_055,
        run_hash: {
            let mut b = [0u8; 32];
            for (i, x) in (0..32).map(|i| (i, u8::from_str_radix(&anchored[i * 2..i * 2 + 2], 16).unwrap())) {
                b[i] = x;
            }
            b
        },
    }];
    assert_eq!(missing_tapes(&st, &shifts), vec![anchored.clone()],
               "the leaf with no tape is named, and only that one");

    // The session that failed to restore is exactly the one holding it.
    let found = recover_tape(&st, 1789538329, &anchored, &[(sce, played.clone())]);
    assert_eq!(found.as_deref(), Some(&played[..]), "offered, and taken");
    assert!(missing_tapes(&st, &shifts).is_empty(), "and nothing is missing afterwards");
}
