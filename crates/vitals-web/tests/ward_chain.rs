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
use vitals_web::ward::{DIED, OPEN};
use vitals_web::ward_chain::{decode_patient, shift_in, Seen};

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

    let page = |sig: &str, id: u64, signer: u8, slot: u64| {
        (ShiftOnChain { patient_id: id, signer: [signer; 32], slot, run_hash: [0; 32] },
         sig.to_string())
    };
    let at = |sig: &str, slot: u64| Some((sig.to_string(), slot));

    // What a walk hands back: the shifts it read, and the newest entry it got all the way through.
    seen.absorb(vec![page("sig3", 42, 0xB2, 300), page("sig2", 42, 0xA1, 200),
                     page("sig1", 42, 0xA1, 100)], at("sig3", 300));
    assert_eq!(seen.shifts().len(), 3);
    assert_eq!(seen.until().as_deref(), Some("sig3"),
               "the next read stops at the newest signature already read, so history is walked once");

    // The same page again — a retry, a restart, two instances. It must change nothing.
    seen.absorb(vec![page("sig3", 42, 0xB2, 300), page("sig2", 42, 0xA1, 200)], at("sig3", 300));
    assert_eq!(seen.shifts().len(), 3, "a signature already read is not a new shift");

    seen.absorb(vec![page("sig5", 42, 0xC3, 500), page("sig4", 42, 0xA1, 400)], at("sig5", 500));
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
        difficulty: None,
        // No case: the ward places her from its own catalogue. A pack naming one of the season's
        // sixteen is refused at this door now, and a pack naming a case the ward holds is checked
        // against the store by `enqueue` rather than by the shape — so the pack's own shape is
        // tested with the case the factory will most often send, which is none.
        case: String::new(),
        persona: Persona { name: "Ploy Siriwattana".into(), country: "THA".into(), age: 54, sex: "f".into() },
        portrait: std::collections::BTreeMap::new(),
        endemic: false,
    }
}

/// The 256 px sibling of the same picture: same object, same sha, a size in its name.
fn small_url(n: u8) -> String {
    portrait_url(n).replace(".webp", "-256.webp")
}

fn portrait_url(n: u8) -> String {
    format!("https://storage.googleapis.com/vitals-world-portraits/{}.webp",
            format!("{n:02x}").repeat(32))
}

