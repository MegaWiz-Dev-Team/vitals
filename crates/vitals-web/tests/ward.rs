//! The ward's arithmetic, written before the ward.
//!
//! Two things are being pinned here and they fail differently. The beds decide whether the ward
//! keeps running when nobody is watching — get that wrong and it quietly stops admitting, which
//! looks exactly like nobody came. The census decides what the weekly video says out loud — get
//! that wrong and we read a wrong number to a judge with a citation next to it, which is worse
//! than having no number at all.
//!
//! Every figure here is derived from what the chain says, never from a counter the server keeps:
//! admitted, went home and died come from the patient accounts; shifts and keys from the anchored
//! leaves; and *on the ward* is subtraction, never its own tally — a separate count is a second
//! source of truth, and the moment the two disagree there is no way to tell which is lying.

use vitals_web::ward::{census, to_admit, PatientOnChain, ShiftOnChain, BEDS};

fn patient(id: u64, state: u8, shifts: u32, admitted: u64, closed: u64) -> PatientOnChain {
    PatientOnChain { patient_id: id, state, shifts, admitted_slot: admitted, closed_slot: closed }
}

fn shift_by(patient_id: u64, signer: u8, slot: u64) -> ShiftOnChain {
    ShiftOnChain { patient_id, signer: [signer; 32], slot }
}

const OPEN: u8 = 0;
const DISCHARGED: u8 = 1;
const DIED: u8 = 2;

#[test]
fn the_census_is_read_off_the_chain_and_on_the_ward_is_subtraction() {
    let patients = vec![
        patient(1, DISCHARGED, 4, 10, 90),
        patient(2, DIED, 2, 20, 60),
        patient(3, OPEN, 1, 30, 0),
        patient(4, OPEN, 0, 40, 0),
    ];
    let shifts = vec![
        shift_by(1, 0xA1, 11), shift_by(1, 0xB2, 30), shift_by(1, 0xA1, 50), shift_by(1, 0xC3, 80),
        shift_by(2, 0xB2, 25), shift_by(2, 0xB2, 55),
        shift_by(3, 0xA1, 35),
    ];

    let c = census(&patients, &shifts, None);
    assert_eq!(c.admitted, 4, "four patients were released onto the ward");
    assert_eq!(c.went_home, 1);
    assert_eq!(c.died, 1);
    assert_eq!(c.on_ward, 2, "admitted minus went home minus died, and nothing else");
    assert_eq!(c.shifts, 7, "one per anchored leaf");
    assert_eq!(c.keys, 3, "three distinct signers, not seven and not 'three people'");
}

#[test]
fn a_week_is_the_same_arithmetic_over_a_window() {
    let patients = vec![
        patient(1, DISCHARGED, 1, 10, 20),   // admitted and gone before the window
        patient(2, DIED, 1, 50, 60),          // both inside it
        patient(3, OPEN, 1, 55, 0),           // admitted inside it, still here
    ];
    let shifts = vec![shift_by(1, 0xA1, 15), shift_by(2, 0xB2, 55), shift_by(3, 0xB2, 58)];

    let week = census(&patients, &shifts, Some(40));
    assert_eq!(week.admitted, 2, "only the two admitted at or after the window's first slot");
    assert_eq!(week.died, 1);
    assert_eq!(week.went_home, 0, "the one who went home did so before the window");
    assert_eq!(week.shifts, 2, "the leaf from slot 15 is outside the window");
    assert_eq!(week.keys, 1, "one distinct signer took every shift inside the window");
}

#[test]
fn an_empty_ward_reports_zeroes_and_not_nothing() {
    let c = census(&[], &[], None);
    assert_eq!((c.admitted, c.on_ward, c.went_home, c.died, c.shifts, c.keys), (0, 0, 0, 0, 0, 0),
               "a week where nobody came is a fact, and the card shows it as 0");
}

