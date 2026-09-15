//! Reading the ward off the chain, tested where it can be tested.
//!
//! Three things happen between an RPC answer and a number on `/api/ward`, and each of them can be
//! got wrong in a way no integration test on devnet would show for days:
//!
//!   * a patient account is **decoded** — if the layout drifts from the program's, every figure is
//!     wrong and nothing errors;
//!   * a transaction is **classified** — the census counts *anchored shifts*, and a ward that
//!     counted leases taken, or admissions, would report work nobody did;
//!   * a page of history is **merged into the cache** — read the same signature twice and the
//!     ward invents a shift; miss one and it loses a stranger's work.
//!
//! The RPC calls themselves are proven on devnet by `ward_proof`. These are the parts that must
//! hold before the wire is ever touched.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::pubkey::Pubkey;
use vitals_program::{Instruction, PatientAccount, RecordWire, PATIENT_DIED, PATIENT_OPEN};
use vitals_web::ward::{ShiftOnChain, DIED, OPEN};
use vitals_web::ward_chain::{decode_patient, shift_in, Seen, SeenShift};

fn a_patient(id: u64, state: u8, shifts: u32, admitted: u64, closed: u64) -> Vec<u8> {
    let p = PatientAccount {
        operator: [7; 32],
        patient_id: id,
        scenario_hash: [9; 32],
        head: [3; 32],
        shifts,
        state,
        lease_holder: [0; 32],
        lease_until_slot: 0,
        admitted_slot: admitted,
        closed_slot: closed,
    };
    let mut v = Vec::new();
    p.serialize(&mut v).expect("borsh");
    v
}

fn a_record() -> RecordWire {
    RecordWire {
        player: [1; 32], sce_hash: [2; 32], case: [3; 32], run_hash: [4; 32],
        difficulty: 1, exam_mode: false, outcome: 1, harm_count: 0,
        rubric_hash: [5; 32], det_score: 30, det_max: 40, judged_score: 40, judged_max: 60,
    }
}

fn data_of(ix: &Instruction) -> Vec<u8> {
    let mut v = Vec::new();
    ix.serialize(&mut v).expect("borsh");
    v
}

/// The account layout is shared with the program, and nothing at runtime checks that it still is.
#[test]
fn a_patient_account_decodes_into_exactly_what_the_census_counts() {
    let open = decode_patient(&a_patient(42, PATIENT_OPEN, 3, 100, 0)).expect("an open patient");
    assert_eq!(open.patient_id, 42);
    assert_eq!(open.state, OPEN);
    assert_eq!(open.shifts, 3);
    assert_eq!(open.admitted_slot, 100);
    assert_eq!(open.closed_slot, 0);

    let dead = decode_patient(&a_patient(43, PATIENT_DIED, 5, 100, 900)).expect("a closed patient");
    assert_eq!(dead.state, DIED, "the program's state byte and the ward's must be the same byte");
    assert_eq!(dead.closed_slot, 900);

    assert!(decode_patient(&[]).is_none(), "an empty account is not a patient");
    assert!(decode_patient(&[0u8; 8]).is_none(), "and neither is a truncated one");
}

/// The census says *shifts*, and a shift is an anchored leaf — not a lease, not an admission.
#[test]
fn only_an_anchored_shift_counts_and_the_player_is_the_one_who_signed_it() {
    let us = Pubkey::new_unique();
    let operator = Pubkey::new_unique();
    let player = Pubkey::new_unique();
    let patient = Pubkey::new_unique();

    // AnchorShift's accounts, in the program's own order: operator, player, account, tree,
    // commitment, patient, system.
    let anchored = [operator, player, Pubkey::new_unique(), Pubkey::new_unique(),
                    Pubkey::new_unique(), patient, Pubkey::new_unique()];
    let ix = Instruction::AnchorShift {
        tree_id: 1, patient_id: 42, record: a_record(), prev_head: [0; 32],
    };

    let s = shift_in(&us, &us, &data_of(&ix), &anchored, 1234).expect("an anchored shift");
    assert_eq!(s.patient_id, 42);
    assert_eq!(s.signer, player.to_bytes(),
               "the shift belongs to the key that played it, never to the relay that paid");
    assert_eq!(s.slot, 1234);

    let took = data_of(&Instruction::TakeShift { patient_id: 42 });
    assert!(shift_in(&us, &us, &took, &anchored, 1234).is_none(),
            "taking the head is not doing the work — a ward that counted leases would report \
             shifts nobody played");
    let admitted = data_of(&Instruction::AdmitPatient { patient_id: 42, scenario_hash: [0; 32] });
    assert!(shift_in(&us, &us, &admitted, &anchored, 1234).is_none(), "nor is admitting her");

    let someone_else = Pubkey::new_unique();
    assert!(shift_in(&someone_else, &us, &data_of(&ix), &anchored, 1234).is_none(),
            "another program's instruction is not our ward's shift, whatever its bytes decode to");

    assert!(shift_in(&us, &us, &data_of(&ix), &anchored[..2], 1234).is_none(),
            "an instruction naming too few accounts cannot say who played it, and a shift with a \
             guessed signer is worse than no shift");
}