/// The door the factory pushes patients through, and everything it refuses.
///
/// The factory runs unattended on another machine, so this is the last place a wrong patient can
/// be stopped. After it, she is on a board in front of strangers with her name on her.
#[test]
fn the_queue_takes_only_patients_the_ward_can_actually_serve() {
    assert!(validate_pack(&a_pack()).is_ok());

    // A case the ward might hold: the shape is fine here and whether this ward *has* it is asked
    // at the door that can see the store (`enqueue`), because the same pack is good the minute
    // after the compiler sends that case.
    let mut p = a_pack();
    p.case = "ddx-dengue-fever-1".into();
    assert!(validate_pack(&p).is_ok(), "the shape of a case id is not the question here");

    // One of the season's sixteen is refused outright, whoever sends it.
    let mut p = a_pack();
    p.case = "osce-a2".into();
    let why = validate_pack(&p).expect_err("the season is vitals.academy's");
    assert!(why.contains("/api/ward/case"), "and the sentence says where cases come from: {why}");

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

    // The endemic claim is not a fact about the pack's shape and is no longer asked here: it is a
    // question about the catalogue — is the case it names tagged endemic, and for which country —
    // so the door asks it where the catalogue is, in `enqueue`. Checked against the season's static
    // file until 17 ก.ย., which turned away every endemic patient the first real factory tick
    // built; `case_door.rs::an_endemic_claim_is_checked_against_the_catalogue` is the rule now.
    let mut p = a_pack();
    p.endemic = true;
    assert!(validate_pack(&p).is_ok(),
            "the pack's own shape cannot answer this, so it must not refuse it either");

    // The portrait is a set now, keyed on the engine's own status words. Each value is the one
    // published shape and no other: the board puts these strings in image sources on a page
    // strangers open, and the factory that supplies them runs unattended on another machine.
    let mut p = a_pack();
    p.portrait.insert("stable".into(), portrait_url(1));
    p.portrait.insert("critical".into(), portrait_url(2));
    assert!(validate_pack(&p).is_ok(), "two of the seven states, both properly published");

    let mut p = a_pack();
    p.portrait.insert("worse".into(), portrait_url(1));
    assert!(validate_pack(&p).is_err(),
            "\"worse\" is not a state the engine reports — a key nothing can produce is a picture \
             that would never be shown, and a typo that is silently never shown is worse");

    let mut p = a_pack();
    p.portrait.insert("dead".into(), portrait_url(1));
    assert!(validate_pack(&p).is_err(),
            "no picture of a dead patient is made (producer, 16 ก.ย.) — the board shows her last \
             living state and says died in words");

    for wrong in [
        "javascript:alert(1)".to_string(),
        "https://example.invalid/a.jpg".to_string(),
        format!("http://storage.googleapis.com/vitals-world-portraits/{}.webp", "a".repeat(64)),
        format!("https://storage.googleapis.com/some-other-bucket/{}.webp", "a".repeat(64)),
        format!("https://storage.googleapis.com/vitals-world-portraits/{}.png", "a".repeat(64)),
        format!("https://storage.googleapis.com/vitals-world-portraits/../{}.webp", "a".repeat(64)),
        format!("https://storage.googleapis.com/vitals-world-portraits/{}.webp", "z".repeat(64)),
        format!("https://storage.googleapis.com/vitals-world-portraits/{}.webp", "a".repeat(63)),
    ] {
        let mut p = a_pack();
        p.portrait.insert("stable".into(), wrong.clone());
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
    assert_eq!(first.depth, Some(2), "the depth is what the factory tops up against");

    // The same page again, in full. Nothing is added and nothing is lost.
    let again = enqueue(&store, vec![a_pack(), other]);
    assert_eq!(again.queued, 0);
    assert_eq!(again.duplicates, 2);
    assert_eq!(again.depth, Some(2));
    assert_eq!(queue_depth(&store), Ok(2));

    let _ = std::fs::remove_dir_all(&root);
}

// ── the refill ──────────────────────────────────────────────────────────────

use vitals_web::ward_chain::{choose_next, next_patient_id};

fn queued(case: &str, name: &str) -> (String, Pack) {
    let p = Pack {
        difficulty: None,
        case: case.into(),
        persona: Persona { name: name.into(), country: "THA".into(), age: 40, sex: "f".into() },
        portrait: Default::default(),
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
    let intern = queued("ep2", "An Intern Case");   // intern
    let resident = queued("osce-d4", "A Resident Case");  // resident
    let queue = vec![student.clone(), intern.clone(), resident.clone()];

    // An empty ward: nothing is under-represented, so the choice is the same every time rather
    // than arbitrary. A ward that admitted a different patient on each tick would be a ward whose
    // behaviour nobody could reproduce from the same state.
    let empty: Vec<String> = vec![];
    let first = choose_next(&queue, &empty).expect("an empty ward admits somebody");
    assert_eq!(first, choose_next(&queue, &empty).unwrap(), "the same state, the same patient");

    // Two interns already in beds: the student and the resident are what the ward is missing.
    let interns = vec!["ep2".to_string(), "osce-b".to_string()];
    let pick = choose_next(&queue, &interns).expect("a bed to fill");
    assert!(pick == student.0 || pick == resident.0,
            "with two interns on the ward, a third would leave a student with nothing to open");

    // Every band once: the one band with nobody in it wins.
    let one_each = vec!["osce-a".to_string(), "ep2".to_string()];
    let pick = choose_next(&[intern.clone(), resident.clone()], &one_each)
        .expect("a bed to fill");
    assert_eq!(pick, resident.0, "student and intern are held; resident is the empty band");

    // A case already in a bed is not admitted again, whatever else it would balance.
    let on_ward: Vec<String> = vec!["osce-a".into(), "ep2".into(), "osce-d4".into()];
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
///
/// There are three words now (`preview` since 18 ก.ย.) and what each of them allows is in
/// `preview_door.rs`. What is here is the rule that did not change: everything that is not one of
/// those words is a shut door, and `door_open_here` — the question both factory doors ask — is
/// true for the two states that are filling a ward and false for the one that is not.
#[test]
fn the_door_is_shut_unless_somebody_opened_it() {
    use vitals_web::ward_chain::{door_from, Door};

    assert_eq!(door_from(None), Door::Closed, "a deploy that says nothing has a closed door");
    assert_eq!(door_from(Some("open")), Door::Open, "and one word opens it");
    assert_eq!(door_from(Some("OPEN")), Door::Open, "however it is typed");
    assert_eq!(door_from(Some(" open ")), Door::Open, "and with whatever whitespace a shell adds");

    for shut in ["", "closed", "false", "0", "no", "opened", "open the ward", "1", "true"] {
        assert_eq!(door_from(Some(shut)), Door::Closed,
                   "{shut:?} is not one of the three words — anything else leaves it shut, \
                    because the failure that matters is a ward that opened by accident");
        assert!(!door_from(Some(shut)).takes_packs(), "{shut:?}");
    }
    assert!(Door::Open.takes_packs() && Door::Preview.takes_packs(),
            "both of the states that are filling a ward take packs");
}

/// The factory fills in her other states after she is admitted, and cannot overwrite one.
///
/// Packs arrive with `stable` filled; the rest are made at admission, when a bed has actually
/// opened for her, so a patient who never gets worse never costs a picture of her getting worse.
/// They arrive through the same door, keyed by patient, and the rule is **add only**: a portrait
/// already on a patient is one the board may have shown, and a factory that could replace it could
/// change the face of a patient strangers have been treating.
#[test]
fn portraits_are_added_to_a_patient_and_never_replaced() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{fill_portraits, PERSONA_STORE};

    let root = std::env::temp_dir().join(format!("vitals-fill-{}-{:?}", std::process::id(),
                                                 std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(root.clone()).expect("a store");

    let mut pack = a_pack();
    pack.portrait.insert("stable".into(), portrait_url(1));
    store.put(PERSONA_STORE, "p42", &pack).expect("a patient with a base picture");

    let add = |pairs: Vec<(&str, String)>| -> std::collections::BTreeMap<String, String> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    };

    let r = fill_portraits(&store, 42, add(vec![("critical", portrait_url(2)),
                                                ("arrest", portrait_url(3))]));
    assert_eq!(r.added, 2);
    assert_eq!(r.kept, 0);
    assert!(r.rejected.is_empty());

    // The same push again, plus an attempt on a state she already has.
    let r = fill_portraits(&store, 42, add(vec![("critical", portrait_url(9)),
                                                ("stable", portrait_url(9))]));
    assert_eq!(r.added, 0);
    assert_eq!(r.kept, 2, "both were already there, and both stayed as they were");

    let back: Pack = store.get(PERSONA_STORE, "p42").expect("still a patient");
    assert_eq!(back.portrait.get("stable"), Some(&portrait_url(1)),
               "her base picture is the one she was admitted with, not the one pushed later");
    assert_eq!(back.portrait.get("critical"), Some(&portrait_url(2)));
    assert_eq!(back.portrait.len(), 3);

    // A bad key or a bad url takes nothing with it.
    let r = fill_portraits(&store, 42, add(vec![("worse", portrait_url(4)),
                                                ("improving", "https://example.invalid/x.jpg".into())]));
    assert_eq!(r.added, 0);
    assert_eq!(r.rejected.len(), 2, "both named, so an unattended factory can fix them");
    let back: Pack = store.get(PERSONA_STORE, "p42").expect("still a patient");
    assert_eq!(back.portrait.len(), 3, "and nothing was written");

    let r = fill_portraits(&store, 77, add(vec![("stable", portrait_url(1))]));
    assert_eq!(r.added, 0);
    assert!(r.rejected[0].contains("77"),
            "a patient nobody admitted is named in the refusal rather than silently created — a \
             pack with no patient would sit in the store for ever and show nowhere");

    let _ = std::fs::remove_dir_all(&root);
}

// ── the bay, resumed ────────────────────────────────────────────────────────

use std::collections::BTreeMap;
use vitals_replay::{resume as replay_resume, Step, SLOT_SECONDS};
use vitals_web::ward::ShiftOnChain;
use vitals_web::ward_chain::resumed;

fn ep1() -> String {
    std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/sce-anaphylaxis-ep1.json"),
    )
    .expect("ep1 is in the repository")
}

/// A readable stand-in for a run hash: the name's bytes, zero-padded.
fn hash_of(name: &str) -> [u8; 32] {
    let mut h = [0u8; 32];
    for (i, b) in name.bytes().take(32).enumerate() {
        h[i] = b;
    }
    h
}

/// The same hash as the tapes are keyed by — hex, which is what the chain hands back.
fn hex_of(name: &str) -> String {
    vitals_web::ward_chain::hex32(&hash_of(name))
}

fn anchored(name: &str, slot: u64) -> ShiftOnChain {
    ShiftOnChain { patient_id: 42, signer: [1; 32], slot, run_hash: hash_of(name) }
}

fn seen(st: &vitals_sce::runtime::SceState) -> (Option<String>, String, usize) {
    (st.outcome().map(|o| format!("{o:?}")), format!("{:.3}", st.t_sec()), st.harm_events.len())
}

/// **The chain decides what happened to her, and in what order.**
///
/// Not our store. The shifts come from the AnchorShift transactions on her account, in slot order,
/// and each tape is looked up by the `run_hash` the leaf commits to — so a tape we hold that was
/// never anchored is never replayed, and a tape we have lost stops the rebuild with a sentence
/// instead of quietly producing a different patient.
///
/// That is "no server can change her past without every browser noticing", made literal on our own
/// server: every input to this function is either on the chain or is bytes the chain committed to.
/// The timing too — the idle clock runs anchor to anchor, which is arithmetic anybody can repeat,
/// rather than from a clock reading we kept to ourselves.
#[test]
fn the_chain_decides_what_happened_to_her_and_in_what_order() {
    let sce = ep1();
    let first = vec![Step::Tick(30.0), Step::Do("oxygen".into()), Step::Tick(45.0)];
    let second = vec![Step::Tick(20.0), Step::Do("adrenaline im".into()), Step::Tick(60.0)];
    let mut tapes: BTreeMap<String, Vec<Step>> = BTreeMap::new();
    tapes.insert(hex_of("one"), first.clone());
    tapes.insert(hex_of("two"), second.clone());
    tapes.insert(hex_of("never-anchored"), vec![Step::Do("stand her up".into()), Step::Tick(60.0)]);
    let chart = |h: &str| tapes.get(h).cloned();

    // Nobody has been yet.
    let (fresh, n) = resumed(&sce, &[], &chart, 1_000_000, 1_000_000, &dated).expect("an unvisited patient");
    let (start, _) = replay_resume(&sce, &[]).expect("the scenario's start");
    assert_eq!(seen(&fresh), seen(&start));
    assert_eq!(n, 0);

    // Two shifts, anchored back to back. The chain of shifts equals the whole tape.
    let two = [anchored("one", 1_000_010), anchored("two", 1_000_020)];
    let (rebuilt, n) = resumed(&sce, &two, &chart, 1_000_000, 1_000_020, &dated).expect("two shifts");
    let whole: Vec<Step> = first.iter().chain(&second).cloned().collect();
    let (one_tape, _) = replay_resume(&sce, &whole).expect("one tape of both");
    // The chain of shifts equals the whole tape **plus the idle the chain's own gaps buy** — ten
    // slots from her admission to the first anchor and ten between the anchors. Not a tolerance: a
    // number, because every part of it is arithmetic a stranger repeats from two slot numbers.
    let bought = vitals_replay::idle_sim_seconds(10.0 * vitals_replay::SLOT_SECONDS) * 2.0;
    assert!((rebuilt.t_sec() - (one_tape.t_sec() + bought)).abs() < 1e-6,
            "the chain of shifts must equal the whole plus its own gaps — {} against {} + {bought}",
            rebuilt.t_sec(), one_tape.t_sec());
    assert_eq!(seen(&rebuilt).0, seen(&one_tape).0, "and the same outcome");
    assert_eq!(seen(&rebuilt).2, seen(&one_tape).2, "and the same harm");
    assert_eq!(n, 2);

    // A tape we hold but the chain never anchored is not part of her past.
    let (same, _) = resumed(&sce, &two, &chart, 1_000_000, 1_000_020, &dated).expect("two shifts again");
    assert_eq!(seen(&same), seen(&rebuilt),
               "the store holds a third tape, and it changes nothing — only anchored work counts");

    // A tape the chain names and we cannot produce stops the rebuild, in words.
    let missing = [anchored("one", 1_000_010), anchored("gone", 1_000_020)];
    let err = match resumed(&sce, &missing, &chart, 1_000_000, 1_000_020, &dated) {
        Err(e) => e,
        Ok(_) => panic!("a tape the chain names and we cannot produce must stop the rebuild"),
    };
    assert!(err.contains("cannot be rebuilt") || err.contains("missing"),
            "a lost tape must say so rather than produce a patient nobody can check: {err}");

    // The idle clock runs anchor to anchor, and from her admission to the first anchor, and from
    // the last anchor to now — three spans, all of them chain arithmetic.
    let a_night = (10.0 * 3600.0 / SLOT_SECONDS) as u64;
    let apart = [anchored("one", 1_000_010), anchored("two", 1_000_010 + a_night)];
    let (after_a_night, _) = resumed(&sce, &apart, &chart, 1_000_000, 1_000_010 + a_night, &dated).expect("a night apart");
    assert_ne!(seen(&after_a_night), seen(&rebuilt),
               "ten hours between two anchors is time she spent untreated");

    let (waiting, _) = resumed(&sce, &two, &chart, 1_000_000, 1_000_020 + a_night, &dated).expect("nobody since");
    assert_ne!(seen(&waiting), seen(&rebuilt),
               "and so is ten hours since the last stranger left");

    let (admitted_early, _) = resumed(&sce, &two, &chart, 1_000_000 - a_night, 1_000_020, &dated).expect("admitted early");
    assert_ne!(seen(&admitted_early), seen(&rebuilt),
               "a patient nobody came to for ten hours after she was admitted is not the patient \
                the first stranger would have found at once");
}

// ── what the chain says no to ───────────────────────────────────────────────

use vitals_web::ward_chain::{commit_ix, open_account_ix, refusal};

/// A refusal is a sentence, not an error code.
///
/// Every one of these is a thing that happens on a working ward with strangers in it: somebody
/// worked from a state that moved, somebody walked into a room that is taken, somebody came to a
/// patient who went home last night. The program refuses each by its own code, and a person
/// standing at a bed cannot read `custom program error: 0x10`.
///
/// This is also the demo. The refusal is the part of the ward that proves the chain is deciding
/// rather than the server, so it has to be legible when it happens.
#[test]
fn a_refusal_is_a_sentence_a_person_can_act_on() {
    let stale = refusal("Error processing Instruction 0: custom program error: 0x10")
        .expect("StaleHead is 16");
    assert!(stale.contains("moved") || stale.contains("somebody else"),
            "it has to say what happened to her, not what the program is called: {stale}");

    let held = refusal("… custom program error: 0x11").expect("LeaseHeld is 17");
    assert!(held.to_lowercase().contains("someone") || held.to_lowercase().contains("somebody"),
            "somebody is already in the room: {held}");

    let closed = refusal("… custom program error: 0x12").expect("PatientClosed is 18");
    assert!(closed.contains("left the ward") || closed.contains("stay"), "{closed}");

    let not_holder = refusal("… custom program error: 0x13").expect("NotLeaseHolder is 19");
    assert!(!not_holder.is_empty());

    assert!(refusal("connection refused").is_none(),
            "an outage is not the program refusing anything, and calling it one would tell a \
             stranger their work was rejected when it was never sent");
    assert!(refusal("custom program error: 0x1").is_none(),
            "a code this ward does not know stays unexplained rather than guessed at");
}

/// The two instructions a browser needs before it can play, built the program's way.
#[test]
fn a_stranger_opens_an_account_and_declares_before_playing() {
    let program = Pubkey::new_unique();
    let operator = Pubkey::new_unique();
    let player = Pubkey::new_unique();

    let open = open_account_ix(&program, &operator, &player);
    assert_eq!(open.program_id, program);
    assert!(matches!(Instruction::deserialize(&mut &open.data[..]), Ok(Instruction::OpenAccount)));
    assert_eq!(open.accounts[0].pubkey, operator, "the relay pays for the account");
    assert!(open.accounts[0].is_signer);
    assert_eq!(open.accounts[1].pubkey, player, "and the key that will play signs for itself");
    assert!(open.accounts[1].is_signer);

    let hash = [7u8; 32];
    let declare = commit_ix(&program, &operator, &player, hash);
    match Instruction::deserialize(&mut &declare.data[..]).expect("decodes") {
        Instruction::Commit { hash: h } => assert_eq!(h, hash,
            "the declaration binds the case before the outcome is known — that is what makes it a \
             declaration rather than a claim"),
        other => panic!("declaring must be Commit, not {other:?}"),
    }
    assert_eq!(declare.accounts.len(), 5, "the program's own five");
    assert!(declare.accounts[0].is_signer && declare.accounts[1].is_signer,
            "the relay pays and the player declares");
}

/// One unreadable entry in her history must not blind the ward — and must not be skipped past.
///
/// Devnet handed back a null where a transaction should have been, and the read failed whole:
/// "patient 1789490621's history could not be read". The next read was fine, so the ward was
/// briefly unreadable for a hiccup — but the fix has to be careful in a way the bug was not.
///
/// History arrives newest-first. If a null in the middle were simply skipped, the cursor would
/// move past it and the shift underneath it would never be read again: a shift that happened,
/// paid for, anchored on chain, and absent from the census for ever. So the page is walked
/// **oldest-first and stops at the first entry it cannot read**: everything before it is kept, the
/// cursor stops there, and the next read starts again from exactly that point.
#[test]
fn an_unreadable_entry_stops_the_walk_rather_than_being_skipped() {
    use vitals_web::ward::ShiftOnChain;
    use vitals_web::ward_chain::{walk_history, Budget, Seen};

    // Newest first, the way getSignaturesForAddress answers.
    let page = vec![("s5".to_string(), 500u64), ("s4".into(), 400), ("s3".into(), 300),
                    ("s2".into(), 200), ("s1".into(), 100)];
    let shift_at = |slot: u64| ShiftOnChain {
        patient_id: 42, signer: [1; 32], slot, run_hash: [slot as u8; 32],
    };

    // s3 is the null. s1 and s2 are read; s4 and s5 are not reached.
    let read = |sig: &str, slot: u64| -> Result<Vec<ShiftOnChain>, String> {
        match sig {
            "s3" => Err("invalid type: null, expected struct".into()),
            // Not every signature is a shift — taking the head is a transaction too.
            "s2" => Ok(vec![]),
            _ => Ok(vec![shift_at(slot)]),
        }
    };
    // Read to the end: this test is about the entry that cannot be read, not about a budget.
    let (got, cursor, trouble) = walk_history(page.clone(), &Budget::whole_history(), read);

    assert_eq!(got.len(), 1, "only s1 produced a shift, and s2 produced none");
    assert_eq!(cursor.as_ref().map(|(s, _)| s.as_str()), Some("s2"),
               "the cursor stops at the newest entry that was fully read — including one that was \
                read and held no shift, or every lease would be re-fetched for ever");
    let trouble = trouble.expect("the walk says why it stopped");
    assert!(trouble.contains("s3"), "and names the entry it stopped at: {trouble}");

    // Absorbing keeps the cursor the walk chose, rather than deriving it from the shifts.
    let mut seen = Seen::default();
    seen.absorb(got.into_iter().map(|s| (s, "sig".to_string())).collect(), cursor);
    assert_eq!(seen.until().as_deref(), Some("s2"),
               "so the next read begins at s3 again and the shift under it is not lost");
}

/// Putting a shift down is a thing a stranger must be able to do.
///
/// Producer's ruling, 16 ก.ย., after I kept holding beds open by walking away from my own tests:
/// there has to be a way to hand her back. "I have to go" happens more often on a public ward
/// than "I have finished", and without it one closed laptop holds a bed for the length of a lease.
///
/// Only the holder may put it down while it stands — otherwise anyone could clear anyone's shift
/// — and the relay is not in the instruction at all, exactly as taking it is not.
#[test]
fn the_head_can_be_handed_back_by_the_person_holding_it() {
    use vitals_web::ward_chain::release_shift_ix;

    let program = Pubkey::new_unique();
    let operator = Pubkey::new_unique();
    let player = Pubkey::new_unique();

    let ix = release_shift_ix(&program, &operator, &player, 42);
    match Instruction::deserialize(&mut &ix.data[..]).expect("decodes") {
        Instruction::ReleaseShift { patient_id } => assert_eq!(patient_id, 42),
        other => panic!("handing her back must be ReleaseShift, not {other:?}"),
    }
    assert_eq!(ix.accounts.len(), 3, "the player, who they are, and the patient");
    assert_eq!(ix.accounts[0].pubkey, player);
    assert!(ix.accounts[0].is_signer, "only the person holding it may put it down");
    assert!(ix.accounts[2].is_writable, "the lease is written on her");
    assert!(!ix.accounts.iter().any(|a| a.pubkey == operator),
            "the relay pays for this and takes no part in it — the same bargain as taking it");
}

/// A face may be fixed while she is waiting, and never once she is in a bed.
///
/// Producer's ruling from the factory's second tick: a base portrait came back wrong — a doll
/// rather than a person — on a pack still in the queue. While she is waiting, nobody has seen her,
/// so any key may be replaced. The moment she is admitted the add-only rule holds: a face the
/// board has shown is one strangers have been treating, and changing it underneath them is the
/// thing add-only exists to prevent.
///
/// The pack's address does not move when a portrait does, which is what makes this safe: portraits
/// were deliberately left out of the content hash, so fixing a face is not a different patient.
#[test]
fn a_queued_face_may_be_replaced_and_an_admitted_one_may_not() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{enqueue, pack_id, replace_queued_portraits, QUEUE_STORE};

    let root = std::env::temp_dir().join(format!("vitals-face-{}-{:?}", std::process::id(),
                                                 std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(root.clone()).expect("a store");

    let mut pack = a_pack();
    pack.portrait.insert("stable".into(), portrait_url(1));
    let id = pack_id(&pack);
    assert_eq!(enqueue(&store, vec![pack.clone()]).queued, 1);

    // The doll is replaced while she waits.
    let fixed = replace_queued_portraits(
        &store, &id,
        [("stable".to_string(), portrait_url(2))].into_iter().collect());
    assert_eq!(fixed.added, 1, "a key that was there is replaced rather than kept");
    assert!(fixed.rejected.is_empty());
    let back: Pack = store.get(QUEUE_STORE, &id).expect("still queued");
    assert_eq!(back.portrait.get("stable"), Some(&portrait_url(2)));
    assert_eq!(pack_id(&back), id,
               "and she is the same patient — a portrait is not part of what names a pack, which \
                is what makes fixing one safe");

    // The same validation. A key the engine cannot report, or a url from anywhere else, is refused.
    let bad = replace_queued_portraits(
        &store, &id,
        [("worse".to_string(), portrait_url(3)),
         ("critical".to_string(), "https://example.invalid/x.jpg".into())].into_iter().collect());
    assert_eq!(bad.added, 0);
    assert_eq!(bad.rejected.len(), 2);

    // Nobody by that address is waiting — she may have been admitted since.
    let gone = replace_queued_portraits(
        &store, &"f".repeat(64),
        [("stable".to_string(), portrait_url(1))].into_iter().collect());
    assert_eq!(gone.added, 0);
    assert!(gone.rejected[0].contains("waiting"),
            "the refusal says she is not in the queue, so the factory knows to use the patient \
             route and that the rule there is add-only: {:?}", gone.rejected);

    let _ = std::fs::remove_dir_all(&root);
}

// ── the shift receipt ───────────────────────────────────────────────────────

use vitals_web::ward::Pack as WardPack;
use vitals_web::ward_chain::receipt;

// ── the chain's clock, for tests written in slots ──────────────────────────
/// The chain's clock, for tests written in slots.
///
/// Every gap below is a number of slots, and the ward no longer multiplies those by anything: it
/// asks the chain when each block was produced. So the tests answer as a chain running at the
/// nominal 0.4 s a slot would — the spans mean exactly what they always meant here, and what
/// changed is where the ward gets them from. (The real devnet was at 0.166 s on 17 ก.ย., which is
/// the whole reason this is asked rather than assumed.)
fn dated(slot: u64) -> Option<i64> {
    (slot != 0).then(|| 1_789_000_000 + (slot as i64 * 2) / 5)
}

/// **A shift can be checked by somebody who never played it.**
///
/// The receipt is the ward's answer to "why should anyone believe you". It carries what the chain
/// holds — whose key, which patient, which head it extended, at which slot — and what anybody can
/// recompute from the tape: the beats, the harm this shift added, and the deterministic score for
/// this shift's own actions.
///
/// **The judged 60 is not on it, and the page says why.** A judged score belongs to a finished
/// case; a shift is a few minutes in the middle of somebody's stay. Publishing a number that
/// cannot mean what a reader assumes is the failure this product exists not to commit.
///
/// The harm is this shift's own, never what it inherited — a stranger is answerable for what they
/// did, not for what they walked into.
#[test]
fn a_shift_receipt_carries_what_the_chain_holds_and_what_anybody_can_recompute() {
    let sce = ep1();
    let before = vec![Step::Tick(30.0), Step::Do("stand her up".into()), Step::Tick(30.0)];
    let mine = vec![Step::Tick(20.0), Step::Do("oxygen".into()), Step::Tick(40.0)];

    let mut tapes: BTreeMap<String, Vec<Step>> = BTreeMap::new();
    tapes.insert(hex_of("first"), before.clone());
    tapes.insert(hex_of("mine"), mine.clone());
    let chart = |h: &str| tapes.get(h).cloned();

    let pack = WardPack {
        difficulty: None,
        case: "ep1".into(),
        persona: Persona { name: "Ing".into(), country: "THA".into(), age: 19, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    let shifts = [anchored("first", 1_000_010), anchored("mine", 1_000_020)];

    let r = receipt(&sce, None, &shifts, &shifts[1], &chart, &pack, 1_000_000, &dated)
        .expect("a receipt for a shift the chain names");

    assert_eq!(r["patient_id"], 42);
    assert_eq!(r["shift"], 2, "the second link in her chain");
    assert_eq!(r["run_hash"], hex_of("mine"));
    assert_eq!(r["slot"], 1_000_020, "when the chain says it landed");
    assert!(r["player"].as_str().is_some_and(|s| !s.is_empty()), "whose key played it");

    let did = &r["did"];
    assert!(did["beats"].as_u64().is_some());
    assert_eq!(did["steps"], mine.len());
    assert_eq!(did["harm"].as_array().map(|h| h.len()).unwrap_or(9), 0,
               "standing her up was the shift before this one, and this stranger did not do it");

    assert!(r["judged"].is_null(), "no judged score on a shift");
    let why = r["judged_omitted"].as_str().expect("and it says why rather than leaving a hole");
    /* It said why at length — "a judged score belongs to a finished case…" — and the director cut
       it to one line on 18 ก.ย.: a receipt is a record, and the reasoning belongs in the plan where
       somebody looking for it can find it. What the payload still has to do is say *what* is absent
       and *where*, in a sentence short enough for the strip's own rule. */
    assert!(why.contains("mid-stay"),
            "it has to say where the marks are missing from, not only that they are: {why}");
    assert!(why.split_whitespace().count() <= 12, "one line on a receipt: {why}");

    // The tape is offered, addressed by the hash the leaf commits to, so a stranger can replay it
    // without asking us for anything.
    assert_eq!(r["tape"], format!("/api/tape/{}", hex_of("mine")));
    let how = r["derivations"]["det"].as_str().expect("the score says how it was got");
    assert!(how.contains("rubric") || how.contains("recompute"), "{how}");
}

/// A receipt says so when its hash names more than one shift.
///
/// A run hash is the hash of the **tape**, and two strangers who did exactly the same things to
/// the same case produce the same bytes — I watched it happen the first time two of my own test
/// shifts played the same three orders. Their leaves differ, because a leaf carries the player and
/// the commitment; the tape hash does not.
///
/// So a receipt addressed by tape hash can name several shifts, and one that showed the first and
/// said nothing would be telling a reader "this is the shift" when the truth is "this is one of
/// three". The number is on the receipt, and so is where the others are.
#[test]
fn a_receipt_says_when_its_hash_names_more_than_one_shift() {
    let sce = ep1();
    let same = vec![Step::Tick(20.0), Step::Do("oxygen".into()), Step::Tick(40.0)];
    let mut tapes: BTreeMap<String, Vec<Step>> = BTreeMap::new();
    tapes.insert(hex_of("same"), same);
    let chart = |h: &str| tapes.get(h).cloned();

    let pack = WardPack {
        difficulty: None,
        case: "ep1".into(),
        persona: Persona { name: "Ing".into(), country: "THA".into(), age: 19, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    // Two shifts, different keys, the same bytes.
    let mut second = anchored("same", 1_000_030);
    second.signer = [9; 32];
    let shifts = [anchored("same", 1_000_010), second];

    let r = receipt(&sce, None, &shifts, &shifts[0], &chart, &pack, 1_000_000, &dated).expect("a receipt");
    assert_eq!(r["also_anchored"], 1,
               "one other shift on this ward has the same tape, and the receipt says so rather \
                than presenting itself as the only one");
    let note = r["also_anchored_note"].as_str().expect("and says what that means");
    assert!(note.contains("same tape") || note.contains("same bytes"), "{note}");

    // A hash that names exactly one shift says nothing, because there is nothing to say.
    let alone = receipt(&sce, None, &shifts[..1], &shifts[0], &chart, &pack, 1_000_000, &dated).unwrap();
    assert_eq!(alone["also_anchored"], 0);
    assert!(alone["also_anchored_note"].is_null());
}


/// **The board loads a thumbnail; the bedside loads the picture.**
///
/// Twenty patients on a globe is twenty full-size portraits over a mobile connection for a board
/// nobody has clicked yet, and the ward pays for every byte of it. The factory now makes a 256 px
/// sibling of each face — same object, same sha, `-256` in the name — and a pack carries both
/// under `<state>` and `<state>_256`.
///
/// The pairing is checked rather than trusted. A `_256` key holding a full-size address is the
/// bug this rule exists to stop: it looks right in the JSON, draws correctly on the board, and
/// quietly undoes the whole change.
#[test]
fn a_pack_may_carry_the_small_sibling_of_every_face() {
    let mut p = a_pack();
    p.portrait.insert("stable".into(), portrait_url(1));
    p.portrait.insert("stable_256".into(), small_url(1));
    p.portrait.insert("critical_256".into(), small_url(2));
    validate_pack(&p).expect("both sizes of a state the engine reports");

    let mut wrong_size = a_pack();
    wrong_size.portrait.insert("stable_256".into(), portrait_url(1));
    assert!(validate_pack(&wrong_size).is_err(),
            "a _256 key holding a full-size address draws correctly and defeats the whole point");

    let mut wrong_key = a_pack();
    wrong_key.portrait.insert("stable".into(), small_url(1));
    assert!(validate_pack(&wrong_key).is_err(),
            "and the plain key must hold the full-size one, for the same reason in reverse");

    let mut dead = a_pack();
    dead.portrait.insert("dead_256".into(), small_url(1));
    assert!(validate_pack(&dead).is_err(),
            "no picture of a dead patient is made in either size");

    let mut nonsense = a_pack();
    nonsense.portrait.insert("worse_256".into(), small_url(1));
    assert!(validate_pack(&nonsense).is_err(), "and the state still has to be one the engine reports");

    // The address shape, at the door that decides what a page may load.
    use vitals_web::ward_chain::is_portrait_url;
    assert!(is_portrait_url(&small_url(1)), "the sibling is a portrait this ward publishes");
    for wrong in [
        portrait_url(1).replace(".webp", "-512.webp"),
        portrait_url(1).replace(".webp", "-256.png"),
        portrait_url(1).replace(".webp", "-256"),
        portrait_url(1).replace(".webp", "-0256.webp"),
    ] {
        assert!(!is_portrait_url(&wrong), "{wrong} is not one of the two shapes the ward publishes");
    }
}

/// The small ones arrive through the same door, under the same add-only rule.
#[test]
fn the_small_siblings_are_added_to_a_patient_like_any_other_face() {
    use std::collections::BTreeMap;
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{fill_portraits, PERSONA_STORE};

    let dir = std::env::temp_dir().join(format!("vitals-small-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");
    let mut pack = a_pack();
    pack.portrait.insert("stable".into(), portrait_url(1));
    store.put(PERSONA_STORE, "p7", &pack).expect("admit her");

    let mut add = BTreeMap::new();
    add.insert("stable_256".to_string(), small_url(1));
    add.insert("critical_256".to_string(), small_url(2));
    let filled = fill_portraits(&store, 7, add);
    assert_eq!(filled.added, 2, "both siblings land: {:?}", filled.rejected);
    assert!(filled.rejected.is_empty(), "{:?}", filled.rejected);

    let mut bad = BTreeMap::new();
    bad.insert("stable_256".to_string(), portrait_url(1));
    assert_eq!(fill_portraits(&store, 7, bad).added, 0,
               "a full-size address under a _256 key is refused here too, not only at the queue");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── the patient nobody came back to ─────────────────────────────────────────

/// **Somebody has to close her, and it must not be the next stranger.**
///
/// The founder removed the idle cap on 16 ก.ย.: a patient nobody visits deteriorates as the engine
/// says, and can arrest and die with nobody in the room. That leaves a question the ward has to
/// answer rather than discover — *when is a death that happened in an idle span written down?*
///
/// His ruling: the ticker, never a stranger. It replays idle time for every open bed each minute
/// and, when the engine has reached death, anchors a closing shift with an empty tape and the idle
/// span, so the chain reads *died, nobody on shift* — and the next person to open her page finds a
/// closed patient rather than a corpse the board still calls alive.
///
/// This is the decision half: pure, chain-derived, and the same arithmetic a stranger would do.
#[test]
fn a_patient_nobody_came_back_to_is_closed_by_the_ward_and_not_by_the_next_stranger() {
    use vitals_web::ward_chain::died_unattended;

    let sce = ep1();
    let chart = |_: &str| Some(Vec::<Step>::new());

    // Admitted, never visited, and long enough ago that the engine has finished her. EP1 arrests
    // at 518 simulated seconds untended, which at 1:60 is a little under nine real hours.
    let admitted = 1_000_000u64;
    let nine_hours = (9.0 * 3600.0 / vitals_replay::SLOT_SECONDS) as u64;
    let closed = died_unattended(&sce, &[], &chart, admitted, admitted + nine_hours, &dated)
        .expect("the chain reads")
        .expect("nine hours alone finishes EP1 — the whole point of the founder's ruling");
    assert!(closed.outcome.to_lowercase().contains("death"),
            "the engine's own word for it, carried rather than restated: {}", closed.outcome);
    assert_eq!(closed.idle_slots, nine_hours,
               "the span is the chain's arithmetic: admission to now, in slots");
    assert_eq!(closed.since_slot, admitted,
               "and it runs from her admission, because no shift has been anchored on her");
    assert!(closed.replay.steps == 0,
            "an empty tape — nobody did anything to her, and the record must not imply otherwise");

    // An hour is an hour. She is worse, and she is alive, and the ticker leaves her alone.
    let one_hour = (3600.0 / vitals_replay::SLOT_SECONDS) as u64;
    assert!(died_unattended(&sce, &[], &chart, admitted, admitted + one_hour, &dated)
                .expect("the chain reads")
                .is_none(),
            "a patient who is merely deteriorating is not a patient to close");

    // The span runs from the last anchor rather than from admission — somebody was with her at the
    // end of it. Her chart still carries the hour before that shift, because that hour happened:
    // what moves is where the *next* span is measured from.
    let seen_at = admitted + one_hour;
    let recent = anchored("one", seen_at);
    let after = died_unattended(&sce, std::slice::from_ref(&recent), &chart, admitted, seen_at + one_hour, &dated)
        .expect("the chain reads");
    assert!(after.is_none(), "an hour after a shift is an hour — two real hours has not killed her");

    let long_after = died_unattended(&sce, &[recent], &chart, admitted, seen_at + nine_hours, &dated)
        .expect("the chain reads")
        .expect("nine hours after the last shift is nine hours");
    assert_eq!(long_after.since_slot, seen_at, "measured from the last anchor");
    assert_eq!(long_after.idle_slots, nine_hours);

    // A tape the chain names and we have lost stops the reading. The alternative is closing a
    // patient on a chart nobody can rebuild, which is the one thing the ward may not do.
    let missing = anchored("never-stored", admitted + one_hour);
    let err = died_unattended(&sce, &[missing], &|_| None, admitted, admitted + nine_hours * 2, &dated);
    assert!(err.is_err(),
            "her chart cannot be rebuilt, so the ward says so rather than closing her on a guess");
}

/// **A receipt address this ward never played is refused without reading the chain.**
///
/// `/shift/<64 hex>` answered in 29 to 102 seconds on staging (measured 17 ก.ย., three times, on a
/// hash nobody has ever anchored), and the ward serves requests one at a time — so any stranger
/// with a URL bar could hold the whole ward, board and beds and all, for a minute and a half. The
/// cause is in `find_shift`: when the cached pass finds nothing it walks *every patient* again,
/// refreshing each one's history from the RPC, which is twenty-odd round trips for an answer that
/// was always going to be "no shift on this ward has that hash".
///
/// The refresh exists for one real case: this ward anchored a shift a moment ago and its own cache
/// has not caught up. That case has a tell — the tape is here, kept under the hash the leaf commits
/// to, because this ward is what kept it. So the tape is the ticket to the chain walk, and a hash
/// with no tape behind it is refused from the store alone.
///
/// Nothing is lost by it. A receipt is rebuilt *from* the tape; a shift whose tape this ward does
/// not hold cannot be rendered even when the chain confirms it exists, which is what the board
/// already says in words about its unrebuildable patients.
#[test]
fn a_receipt_address_this_ward_never_played_is_refused_without_reading_the_chain() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{keep_tape, worth_reading_the_chain_for, StoredTape};

    let root = std::env::temp_dir().join(format!("vitals-receipt-{}-{:?}", std::process::id(),
                                                 std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(root.clone()).expect("a store");

    let ours = "a".repeat(64);
    let strangers = "9f2c1e5a7b3d4c6e8a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f6071";
    keep_tape(&store, &StoredTape { patient_id: 7, run_hash: ours.clone(), steps: vec![] })
        .expect("the tape is kept");

    assert!(worth_reading_the_chain_for(&store, &ours),
            "this ward played it and holds the tape, so a cache that has not caught up is worth \
             one read of the chain");
    assert!(!worth_reading_the_chain_for(&store, strangers),
            "a hash nobody here has ever played is answered from the store, in milliseconds");
    assert!(!worth_reading_the_chain_for(&store, "zzz"), "and a thing that is not a hash at all");

    let _ = std::fs::remove_dir_all(&root);
}

/// **The same real hour is the same patient, whatever the chain's slot rate.**
///
/// The ward turned a gap between two shifts into simulated time by multiplying the slot count by
/// 0.4 s. Devnet spent 17 ก.ย. producing slots at 0.166 s — measured four ways against getBlockTime
/// — so a patient left alone for ten real minutes was replayed as though twenty-four had passed,
/// and the ticker's unattended deaths came 2.4× too early. Nobody would have seen it in a test:
/// the arithmetic was self-consistent and wrong about the world.
///
/// Two chains here, one producing slots at 0.4 s and one at 0.166 s. The same *real* hour leaves
/// the same patient on both, and the same *slot* gap leaves different ones — which is the whole
/// change in one pair of assertions.
#[test]
fn the_same_real_hour_is_the_same_patient_on_any_chain() {
    use vitals_web::ward_chain::resumed;
    let sce = ep1();
    let chart = |_: &str| Some(Vec::new());
    let admitted = 1_000_000u64;

    // Two clocks. `nominal` is 0.4 s a slot, `devnet` is what the chain was really doing.
    let nominal = |slot: u64| (slot != 0).then(|| 1_789_000_000 + (slot as i64 * 2) / 5);
    let devnet = |slot: u64| (slot != 0).then(|| 1_789_000_000 + (slot as i64 * 166) / 1000);

    // One real hour on each chain: 9,000 slots at 0.4 s, 21,687 at 0.166 s.
    let (a, _) = resumed(&sce, &[], &chart, admitted, admitted + 9_000, &nominal).expect("an hour");
    let (b, _) = resumed(&sce, &[], &chart, admitted, admitted + 21_687, &devnet).expect("an hour");
    assert!((a.t_sec() - b.t_sec()).abs() < 1.0,
            "an hour is an hour: {} vs {} simulated seconds", a.t_sec(), b.t_sec());

    // The same slot gap on the two chains is not the same span, and must not leave the same
    // patient — this is the bug, stated as an inequality.
    let (fast, _) = resumed(&sce, &[], &chart, admitted, admitted + 9_000, &devnet).expect("a gap");
    assert!(fast.t_sec() < a.t_sec() - 1.0,
            "nine thousand slots is an hour on one chain and twenty-five minutes on the other, and \
             the patient has to be the one the clock says: {} vs {}", fast.t_sec(), a.t_sec());

    // A slot this ward cannot date advances her by nothing rather than by a guess.
    let (undated, _) = resumed(&sce, &[], &chart, admitted, admitted + 9_000, &|_| None)
        .expect("a chain this ward cannot date");
    assert_eq!(undated.t_sec(), 0.0,
               "no block time, no idle time: the ticker asks again a minute later with the block \
                times cached, and nothing false is written down in between");
}

/// **A head that moved because *we* anchored it says so.**
///
/// `StaleHead` has two stories and the ward told one of them for both: "somebody else anchored a
/// shift on the head you were extending". After a double press of Hand over that somebody else is
/// us — the head on chain is the leaf this shift just filed — and the sentence accused a stranger
/// of losing a race they had won. Demo capture, item 2.
///
/// So the refusal is read against the head the chain is holding now. If that head is this shift's
/// own leaf, the shift is anchored and the words say exactly that; if it is anybody else's, or the
/// chain cannot be read at the moment of the refusal, the general sentence stands rather than a
/// guess.
#[test]
fn a_head_this_shift_moved_itself_is_not_somebody_elses() {
    use vitals_web::ward_chain::anchor_refusal;
    let stale = "Error processing Instruction 0: custom program error: 0x10";
    let ours = [7u8; 32];

    let mine = anchor_refusal(stale, Some(ours), ours).expect("StaleHead is 16");
    assert!(mine.contains("already anchored"),
            "the words the founder has to read are that this shift is already anchored: {mine}");

    let theirs = anchor_refusal(stale, Some([9u8; 32]), ours).expect("StaleHead is 16");
    assert!(!theirs.contains("already anchored"),
            "a head somebody else moved is still somebody else's: {theirs}");
    assert_eq!(theirs, refusal(stale).unwrap(),
               "and it is the same sentence the ward already had");

    assert_eq!(anchor_refusal(stale, None, ours), refusal(stale),
               "a chain that could not be read at the moment of the refusal does not get to \
                claim the head is ours");

    let held = "… custom program error: 0x11";
    assert_eq!(anchor_refusal(held, Some(ours), ours), refusal(held),
               "every other refusal passes through untouched — only the stale head has two stories");
    assert!(anchor_refusal("connection refused", Some(ours), ours).is_none(),
            "an outage is not a refusal here either");
}

/// **The chain dates every slot it hands us, and the ward used to throw the dates away.**
///
/// A receipt opened seconds after its own anchor took 21–61 s on staging with the page silent, and
/// the shape of it is two passes over the same facts: `getSignaturesForAddress` answers with a
/// block time beside every signature, the walk drops them, and then `slot_times` asks the chain for
/// those same times one slot per round trip — measured at 0.11–1.03 s each against public devnet on
/// 18 ก.ย., up to `DATE_AT_MOST` of them, in front of a reader looking at nothing.
///
/// So the ward keeps what it is given, at the moment it is given it. Written once, because a
/// block's time is decided when the block is produced: a second answer for a slot is either the
/// same answer or a wrong one.
#[test]
fn the_ward_keeps_the_block_times_the_chain_already_gave_it() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{cached_dater, learn_slot_times};

    let dir = std::env::temp_dir().join(format!("vitals-times-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");

    // A page of signatures as the chain hands it back: a slot, and a time it may or may not know.
    let page = [
        (500_000_001u64, Some(1_758_000_000i64)),
        (500_000_002, None),
        (500_000_003, Some(1_758_000_042)),
    ];
    assert_eq!(
        learn_slot_times(&store, &page), 2,
        "the two the chain dated are kept, and the one it did not is not guessed at"
    );

    let dated = cached_dater(&store);
    assert_eq!(dated(500_000_001), Some(1_758_000_000));
    assert_eq!(dated(500_000_003), Some(1_758_000_042));
    assert_eq!(dated(500_000_002), None,
               "an undated slot stays undated rather than becoming zero — a guessed date would \
                write a deterioration nobody can check");

    let again = [(500_000_001u64, Some(1_758_000_999i64))];
    assert_eq!(learn_slot_times(&store, &again), 0, "nothing new to learn");
    assert_eq!(cached_dater(&store)(500_000_001), Some(1_758_000_000),
               "and the first answer stands: a block's time does not change");

    // Slot 0 is not a slot. It is what a patient carries when the ward never learned when she was
    // admitted, and dating it would put her admission at the epoch.
    assert_eq!(learn_slot_times(&store, &[(0, Some(1_758_000_000))]), 0);
    assert_eq!(cached_dater(&store)(0), None);

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The one slot no transaction dates is the one she is opened at.**
///
/// Every other slot on a patient's chain is dated by the call that found it. `now_slot` is not: no
/// transaction happened in it, so the ward used to ask `getBlockTime` — a round trip, with a reader
/// waiting, to be told the time. The answer to "what time is the slot the chain is on right now" is
/// *now*, and the honest failure here is the alternative: a span counted as zero leaves a patient
/// exactly as the last shift left her, however long she has been alone, which is the bug the dating
/// exists to prevent.
#[test]
fn the_slot_the_chain_is_on_now_is_dated_by_the_wards_own_clock() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{dater_to_now, learn_slot_times};

    let dir = std::env::temp_dir().join(format!("vitals-now-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");
    learn_slot_times(&store, &[(500_000_001, Some(1_758_000_000))]);

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dated = dater_to_now(&store, 500_000_009);

    let now = dated(500_000_009).expect("the slot the chain is on is dated");
    assert!((now - now_unix).abs() <= 5, "and it is dated now, not at the epoch: {now}");
    assert_eq!(dated(500_000_001), Some(1_758_000_000), "every other slot is the store's own");
    assert_eq!(dated(500_000_002), None, "and a slot nothing has dated is still undated");
    assert_eq!(dated(0), None, "slot 0 is not a slot, even when the ward is standing on it");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The last board outlives the instance that read it.**
///
/// A derived value with its own `as_of`, so keeping it is safe and serving a minute-old one is a
/// fact a reader can see. What must not happen is the two failure shapes:
///
///   * a `ward_unavailable` — the answer given when the chain could not be read — overwriting a
///     good board, which would turn one bad minute into a permanently empty ward;
///   * a build serving a board whose payload shape it does not understand. The stored record
///     carries a version for exactly that, and a mismatch is refused rather than parsed hopefully.
#[test]
fn the_last_board_outlives_the_instance_that_read_it() {
    use vitals_web::store::Store;
    use vitals_web::ward_chain::{keep_board, last_board, BOARD_VERSION};

    let dir = std::env::temp_dir().join(format!("vitals-board-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");

    assert!(last_board(&store).is_none(), "a ward that has never read the chain has no board");

    let good = serde_json::json!({ "readable": true, "as_of_slot": 500_283_135, "patients": [] });
    assert!(keep_board(&store, &good, "vitals-world-00051-4rd"), "a readable board is kept");
    let kept = last_board(&store).expect("and comes back");
    assert_eq!(kept.board["as_of_slot"], 500_283_135, "as the board it was, with its own as_of");
    assert!(kept.age.as_secs() < 5, "dated by when it was kept: {:?}", kept.age);
    assert_eq!(kept.revision, "vitals-world-00051-4rd",
               "and it names the revision that read it, so a slow first request can be attributed \
                to a deploy rather than guessed at");

    let bad = serde_json::json!({ "readable": false, "why": "devnet said no" });
    assert!(!keep_board(&store, &bad, "vitals-world-00051-4rd"),
            "a ward that could not read its chain has not read a board");
    assert_eq!(last_board(&store).expect("the good one stands").board["as_of_slot"], 500_283_135,
               "an outage must never displace the last thing this ward actually saw");

    // A build that does not know this shape refuses it rather than parsing it hopefully.
    let mut wrong = store
        .get::<serde_json::Value>(vitals_web::ward_chain::BOARD_STORE, "last")
        .expect("the stored record");
    wrong["version"] = serde_json::json!(BOARD_VERSION + 1);
    store.put(vitals_web::ward_chain::BOARD_STORE, "last", &wrong).expect("write it back");
    assert!(last_board(&store).is_none(), "a board from a shape this build does not know is not served");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A receipt says what was done, not what the id for it is.**
///
/// The demo capture's receipt reads "0:39 ordered `tx_oxygen`", "0:17 asked
/// `ask_chest_abdominal_and_flank_pain`". Those are intervention ids, and they are on the tape on
/// purpose: the page sends the id because that is what the case is keyed by, what the matcher rules
/// on, and what a verifier re-runs — the same run in any language. None of that is a reason to show
/// them to a reader.
///
/// The case carries the words beside the id (`sce.interventions[].label`) and the receipt is built
/// with the case in hand, so it can print the words and keep the id. An order the case has no label
/// for keeps what the tape says: a receipt that invented a phrase for an id nobody wrote would be
/// worse than one that shows the id.
#[test]
fn a_receipt_names_what_was_done_and_keeps_the_id() {
    use vitals_web::ward_chain::label_for;

    let sce = serde_json::json!({
        "interventions": [
            { "id": "tx_oxygen", "label": "Oxygen by face mask, 15 L/min" },
            { "id": "ask_chest_abdominal_and_flank_pain", "label": "Asked about: chest and flank pain" },
            { "id": "tx_nolabel" },
        ]
    })
    .to_string();

    assert_eq!(label_for(&sce, "tx_oxygen").as_deref(), Some("Oxygen by face mask, 15 L/min"),
               "the case's own words for its own intervention");
    assert_eq!(label_for(&sce, "ask_chest_abdominal_and_flank_pain").as_deref(),
               Some("Asked about: chest and flank pain"));
    assert_eq!(label_for(&sce, "tx_nolabel"), None,
               "an intervention with no words keeps the id rather than being given a phrase");
    assert_eq!(label_for(&sce, "tx_nothing_like_it"), None,
               "and an id this case never wrote is not guessed at");
    assert_eq!(label_for("{}", "tx_oxygen"), None, "a case with no interventions says nothing");
    assert_eq!(label_for("not json at all", "tx_oxygen"), None,
               "and an unreadable case is not an excuse to invent one");
}

/// **A receipt never reads an id out loud, even when the case wrote no words for it.**
///
/// `label_for` covers the cases that carry labels, which is all of the factory's. What is left is
/// the case that does not — a season station, an older pack, an order the author never named — and
/// there the receipt printed the tape's own text, which for an intervention is its id:
/// "0:47 ORDERED tx_source_control".
///
/// The id is not a phrase to invent around; it is words already, with the compiler's row prefix on
/// the front and underscores between. So it is read as what it is. A typed order is not an id and
/// is not touched — "oxygen face mask 15 lpm" is what somebody wrote and what the tape kept.
#[test]
fn a_receipt_never_reads_an_id_out_loud() {
    use vitals_web::ward_chain::id_as_words;

    assert_eq!(id_as_words("tx_source_control"), "source control");
    assert_eq!(id_as_words("ask_chest_abdominal_and_flank_pain"), "chest abdominal and flank pain");
    assert_eq!(id_as_words("exam_neck"), "neck");
    assert_eq!(id_as_words("ix_cxr"), "cxr");
    assert_eq!(id_as_words("dx_boerhaave"), "boerhaave");

    assert_eq!(id_as_words("oxygen face mask 15 lpm"), "oxygen face mask 15 lpm",
               "a typed order is not an id: it is what somebody wrote, and the tape kept it");
    assert_eq!(id_as_words("shock"), "shock", "one word with no prefix is already words");
    assert_eq!(id_as_words("tx_"), "", "a prefix and nothing else says nothing, rather than \"tx_\"");
    assert_eq!(id_as_words("weird_id_no_prefix"), "weird id no prefix",
               "and an id from a vocabulary this ward does not know still opens its underscores");
}

/// **The words a stranger reads on a receipt are never an id.**
///
/// The director's rule, and the assertion is on the words rather than on the document: the id stays
/// beside them in small monospace, because a receipt is a thing a stranger checks and the id is what
/// they would check it with. What must never happen is the id standing *as* the words, which is what
/// "0:47 ORDERED tx_source_control" was.
///
/// Added after the fix at the director's request, and verified against the old behaviour before
/// being kept: with the fallback removed, `said` comes back as the raw id and this fails.
#[test]
fn the_words_a_stranger_reads_are_never_an_id() {
    let sce = ep1();
    let mine = vec![
        Step::Tick(20.0),
        Step::Do("tx_source_control".into()),
        Step::Tick(10.0),
        Step::Ask("ask_chest_and_flank_pain".into()),
    ];
    let mut tapes: BTreeMap<String, Vec<Step>> = BTreeMap::new();
    tapes.insert(hex_of("mine"), mine.clone());
    let chart = |h: &str| tapes.get(h).cloned();
    let pack = WardPack {
        difficulty: None,
        case: "ep1".into(),
        persona: Persona { name: "Ing".into(), country: "THA".into(), age: 19, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    let shifts = [anchored("mine", 1_000_020)];
    let r = receipt(&sce, None, &shifts, &shifts[0], &chart, &pack, 1_000_000, &dated)
        .expect("a receipt");

    let rows = r["timeline"].as_array().expect("a timeline");
    assert_eq!(rows.len(), 2, "one order and one question: {rows:?}");
    for row in rows {
        let said = row["said"].as_str().expect("every row carries the words it is read as");
        for raw in ["tx_", "ask_", "ix_", "dx_", "exam_"] {
            assert!(!said.contains(raw),
                    "the words a stranger reads are an id: {said:?} in {row:?}");
        }
        assert!(!said.contains('_'), "and no underscore survives into them: {said:?}");
    }
    assert_eq!(rows[0]["said"], "source control");
    assert_eq!(rows[1]["said"], "chest and flank pain");

    // The id itself stays, because that is what somebody checking this would check it with.
    assert_eq!(rows[0]["text"], "tx_source_control");
    assert_eq!(rows[1]["text"], "ask_chest_and_flank_pain");
}

/// **A label does not say which row it is in twice.**
///
/// The compiler prefixes every intervention label with its row — "Ask: …", "Examine: …" — and the
/// tray strips it, because under a tab that already says ASK twenty chips opening with "Ask:" are
/// twenty chips a learner reads the fourth word of (`chipText`). The receipt did not strip it, so
/// its timeline read "asked · Ask: Chest, abdominal and flank pain" — the row said twice, once by
/// the receipt and once by the label.
#[test]
fn a_label_does_not_repeat_the_row_it_is_in() {
    use vitals_web::ward_chain::label_for;

    let sce = serde_json::json!({
        "interventions": [
            { "id": "ask_flank", "label": "Ask: Chest, abdominal and flank pain" },
            { "id": "exam_neck", "label": "Examine: Subcutaneous emphysema in the lower neck" },
            { "id": "ix_cxr", "label": "Order: Chest X-ray (erect PA)" },
            { "id": "tx_o2", "label": "Give: Oxygen by face mask, 15 L/min" },
            { "id": "tx_plain", "label": "Crystalloid bolus, reassessed" },
            { "id": "dx_odd", "label": "Diagnosis: Boerhaave syndrome" },
        ]
    })
    .to_string();

    assert_eq!(label_for(&sce, "ask_flank").as_deref(), Some("Chest, abdominal and flank pain"));
    assert_eq!(label_for(&sce, "exam_neck").as_deref(),
               Some("Subcutaneous emphysema in the lower neck"));
    assert_eq!(label_for(&sce, "ix_cxr").as_deref(), Some("Chest X-ray (erect PA)"));
    assert_eq!(label_for(&sce, "tx_o2").as_deref(), Some("Oxygen by face mask, 15 L/min"));
    assert_eq!(label_for(&sce, "tx_plain").as_deref(), Some("Crystalloid bolus, reassessed"),
               "a label with no prefix is untouched");
    assert_eq!(label_for(&sce, "dx_odd").as_deref(), Some("Diagnosis: Boerhaave syndrome"),
               "and a word the tray does not strip is not stripped here either — one rule, in one \
                place, and `chipText` is where it is written");
}

/// **A slow pass says how the time was spread, because that is what names the cause.**
///
/// 00063: the first ticker pass took 100.1 s over the same 26 patients that boot's repair had
/// walked in 38.3 s four minutes earlier — same chain, same data, same code. Two candidate causes
/// with opposite fixes:
///
///   * Cloud Run allocates CPU only while a request is being processed, so a background thread on
///     an idle instance is throttled. Then **every patient costs the same slowed amount**.
///   * devnet rate-limits the signature listings and the retries back off. Then **most patients
///     are quick and a few are very slow**.
///
/// A total cannot tell those apart and neither can a mean — a mean of 3.8 s is what you get from
/// twenty-six patients at 3.8 s and from twenty-four at 0.4 s plus two at 44 s. The median against
/// the max separates them on sight, which is the whole reason this line exists rather than a
/// stopwatch on the total.
#[test]
fn a_slow_pass_says_how_the_time_was_spread() {
    use std::time::Duration;
    use vitals_web::ward_chain::{pace, slow_pass_note, Pace, SLOW_PASS};

    assert_eq!(pace(&[]), None, "a pass that walked nobody has no shape to report");
    assert_eq!(pace(&[700]), Some(Pace { patients: 1, median_ms: 700, max_ms: 700 }));
    assert_eq!(pace(&[100, 200, 300]), Some(Pace { patients: 3, median_ms: 200, max_ms: 300 }),
               "odd: the middle one");
    assert_eq!(pace(&[100, 200, 300, 400]), Some(Pace { patients: 4, median_ms: 250, max_ms: 400 }),
               "even: the two middle ones averaged");
    assert_eq!(pace(&[300, 100, 200]), Some(Pace { patients: 3, median_ms: 200, max_ms: 300 }),
               "patients arrive in the order the chain lists them, not in order of cost");

    // The two shapes, as they would actually arrive.
    let throttled = pace(&[3_800; 26]).expect("26 patients");
    assert_eq!((throttled.median_ms, throttled.max_ms), (3_800, 3_800));
    let mut limited = vec![400u64; 24];
    limited.extend([44_000, 44_000]);
    let limited = pace(&limited).expect("26 patients");
    assert_eq!((limited.median_ms, limited.max_ms), (400, 44_000),
               "same 100 s total, and the pair says at a glance which of the two it was");

    // Nothing is said about a pass that was not slow. A line every minute is a line nobody reads,
    // and the ticker runs on a ward that is usually quiet.
    assert_eq!(slow_pass_note(Duration::from_secs(9), Some(throttled), &[]), None,
               "under the threshold the pass is silent");
    assert_eq!(slow_pass_note(SLOW_PASS - Duration::from_millis(1), Some(throttled), &[]), None);
    assert_eq!(slow_pass_note(Duration::from_secs(100), None, &[]), None,
               "and a pass with nobody to walk is silent however long it took — the time went to \
                the sweep or the refill, and a per-patient figure over zero patients is a lie");

    let said = slow_pass_note(Duration::from_millis(100_100), Some(throttled), &[])
        .expect("a pass over the threshold says something");
    assert!(said.contains("100.1s"), "the duration, one decimal: {said}");
    assert!(said.contains("26 patients checked"), "how many it walked: {said}");
    assert!(said.contains("3800ms") || said.contains("3.8s"),
            "the median, so the shape can be read: {said}");
    assert!(said.to_lowercase().contains("median") && said.to_lowercase().contains("max"),
            "both named, because the reader is comparing them: {said}");

    let lumpy = slow_pass_note(Duration::from_millis(100_100), Some(limited), &[]).expect("also slow");
    assert_ne!(said, lumpy, "the two shapes cannot print the same line");

    // And the line carries the pass's own parts, so "which patients" and "which part of the pass"
    // are answered by one reading. 00065 needed both: the per-patient shape accounted for three
    // seconds of a 28 s pass, and only the spans could say where the rest went.
    use vitals_web::ward_chain::Span;
    let with_parts = slow_pass_note(Duration::from_millis(28_300), Some(throttled),
                                    &[Span { what: "repair", ms: 3_100 },
                                      Span { what: "reap", ms: 24_000 }])
        .expect("slow");
    assert!(with_parts.contains("repair 3.1s · reap 24.0s · elsewhere 1.2s"),
            "the parts and the unaccounted remainder ride on the same line: {with_parts}");
}

/// **A patient the chain has not moved is not listed.**
///
/// 00064: the pass spent 99.8 s over 26 patients, median 674 ms and max 10.7 s — devnet rate-limits
/// the signature listings and roughly half of them sat in a multi-second backoff. The cheapest fix
/// is not to make the listings faster but to stop making most of them.
///
/// The chain already says how many leaves a patient has: `PatientOnChain::shifts`, read for every
/// patient in the one `chain.patients()` call the tick makes anyway. So the question costs nothing
/// that is not already being paid, and nothing is kept in memory between passes — deliberately,
/// because staging redeploys on every commit and an in-memory note would be empty on exactly the
/// pass that matters, the first one after a start.
///
/// Both halves are needed. The count alone would miss a tape that disappeared from the store while
/// the chain stood still, which is the failure the repair exists for in the first place.
#[test]
fn a_patient_the_chain_has_not_moved_is_not_listed() {
    use vitals_web::ward_chain::needs_listing;

    assert!(!needs_listing(3, 3, true),
            "three leaves on chain, three in the cache, every tape here — a listing cannot add \
             anything, and asking costs ten seconds when devnet is in a mood");
    assert!(!needs_listing(0, 0, true), "and a patient nobody has treated yet is not listed either");

    assert!(needs_listing(4, 3, true), "a new leaf on chain: her cache is behind and must catch up");
    assert!(needs_listing(3, 3, false),
            "the counts agree and a tape is gone from the store — the case the repair exists for, \
             and the reason the count alone is not the whole condition");
    assert!(needs_listing(4, 3, false), "both at once is still listed, once");
    assert!(needs_listing(3, 4, true),
            "more in the cache than the chain says exist is an anomaly, and an anomaly is listed \
             rather than trusted — a duplicate in the cache would otherwise hide a real leaf");
}

/// **The parts of a pass add up to the pass, and the line says so when they do not.**
///
/// 00065, after the listings were skipped: 28.3 s over 26 patients, median 36 ms, max 1025 ms. The
/// per-patient shape accounts for about three seconds of it. The other twenty-five are somewhere
/// the instrument cannot see, and a number nobody can attribute is the exact shape of the two
/// incidents this project has already had — `boot meter +137.6s` and `boot sessions +30.2s`, both
/// true, both read as something they were not, both an hour lost.
///
/// So the spans are named *and* reconciled: whatever the named parts do not account for is printed
/// as `elsewhere`, which is the line admitting what it does not know. A future span added to the
/// pass and left unnamed shows up there rather than silently inflating its neighbour.
#[test]
fn the_parts_of_a_pass_add_up_to_the_pass() {
    use std::time::Duration;
    use vitals_web::ward_chain::{spans_line, Span};

    let named = [Span { what: "repair", ms: 3_100 }, Span { what: "reap", ms: 24_000 }];

    assert_eq!(spans_line(Duration::from_millis(27_100), &named), "repair 3.1s · reap 24.0s",
               "when the parts account for the whole, nothing is added");
    assert_eq!(spans_line(Duration::from_millis(28_300), &named),
               "repair 3.1s · reap 24.0s · elsewhere 1.2s",
               "and when they do not, the gap is named rather than left for a reader to subtract");

    // Under a second reads in milliseconds: a span of "0.1s" and a span of "0.0s" look alike and
    // one of them is forty times the other.
    assert_eq!(spans_line(Duration::from_millis(80), &[Span { what: "queue", ms: 80 }]), "queue 80ms");
    assert_eq!(spans_line(Duration::from_millis(3), &[Span { what: "queue", ms: 3 }]), "queue 3ms");

    // Occurrence order, like the boot marks, because the reader is following the pass through.
    let ordered = [Span { what: "patients", ms: 200 }, Span { what: "packs", ms: 1_500 },
                   Span { what: "repair", ms: 50 }];
    assert_eq!(spans_line(Duration::from_millis(1_750), &ordered),
               "patients 200ms · packs 1.5s · repair 50ms");

    // Rounding must not manufacture an `elsewhere`. Three spans of 1 ms against a 4 ms pass is not
    // a finding, and a line that cried about it every minute would be a line nobody reads.
    let dust = [Span { what: "a", ms: 1 }, Span { what: "b", ms: 1 }, Span { what: "c", ms: 1 }];
    assert_eq!(spans_line(Duration::from_millis(4), &dust), "a 1ms · b 1ms · c 1ms");

    assert_eq!(spans_line(Duration::from_millis(500), &[]), "",
               "a pass with no named parts says nothing rather than claiming it is all elsewhere");
}

/// **One pass, one listing per patient — and the repair is the only place that asks.**
///
/// 00066 spent 35.5 s: `lost` 15.7 s and `reap` 14.6 s, next to a `repair` of 4.3 s that had just
/// skipped 24 of its 26 listings. The reason was not subtle. `lost_tapes` opens its loop with
/// `let _ = chain.refresh(...)` and `reap` does the same a few lines later, so the listing item 1
/// skipped was being made twice more, unconditionally, on every open patient. Eight listings went
/// out in one burst and devnet's limiter charges the later ones hardest — which is also why two
/// passes over identical work differed by seven seconds.
///
/// By the time either runs, the cache is current: `repair_tapes` refreshed it for any patient it
/// listed, and for the ones it skipped the skip's own premise is that the chain's leaf count already
/// matches the cache. So nobody needs to ask again inside one pass. This is `cb23269`'s own rule —
/// *"the question asked once and shared … the boot, the ticker and the board all ask it of the same
/// shifts and cannot answer it differently"* — applied to the listing rather than to the tapes.
///
/// **Why this is a source test and `boot.rs`'s "patients checked" grep was not.** That one was a
/// proxy for something a real test could reach, so it was deleted when the real test arrived. This
/// rule cannot be driven without a validator — gates skips the chain gate — and the counting is the
/// whole invariant, so reading the source is the only executable form it has. Scoped to the two
/// functions in the ticker's pass: `read_ward` and `find_shift` refresh legitimately, once each,
/// for a board read and a single-patient lookup, and this must not forbid them.
#[test]
fn one_pass_asks_the_chain_about_a_patient_at_most_once() {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ward_chain.rs"))
        .expect("src/ward_chain.rs");

    // A function's source, from its signature to the next one that starts at column zero.
    let body = |name: &str| -> String {
        let at = src.find(name).unwrap_or_else(|| panic!("{name} is not in ward_chain.rs"));
        let rest = &src[at + name.len()..];
        let end = rest.find("\nfn ").unwrap_or(rest.len())
            .min(rest.find("\npub fn ").unwrap_or(rest.len()));
        rest[..end].to_string()
    };

    for asked in ["fn lost_tapes(", "fn reap("] {
        assert!(!body(asked).contains("chain.refresh("),
                "{asked} lists a patient's signatures again — the repair already did it this pass, \
                 and on a rate-limited endpoint the second and third asks wear the backoff the \
                 first one earned");
    }

    assert!(body("fn repair_one(").contains("chain.refresh("),
            "the repair is where a pass asks the chain, and it asks only when `needs_listing` says \
             the answer could have changed");
}

/// **A history read only as far as it got is kept, and never decided on.**
///
/// `walk_history` has always stopped at the first entry it could not read and kept everything
/// before it — the test above pins that. What `refresh` did with the answer is the defect:
///
///     let added = seen.absorb(named, cursor);
///     if let Some(why) = trouble { eprintln!("…read as far as it could be — {why}"); }
///     Ok(added)
///
/// `trouble` was logged and swallowed, so `refresh` returned `Ok` for a history read to its end and
/// `Ok` for one that stopped a third of the way through, and no caller could tell them apart. The
/// consequence is not cosmetic: `repair_one` persisted the partial history and marked her
/// confirmed, and `reap` was then free to decide she died unattended on a history missing her most
/// recent shifts — a chain write, on a reading the function's own comment says is incomplete. Under
/// rate limiting the walk stops early more often, so the failure gets likelier exactly when the
/// ward is busiest.
///
/// **Persistence and trust are separate decisions**, which is the whole rule here. The progress is
/// always kept — it was paid for in round trips, and throwing it away means the next pass re-walks
/// the same transactions from the same place, for ever. The trust is withheld whenever the history
/// is not known whole.
#[test]
fn a_partial_history_is_kept_and_never_decided_on() {
    use std::collections::{BTreeMap, BTreeSet};
    use vitals_web::ward_chain::{may_close, Cached, Reading, Seen};

    let whole = Reading { added: 3, stopped: None };
    let partial = Reading { added: 2, stopped: Some("s3: invalid type: null".into()) };
    assert!(whole.whole(), "a history read to its end can be decided on");
    assert!(!partial.whole(),
            "and one that stopped cannot — the shifts past the stop are the recent ones, and a \
             patient missing her recent shifts is a patient who looks idle");

    // The pass's own record of who it may write about.
    let mut cached = Cached { seen: BTreeMap::new(), unread: BTreeSet::new() };
    cached.seen.insert(1, Seen::default());
    cached.seen.insert(2, Seen::default());
    cached.unread.insert(2);

    assert!(may_close(&cached, 1), "a patient whose history this pass read whole");
    assert!(!may_close(&cached, 2),
            "a patient whose history failed or stopped short — her cache is still there to answer \
             local questions, and it may not end her stay");
    assert!(!may_close(&cached, 3),
            "and a patient this pass has no reading of at all is not a patient to close: absence \
             of a history is not evidence of an idle one");
}

/// **A pass spends only so long on one patient's history, and the rest is the next pass's.**
///
/// 00067: 69.2 s, of which `repair` was 66.9 s — two listings, one of them 51 s. The listing itself
/// is a single call; the time is in what follows it, one `get_transaction` per signature with
/// nothing bounding how many. One patient with a long unread history holds the whole ward.
///
/// **Two legs, because a count alone does not bound a pass.** Unthrottled a transaction is ~40 ms;
/// throttled it is ~1 s. A budget of 200 entries is 8 s on a good day and 200 s on a bad one — and
/// the bad day is the one this exists for. The clock is what bounds the pass; the entry count keeps
/// one absurd history from queueing behind a clock that has not run out yet.
///
/// `spent` takes `now` rather than reading it, so the whole decision is pure: no sleeping, no
/// validator, and the two legs can be tested at the boundary rather than near it.
#[test]
fn a_pass_spends_only_so_long_on_one_history() {
    use std::time::{Duration, Instant};
    use vitals_web::ward_chain::Budget;

    let now = Instant::now();
    let b = Budget { entries: 200, until: Some(now + Duration::from_secs(10)) };

    assert_eq!(b.spent(0, now), None, "nothing read yet and the clock has not started");
    assert_eq!(b.spent(199, now), None, "199 of 200, with time in hand");
    assert_eq!(b.spent(3, now - Duration::from_secs(1)), None,
               "the clock leg reads the `now` it is given, not the one the budget was made at");

    let entries = b.spent(200, now).expect("the entry leg is spent at 200");
    assert!(entries.contains("200"), "it says how many it read: {entries}");

    let clock = b.spent(3, now + Duration::from_secs(11)).expect("the clock leg has run out");
    assert!(clock.contains('3'), "and how far it got when the time went: {clock}");

    assert_ne!(entries, clock,
               "the two legs read differently — a history too long and a chain too slow are \
                different facts about the ward and the log is where somebody tells them apart");

    // A caller that must read to the end says so, and then only the entry leg can stop it.
    let no_clock = Budget { entries: 2, until: None };
    assert_eq!(no_clock.spent(1, now + Duration::from_secs(600)), None,
               "no deadline means no deadline, however long the caller has been at it");
    assert!(no_clock.spent(2, now).is_some());

    // **It has to converge, or `may_close` never lets a long history end a stay.** The walk goes
    // oldest-first and the cursor stops at the newest entry fully read, so each pass resumes where
    // the last stopped: 200 signatures at 50 a pass is four passes, and then nothing is left over
    // and the reading is whole. A budget that did not advance the cursor would have turned item 1's
    // refusal into a patient who can never be closed at all.
    let small = Budget { entries: 50, until: None };
    let mut left = 200usize;
    let mut passes = 0;
    while left > 0 {
        let took = (1..=left).take_while(|n| small.spent(n - 1, now).is_none()).count();
        assert!(took > 0, "a pass that reads nothing is a pass that never converges");
        left -= took;
        passes += 1;
        assert!(passes <= 8, "four expected, and anything unbounded is the bug this guards");
    }
    assert_eq!(passes, 4);
}

/// **A budget stop and an unreadable entry leave the walk in the same state.**
///
/// The point of putting the budget in the walk rather than beside it: the partial-read path already
/// existed, was already tested by the test above, and is now — since `Reading` — already distrusted
/// for writes and already persisted. A budget is the deliberate way into it. If a budget stop left a
/// different state behind, that would be a second path with a second set of bugs.
#[test]
fn a_budget_stop_leaves_the_walk_where_a_failure_would() {
    use vitals_web::ward::ShiftOnChain;
    use vitals_web::ward_chain::{walk_history, Budget, Seen};

    let page = vec![("s5".to_string(), 500u64), ("s4".into(), 400), ("s3".into(), 300),
                    ("s2".into(), 200), ("s1".into(), 100)];
    let shift_at = |slot: u64| ShiftOnChain {
        patient_id: 42, signer: [1; 32], slot, run_hash: [slot as u8; 32],
    };
    let mut asked = Vec::new();
    let read = |sig: &str, slot: u64| -> Result<Vec<ShiftOnChain>, String> {
        asked.push(sig.to_string());
        Ok(vec![shift_at(slot)])
    };

    // Two transactions, then the pass's turn for this patient is over.
    let (got, cursor, stopped) =
        walk_history(page.clone(), &Budget { entries: 2, until: None }, read);

    assert_eq!(asked, ["s1", "s2"], "oldest first, and it stopped before paying for a third");
    assert_eq!(got.len(), 2);
    assert_eq!(cursor.as_ref().map(|(s, _)| s.as_str()), Some("s2"),
               "the cursor is the newest entry fully read — the same rule as a failure stop, so \
                the next pass begins at s3 and nothing is walked twice");
    let why = stopped.expect("a stop says why");
    assert!(why.contains("transactions"), "and says it was a budget, not a broken entry: {why}");

    // Which is what makes it converge: absorb, and the next listing starts after s2.
    let mut seen = Seen::default();
    seen.absorb(got.into_iter().map(|s| (s, "sig".to_string())).collect(), cursor);
    assert_eq!(seen.until().as_deref(), Some("s2"));
}

/// **One patient's history failing to refresh does not black out the board.**
///
/// Staging, 20 ก.ย. from 14:26 UTC: `/api/ward` answered `readable: false` for over an hour with
/// `why: "patient 1789488342's history could not be read: 429 Too Many Requests"`. No patients, no
/// beds, nothing — because `read_ward` did `return unavailable(...)` inside its per-patient loop.
/// One rate-limited signature listing on one patient, and twenty-six correct accounts that had just
/// been read in a single successful `getProgramAccounts` were thrown away with it. The unreadable
/// board then went into the served view for the next minute.
///
/// Her *account* was read — that is how her id is known at all. Her state, bed, lease and shift
/// count are correct. Only her history (the signature walk, for the chart) is stale. So the board
/// keeps going: her cached history as it stands, her row flagged, everyone else fresh. The same
/// rule as the ticker's: **persistence and trust are separate**, and a failed listing withholds trust
/// from one history rather than from the whole ward.
///
/// And it asks before it pays, the way the pass does since `daeeea3`: a patient whose chain count
/// matches the cache and whose tapes are present is never listed at all. On a quiet ward the board
/// rebuild then makes zero listings, and there is nothing for the free endpoint to 429.
#[test]
fn one_patients_failed_history_does_not_black_out_the_board() {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use vitals_web::store::Store;
    use vitals_web::ward::{PatientOnChain, ShiftOnChain, OPEN};
    use vitals_web::ward_chain::{histories, keep_tape, Reading, Seen, StoredTape, SHIFT_CACHE};

    let dir = std::env::temp_dir().join(format!("vitals-histories-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");

    // One cached shift each, with its tape present, so "tapes all present" is true for everybody
    // and the only thing deciding a listing is the chain's count against the cache's.
    let hex = |b: u8| -> String { (0..32).map(|_| format!("{b:02x}")).collect() };
    let seed = |id: u64, b: u8| {
        let shift = ShiftOnChain { patient_id: id, signer: [1; 32], slot: 100, run_hash: [b; 32] };
        let mut seen = Seen::default();
        seen.absorb(vec![(shift, "sig1".to_string())], Some(("sig1".to_string(), 100)));
        store.put(SHIFT_CACHE, &format!("p{id}"), &seen).expect("cached");
        keep_tape(&store, &StoredTape { patient_id: id, run_hash: hex(b), steps: vec![] })
            .expect("tape kept");
    };
    seed(1, 0x11);
    seed(2, 0x22);
    seed(3, 0x33);

    let on_chain = |id: u64, shifts: u32| PatientOnChain {
        patient_id: id, state: OPEN, shifts, admitted_slot: 50, closed_slot: 0,
        lease_holder: [0; 32], lease_until_slot: 0,
    };
    // 1: the chain says one shift and the cache has one — nothing to ask.
    // 2: the chain says two — her cache is behind, so she is listed, and the listing succeeds.
    // 3: the chain says five — listed, and the listing is the 429.
    let patients = [on_chain(1, 1), on_chain(2, 2), on_chain(3, 5)];

    let asked = RefCell::new(Vec::new());
    let h = histories(&store, &patients, |id, seen: &mut Seen| {
        asked.borrow_mut().push(id);
        match id {
            2 => {
                let extra = ShiftOnChain { patient_id: 2, signer: [1; 32], slot: 200, run_hash: [0x2a; 32] };
                seen.absorb(vec![(extra, "sig2".to_string())], Some(("sig2".to_string(), 200)));
                Ok(Reading { added: 1, stopped: None })
            }
            _ => Err("HTTP status client error (429 Too Many Requests)".into()),
        }
    });

    assert_eq!(*asked.borrow(), vec![2, 3],
               "patient 1 is never asked about: the chain's count matched the cache and every tape \
                was present, so a listing could not have told the board anything");
    assert_eq!(h.listed, 2);

    // The board is built from all three — the failure took one history's freshness, not the ward.
    let ids: HashSet<u64> = h.shifts.iter().map(|s| s.patient_id).collect();
    assert_eq!(ids, [1, 2, 3].into_iter().collect(),
               "every patient's shifts are on the board, including the one whose listing failed");
    assert_eq!(h.shifts.iter().filter(|s| s.patient_id == 2).count(), 2,
               "and patient 2's fresh shift is there, because her listing worked");
    assert_eq!(h.shifts.iter().filter(|s| s.patient_id == 3).count(), 1,
               "patient 3 has her cached shift — stale, kept, and not pretended to be more");

    // The failure is named against the patient, not raised against the ward.
    assert_eq!(h.unread.len(), 1);
    let why = h.unread.get(&3).expect("patient 3 is the one that could not be refreshed");
    assert!(why.contains("429"), "and the reason is the chain's own words: {why}");

    // Her fresh cache was persisted; the failed one was left as it was.
    let two: Seen = store.get(SHIFT_CACHE, "p2").expect("cache");
    assert_eq!(two.shifts().len(), 2, "a successful listing is kept for the next read");
    let three: Seen = store.get(SHIFT_CACHE, "p3").expect("cache");
    assert_eq!(three.shifts().len(), 1, "a failed one leaves the cache untouched");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **When there is no fresh board at all, the last kept one is served, and says so.**
///
/// The other half of the same incident. `histories` covers one patient failing; this covers
/// `getProgramAccounts` itself failing, when there is no fresh account for anybody. Today that is
/// `unavailable` — no patients, no beds. But `keep_board` has kept every readable board this host
/// ever built, so there is almost always a board a minute or two old that is far more true than
/// "nothing". Served with `from: store`, its own `kept_at`, and the error named — never relabelled
/// as fresh, never pretended to be nothing.
#[test]
fn with_no_fresh_board_the_kept_one_is_served_and_says_so() {
    use vitals_web::ward_chain::{fall_back, Kept};

    let queue = serde_json::json!({ "waiting": 4 });
    let kept = Kept {
        age: std::time::Duration::from_secs(90),
        at_unix: 1_789_915_933,
        revision: "vitals-world-00067".into(),
        board: serde_json::json!({
            "readable": true, "patients": [{ "patient_id": 7 }], "census": { "open": 1 },
            "board": { "from": "chain", "kept_at": 1_789_915_933, "kept_by": null },
            "queue": { "waiting": 2 }
        }),
    };

    let served = fall_back(Some(kept), "devnet:ABC", queue.clone(), "the chain could not be read: 429");
    assert_eq!(served["readable"], true, "a kept board is a readable board");
    assert_eq!(served["patients"][0]["patient_id"], 7, "with the patients it had");
    assert_eq!(served["board"]["from"], "store", "and it says where it came from");
    assert_eq!(served["board"]["kept_at"], 1_789_915_933_u64,
               "with the time it was actually read, not the time it was served — an age ticks and \
                a ticking field is a new ETag every second");
    assert_eq!(served["board"]["kept_by"], "vitals-world-00067");
    assert!(served["stale"].as_str().unwrap_or("").contains("429"),
            "and the reason a fresh one could not be had: {}", served["stale"]);
    assert_eq!(served["queue"], queue,
               "the queue is this host's own fact and is current whatever the chain did");

    let nothing = fall_back(None, "devnet:ABC", queue, "the chain could not be read: 429");
    assert_eq!(nothing["readable"], false,
               "with nothing kept there is nothing to serve, and the board says so rather than \
                inventing one");
    assert!(nothing["why"].as_str().unwrap_or("").contains("429"));
}

/// **A second pass asked for while one is running is refused, never queued.**
///
/// The pass writes to the chain — `reap` anchors closing shifts — so two of them at once could
/// close the same patient twice. When the Scheduler's request finds the in-process ticker (or an
/// earlier request) mid-pass, the answer is 409 with a sentence, and the caller tries again next
/// minute. Never a wait: a request that blocks holds the ward's only request thread, which is the
/// thing this whole day has been about not doing.
///
/// Pure at the seam so the two answers can be pinned without a race in the test: the route hands
/// this whatever the gate gave it.
#[test]
fn a_second_pass_is_refused_not_queued() {
    use vitals_web::ward_chain::{tick_response, Ticked};

    let (code, body) = tick_response(None, std::time::Duration::from_millis(0));
    assert_eq!(code, 409, "the gate was held");
    assert!(body["error"].as_str().unwrap_or("").contains("already running"),
            "and the caller is told why, in words: {body}");

    let ran = Ticked { checked: 26, listed: 2, ..Default::default() };
    let (code, body) = tick_response(Some(ran), std::time::Duration::from_millis(2_140));
    assert_eq!(code, 200);
    assert_eq!(body["took_ms"], 2_140, "how long the pass took, for the Scheduler's log");
    assert_eq!(body["patients"], 26);
    assert_eq!(body["listed"], 2, "and how many cost a listing — the number the rate limit sees");
    assert!(body.get("pace").is_some() && body.get("spans").is_some() && body.get("notes").is_some(),
            "the same facts the slow-pass line prints: {body}");
}
