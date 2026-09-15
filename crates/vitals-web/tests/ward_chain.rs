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

// ── who signs what ──────────────────────────────────────────────────────────

use solana_sdk::{hash::Hash, signature::{Keypair, Signer}};
use vitals_web::ward_chain::prepare_for;

/// The relay pays and the player plays, and neither can do the other's half.
///
/// This is what "no signup, no wallet" costs us in care: the server builds the transaction and
/// signs it as fee payer, then hands the bytes to a browser that holds the only key that can
/// finish it. A stranger's key must not complete somebody else's shift, and the server must not
/// be able to complete it at all — if it could, every shift on the ward would be a shift we could
/// have written ourselves, and the record would prove nothing about anyone.
#[test]
fn a_prepared_shift_is_paid_for_here_and_finished_in_the_browser() {
    let relay = Keypair::new();
    let player = Keypair::new();
    let program = Pubkey::new_unique();
    let ix = || take_shift_ix(&program, &relay.pubkey(), &player.pubkey(), 42);

    let pending = prepare_for(&relay, ix(), &player.pubkey(), Hash::default())
        .expect("the relay can always prepare");
    let to_sign = pending.message();
    assert!(!to_sign.is_empty(), "there are bytes for the browser to sign");

    // Somebody else's signature over the same bytes is not this player's shift.
    let stranger = Keypair::new().sign_message(&to_sign);
    assert!(pending.signed(&stranger.into()).is_err(),
            "a signature from another key must not complete a shift — the key on the record is \
             the whole claim");

    let signed = prepare_for(&relay, ix(), &player.pubkey(), Hash::default())
        .expect("prepare again")
        .signed(&player.sign_message(&to_sign).into())
        .expect("the player's own signature completes it");
    assert!(signed.verify().is_ok(), "and what comes out is a transaction the cluster will take");
    assert_eq!(signed.message.account_keys[0], relay.pubkey(),
               "the fee payer is the relay: a stranger never buys SOL to be treated by strangers");
}

// ── the factory's door ──────────────────────────────────────────────────────

use vitals_web::ward::{Pack, Persona};
use vitals_web::ward_chain::{pack_id, validate_pack};

fn a_pack() -> Pack {
    Pack {
        case: "ep2-stemi".into(),
        persona: Persona { name: "Ploy Siriwattana".into(), country: "THA".into(), age: 54 },
        portrait: None,
        endemic: false,
    }
}

/// The door the factory pushes patients through, and everything it refuses.
///
/// The factory runs unattended on another machine, so this is the last place a wrong patient can
/// be stopped. After it, she is on a board in front of strangers with her name on her.
#[test]
fn the_queue_takes_only_patients_the_ward_can_actually_serve() {
    assert!(validate_pack(&a_pack()).is_ok());

    let mut p = a_pack();
    p.case = "ddx-dengue-fever-1".into();
    assert!(validate_pack(&p).is_err(),
            "a case the ward has not converted is a bed nobody can open — and the factory would \
             never hear about it");

    let mut p = a_pack();
    p.persona.country = "Thailand".into();
    assert!(validate_pack(&p).is_err(), "the globe matches on alpha-3, not on a country's name");

    let mut p = a_pack();
    p.persona.name = "   ".into();
    assert!(validate_pack(&p).is_err(), "a patient with no name is a patient nobody can talk about");

    let mut p = a_pack();
    p.persona.age = 0;
    assert!(validate_pack(&p).is_err(), "nobody is nought");
    p.persona.age = 130;
    assert!(validate_pack(&p).is_err(), "and nobody is a hundred and thirty");

    // The one claim the ward can check for itself, so it does: today no country has an endemic
    // list, so every pack claiming an endemic draw is claiming something no list supports.
    let mut p = a_pack();
    p.endemic = true;
    assert!(validate_pack(&p).is_err(),
            "a pack may not label itself endemic for a country and case the endemic list does not \
             pair — an endemic tag nothing backs is the stereotype the rule exists to prevent");

    // The portrait is one shape and no other. The board renders it as an image src on a page
    // strangers open, so "any https url" would let a pack point the ward's own page at whatever
    // the factory — or anything that reached the factory — decided to name.
    let bucket = "https://storage.googleapis.com/vitals-world-portraits";
    let sha = "a".repeat(64);
    let mut p = a_pack();
    p.portrait = Some(format!("{bucket}/{sha}.webp"));
    assert!(validate_pack(&p).is_ok(), "the one shape the portraits are published in");

    for wrong in [
        "javascript:alert(1)".to_string(),
        "https://example.invalid/a.jpg".to_string(),
        format!("http://storage.googleapis.com/vitals-world-portraits/{sha}.webp"),
        format!("https://storage.googleapis.com/some-other-bucket/{sha}.webp"),
        format!("https://storage.googleapis.com/vitals-world-portraits/{sha}.png"),
        format!("https://storage.googleapis.com/vitals-world-portraits/../{sha}.webp"),
        format!("https://storage.googleapis.com/vitals-world-portraits/{}.webp", "z".repeat(64)),
        format!("https://storage.googleapis.com/vitals-world-portraits/{}.webp", "a".repeat(63)),
    ] {
        let mut p = a_pack();
        p.portrait = Some(wrong.clone());
        assert!(validate_pack(&p).is_err(), "{wrong} is not a portrait this ward publishes");
    }
}