#[test]
fn on_the_ward_can_never_go_negative_even_if_the_chain_is_read_mid_write() {
    // A closed patient whose account was read before its state was written is the shape of a
    // half-read chain. The answer is never a negative census; it is a floor at zero, because a
    // census that goes negative is a bug that reads as a scandal.
    let patients = vec![patient(1, DISCHARGED, 1, 10, 20), patient(2, DIED, 1, 20, 30)];
    let c = census(&patients, &[], Some(25));
    assert_eq!(c.admitted, 0, "neither was admitted inside the window");
    assert_eq!(c.on_ward, 0, "and the subtraction floors at zero rather than going below it");
}

#[test]
fn the_ward_fills_its_beds_and_never_more() {
    assert_eq!(BEDS, 3, "three beds to start — CWF_PLAN.md's beds ruling");
    assert_eq!(to_admit(0, BEDS, 10), 3, "an empty ward opens every bed it has");
    assert_eq!(to_admit(2, BEDS, 10), 1, "one free bed takes one patient");
    assert_eq!(to_admit(3, BEDS, 10), 0, "a full ward admits nobody");
    assert_eq!(to_admit(1, BEDS, 1), 1, "and never more than the queue actually holds");
    assert_eq!(to_admit(0, BEDS, 0), 0, "an empty queue is not an error, it is a quiet night");
    assert_eq!(to_admit(5, BEDS, 5), 0, "more patients than beds — from a bed count that shrank — admits nobody");
}

// ── /api/ward · the payload the weekly card is photographed from ────────────

use vitals_web::ward::ward_payload;

/// The endpoint is the source and the card is a photograph of it, so the payload has to carry the
/// same discipline the card does: every number beside the thing it was derived from, the read time
/// on it, and the word "keys" — never "people", never "doctors".
#[test]
fn every_number_travels_with_where_it_came_from() {
    let patients = vec![
        patient(1, DISCHARGED, 2, 10, 90),
        patient(2, OPEN, 1, 30, 0),
    ];
    let shifts = vec![shift_by(1, 0xA1, 11), shift_by(1, 0xB2, 80), shift_by(2, 0xA1, 35)];

    let v = ward_payload(&patients, &shifts, Some(20), 1234, "devnet:ABC");

    // the six, cumulative and for the window, under names a stranger can read
    for k in ["admitted", "on_ward", "went_home", "died", "shifts", "keys"] {
        assert!(v["cumulative"][k].is_u64(), "cumulative.{k} must be a number");
        assert!(v["week"][k].is_u64(), "week.{k} must be a number");
        assert!(v["derivations"][k].is_string(), "{k} must say where it came from");
    }
    assert_eq!(v["cumulative"]["admitted"], 2);
    assert_eq!(v["cumulative"]["on_ward"], 1);
    assert_eq!(v["week"]["admitted"], 1, "only the patient released at or after slot 20");
    assert_eq!(v["week"]["shifts"], 2);

    assert_eq!(v["as_of_slot"], 1234, "a number without its read time is not evidence");
    assert_eq!(v["source"], "devnet:ABC", "and it says which chain and which program");
    assert_eq!(v["week"]["since_slot"], 20);

    let d = v["derivations"].to_string();
    assert!(d.contains("admitted - went_home - died"),
            "on_ward must publish its own subtraction, so nobody re-counts it another way");
    assert!(!d.contains("people") && !d.contains("doctor"),
            "keys are keys: there is no signup, so nothing here knows how many humans");
}

#[test]
fn the_payload_of_an_empty_ward_is_zeroes_and_still_carries_its_derivations() {
    let v = ward_payload(&[], &[], None, 7, "devnet:ABC");
    assert_eq!(v["cumulative"]["shifts"], 0);
    assert_eq!(v["week"]["since_slot"], serde_json::Value::Null, "no window asked for, none claimed");
    assert!(v["derivations"]["keys"].is_string(), "an empty ward still says how it would have counted");
}

// ── the queue, and admission that needs nobody ──────────────────────────────