/// Transaction history is read once and kept. Both failure modes here are silent.
#[test]
fn the_cache_reads_only_what_is_new_and_counts_no_shift_twice() {
    let mut seen = Seen::default();
    assert!(seen.until().is_none(), "the first read has nothing to stop at");

    let page = |sig: &str, id: u64, signer: u8, slot: u64| SeenShift {
        signature: sig.to_string(),
        shift: ShiftOnChain { patient_id: id, signer: [signer; 32], slot },
    };

    // Newest first, the way getSignaturesForAddress answers.
    seen.absorb(vec![page("sig3", 42, 0xB2, 300), page("sig2", 42, 0xA1, 200),
                     page("sig1", 42, 0xA1, 100)]);
    assert_eq!(seen.shifts().len(), 3);
    assert_eq!(seen.until().as_deref(), Some("sig3"),
               "the next read stops at the newest signature already read, so history is walked once");

    // The same page again — a retry, a restart, two instances. It must change nothing.
    seen.absorb(vec![page("sig3", 42, 0xB2, 300), page("sig2", 42, 0xA1, 200)]);
    assert_eq!(seen.shifts().len(), 3, "a signature already read is not a new shift");

    seen.absorb(vec![page("sig5", 42, 0xC3, 500), page("sig4", 42, 0xA1, 400)]);
    assert_eq!(seen.shifts().len(), 5);
    assert_eq!(seen.until().as_deref(), Some("sig5"));

    let keys: std::collections::HashSet<[u8; 32]> =
        seen.shifts().iter().map(|s| s.signer).collect();
    assert_eq!(keys.len(), 3, "three keys played five shifts");

    // It survives the trip to Firestore, or it is not a cache.
    let json = serde_json::to_string(&seen).expect("the cache serialises");
    let back: Seen = serde_json::from_str(&json).expect("and comes back");
    assert_eq!(back.shifts().len(), 5);
    assert_eq!(back.until().as_deref(), Some("sig5"));
}

// ── the shift flow, as two instructions ────────────────────────────────────

use vitals_web::ward_chain::{anchor_shift_ix, take_shift_ix};

/// Taking the head is a lease, and it is the player's own hand that takes it.
///
/// The relay pays for the transaction — that is what makes the ward need no wallet — but it must
/// not be able to take a shift on somebody's behalf, or "the record says who" is a record of who
/// we said. So the player signs, and the relay's key appears nowhere in this instruction.
#[test]
fn taking_the_head_is_signed_by_the_player_and_names_only_what_it_touches() {
    let program = Pubkey::new_unique();
    let player = Pubkey::new_unique();
    let operator = Pubkey::new_unique();

    let ix = take_shift_ix(&program, &operator, &player, 42);

    assert_eq!(ix.program_id, program);
    match Instruction::deserialize(&mut &ix.data[..]).expect("decodes") {
        Instruction::TakeShift { patient_id } => assert_eq!(patient_id, 42),
        other => panic!("taking the head must be TakeShift, not {other:?}"),
    }

    assert_eq!(ix.accounts.len(), 3, "player, their account, the patient — and nothing else");
    assert_eq!(ix.accounts[0].pubkey, player);
    assert!(ix.accounts[0].is_signer, "the lease is taken by the hand that will do the work");
    assert!(!ix.accounts[1].is_writable, "reading who they are must not be able to change it");
    assert!(ix.accounts[2].is_writable, "the patient is what the lease is written on");
    assert!(!ix.accounts.iter().any(|a| a.pubkey == operator),
            "the relay pays for this transaction and takes no part in it — a relay that could \
             take a shift could take one in somebody's name");
}

/// Anchoring is the one place both keys appear, and each has exactly one job.
#[test]
fn anchoring_a_shift_is_paid_by_the_relay_and_played_by_the_player() {
    let program = Pubkey::new_unique();
    let operator = Pubkey::new_unique();
    let player = Pubkey::new_unique();
    let head = [7u8; 32];

    let ix = anchor_shift_ix(&program, &operator, &player, 42, 1, a_record(), head);

    match Instruction::deserialize(&mut &ix.data[..]).expect("decodes") {
        Instruction::AnchorShift { tree_id, patient_id, prev_head, .. } => {
            assert_eq!((tree_id, patient_id), (1, 42));
            assert_eq!(prev_head, head,
                       "the shift names the head it believes it is extending — that claim is what \
                        the program refuses when it is wrong");
        }
        other => panic!("anchoring must be AnchorShift, not {other:?}"),
    }

    assert_eq!(ix.accounts.len(), 7, "the program's own seven, in its own order");
    assert_eq!(ix.accounts[0].pubkey, operator, "the relay pays, and rent comes out of it");
    assert!(ix.accounts[0].is_signer);
    assert_eq!(ix.accounts[1].pubkey, player, "and the player signs for the work");
    assert!(ix.accounts[1].is_signer);
    assert!(!ix.accounts[1].is_writable, "signing is not spending: nothing debits the player");

    // The same reading `shift_in` does, on the instruction we just built. If these two ever
    // disagree the census credits the wrong key, and nothing anywhere would say so.
    let keys: Vec<Pubkey> = ix.accounts.iter().map(|a| a.pubkey).collect();
    let seen = shift_in(&program, &program, &ix.data, &keys, 99).expect("a shift, read back");
    assert_eq!(seen.signer, player.to_bytes());
    assert_eq!(seen.patient_id, 42);
}