/// Content-addressed, so the same patient is never queued twice.
#[test]
fn a_pack_is_named_by_what_is_in_it() {
    let one = pack_id(&a_pack());
    assert_eq!(one, pack_id(&a_pack()), "the same pack is the same id, on any machine");
    assert_eq!(one.len(), 64, "a sha256, in hex");

    let mut other = a_pack();
    other.persona.age = 55;
    assert_ne!(one, pack_id(&other), "a different patient is a different pack");

    let mut same_person_other_case = a_pack();
    same_person_other_case.case = "osce-a".into();
    assert_ne!(one, pack_id(&same_person_other_case),
               "the same person with a different disease is a different patient, and both may be \
                queued — the factory is what decides, not the address");
}

/// The queue keeps each patient once, and says what it would not take.
///
/// The factory pushes pages of packs from another machine on a timer, so overlap is the normal
/// case and not an error: a retry, a restart, a window that covers what the last one covered. What
/// must never happen is the same patient queued twice — she would be admitted twice, to two beds,
/// with one name.
///
/// And a refusal has to come back in words. The factory is unattended; a door that silently
/// dropped a third of what it was sent would look exactly like a factory that was running.
#[test]
fn the_queue_keeps_each_patient_once_and_says_what_it_refused() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{enqueue, queue_depth};

    let root = std::env::temp_dir().join(format!("vitals-queue-{}-{:?}", std::process::id(),
                                                 std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(root.clone()).expect("a store to queue into");

    let mut other = a_pack();
    other.persona.name = "Anan Thepwong".into();
    let mut wrong = a_pack();
    wrong.case = "ddx-dengue-fever-1".into();

    let first = enqueue(&store, vec![a_pack(), a_pack(), other.clone(), wrong]);
    assert_eq!(first.queued, 2, "two patients, and the repeat of the first is not a third");
    assert_eq!(first.duplicates, 1);
    assert_eq!(first.rejected.len(), 1, "and the one it would not take");
    assert!(first.rejected[0].contains("dengue"), "named, so the factory can fix it: {:?}",
            first.rejected);
    assert_eq!(first.depth, 2, "the depth is what the factory tops up against");

    // The same page again, in full. Nothing is added and nothing is lost.
    let again = enqueue(&store, vec![a_pack(), other]);
    assert_eq!(again.queued, 0);
    assert_eq!(again.duplicates, 2);
    assert_eq!(again.depth, 2);
    assert_eq!(queue_depth(&store), 2);

    let _ = std::fs::remove_dir_all(&root);
}

// ── the refill ──────────────────────────────────────────────────────────────

use vitals_web::ward_chain::{choose_next, next_patient_id};

fn queued(case: &str, name: &str) -> (String, Pack) {
    let p = Pack {
        case: case.into(),
        persona: Persona { name: name.into(), country: "THA".into(), age: 40 },
        portrait: None,
        endemic: false,
    };
    (pack_id(&p), p)
}

/// Which patient the empty bed gets, decided the way the published policy says.
///
/// Two rules, and both are visible on `/api/ward`: no two beds hold the same case at once, and
/// every difficulty band is represented when beds allow. The second is what stops a ward of three
/// intern cases from being the only thing a student can find at four in the morning.
#[test]
fn the_next_patient_is_the_one_the_ward_is_missing() {
    let student = queued("osce-a", "A Student Case");     // student
    let intern = queued("ep2-stemi", "An Intern Case");   // intern
    let resident = queued("osce-d4", "A Resident Case");  // resident
    let queue = vec![student.clone(), intern.clone(), resident.clone()];

    // An empty ward: nothing is under-represented, so the choice is the same every time rather
    // than arbitrary. A ward that admitted a different patient on each tick would be a ward whose
    // behaviour nobody could reproduce from the same state.
    let empty: Vec<String> = vec![];
    let first = choose_next(&queue, &empty).expect("an empty ward admits somebody");
    assert_eq!(first, choose_next(&queue, &empty).unwrap(), "the same state, the same patient");

    // Two interns already in beds: the student and the resident are what the ward is missing.
    let interns = vec!["ep2-stemi".to_string(), "osce-b".to_string()];
    let pick = choose_next(&queue, &interns).expect("a bed to fill");
    assert!(pick == student.0 || pick == resident.0,
            "with two interns on the ward, a third would leave a student with nothing to open");

    // Every band once: the one band with nobody in it wins.
    let one_each = vec!["osce-a".to_string(), "ep2-stemi".to_string()];
    let pick = choose_next(&[intern.clone(), resident.clone()], &one_each)
        .expect("a bed to fill");
    assert_eq!(pick, resident.0, "student and intern are held; resident is the empty band");

    // A case already in a bed is not admitted again, whatever else it would balance.
    let on_ward: Vec<String> = vec!["osce-a".into(), "ep2-stemi".into(), "osce-d4".into()];
    assert!(choose_next(&queue, &on_ward).is_none(),
            "no two beds hold the same case at once — the published rule, and an empty bed is \
             better than breaking it");

    assert!(choose_next(&[], &empty).is_none(), "an empty queue admits nobody");
}

/// A patient id has to be one nobody has used, because it is her address.
#[test]
fn a_patient_id_is_never_reused() {
    let now = 1_760_000_000u64;
    assert_eq!(next_patient_id(now, &[]), now, "the clock, when the clock is free");

    // Three admitted in the same second — the ordinary case when a ward opens with empty beds.
    assert_eq!(next_patient_id(now, &[now]), now + 1);
    assert_eq!(next_patient_id(now, &[now, now + 1]), now + 2);

    // Ids are seeded into the patient's address, so reusing one would not collide loudly: it
    // would open the account that already exists and quietly write a second patient's admission
    // over the first one's chart.
    assert!(![now, now + 1].contains(&next_patient_id(now, &[now, now + 1])));
}

/// **The ward opens when the founder says so, not when a deploy lands.**
///
/// Production can carry this code for days before the ward is meant to be open, and the factory is
/// a job on another machine that will push the moment it has packs. One mistaken push must not be
/// what opens a public ward — so the door is shut unless something says otherwise, and the way it
/// fails is closed.
#[test]
fn the_door_is_shut_unless_somebody_opened_it() {
    use vitals_web::ward_chain::door_is_open;

    assert!(!door_is_open(None), "a deploy that says nothing has a closed door");
    assert!(door_is_open(Some("open")), "and one word opens it");
    assert!(door_is_open(Some("OPEN")), "however it is typed");
    assert!(door_is_open(Some(" open ")), "and with whatever whitespace a shell adds");

    for shut in ["", "closed", "false", "0", "no", "opened", "open the ward", "1", "true"] {
        assert!(!door_is_open(Some(shut)),
                "{shut:?} is not the word — anything that is not 'open' leaves it shut, because \
                 the failure that matters is a ward that opened by accident");
    }
}