use vitals_web::ward::{Queue, Stay};

/// A stay is a chain of cases we already have. The joins are mechanical — state handed from one
/// case to the next — and nothing here writes medicine.
#[test]
fn a_stay_walks_its_chain_and_then_it_is_done() {
    let mut s = Stay::new(7, vec!["anaphylaxis".into(), "observation".into()]);
    assert_eq!(s.patient_id, 7);
    assert_eq!(s.current(), Some("anaphylaxis"));
    assert!(!s.finished(), "a stay on its first case is not finished");
    assert_eq!(s.advance(), Some("observation"), "the bridge to the next case is mechanical");
    assert_eq!(s.current(), Some("observation"));
    assert_eq!(s.advance(), None, "and the chain runs out");
    assert!(s.finished());
}

#[test]
fn admission_needs_nobody_and_fills_only_free_beds() {
    let catalogue = vec![
        vec!["a".to_string(), "b".to_string()],
        vec!["c".to_string()],
        vec!["d".to_string()],
        vec!["e".to_string()],
    ];
    let mut q = Queue::from_catalogue(catalogue.clone(), 100);
    assert_eq!(q.waiting(), 4);

    let first = q.admit(0, BEDS);
    assert_eq!(first.len(), 3, "an empty ward opens all three beds with no human in the loop");
    assert_eq!(q.waiting(), 1);
    assert_eq!(first[0].cases, catalogue[0], "and the stay is the chain the catalogue gave it");

    let ids: Vec<u64> = first.iter().map(|s| s.patient_id).collect();
    assert_eq!(ids, vec![100, 101, 102], "ids start where they were told to and never repeat");

    assert_eq!(q.admit(3, BEDS).len(), 0, "a full ward admits nobody");
    let last = q.admit(2, BEDS);
    assert_eq!(last.len(), 1, "one bed frees, one patient is released, automatically");
    assert_eq!(last[0].patient_id, 103);
    assert_eq!(q.admit(0, BEDS).len(), 0, "an empty queue is a quiet night, not an error");
}

#[test]
fn the_queue_never_invents_a_case() {
    let mut q = Queue::from_catalogue(vec![], 1);
    assert_eq!(q.waiting(), 0);
    assert_eq!(q.admit(0, BEDS).len(), 0,
               "no catalogue, no patients — a longer queue is more existing cases, never new writing");
}

// ── the release policy, published rather than promised ──────────────────────

/// "How many patients a day?" has one honest answer: as many as leave. A bed frees on discharge or
/// death and on nothing else, so the rate is a consequence of how the ward is played, not a number
/// we can pick. The endpoint publishes the rule so a stranger can derive the rate themselves
/// instead of taking a promise from us.
#[test]
fn the_release_policy_is_published_and_promises_no_rate() {
    let v = ward_payload(&[], &[], None, 1, "devnet:ABC");
    let p = &v["policy"];

    assert_eq!(p["beds"], 3);
    assert_eq!(p["a_bed_frees_on"], serde_json::json!(["discharge", "death"]),
               "nothing else frees a bed — not time, not us");
    assert!(p["admissions_per_day"].as_str().unwrap().contains("as many as leave"),
            "the rate is derived from the ward, never promised by us");
    assert!(p["draw"].as_str().unwrap().contains("uniformly"));
    assert!(p["draw"].as_str().unwrap().contains("already on the ward"),
            "a case is not drawn while another copy of it is in a bed");

    let cases = p["catalogue"].as_array().expect("the catalogue is a list a stranger can count");
    assert_eq!(cases.len(), 16, "four episodes and twelve stations, as they exist today");
    let joined = cases.iter().map(|c| c.as_str().unwrap()).collect::<Vec<_>>().join(" ");
    assert!(joined.contains("osce-a") && joined.contains("ep2"), "named, not summarised");
    assert!(!joined.contains("ep1"), "ep1 is the practice case and is not on the ward");
}
