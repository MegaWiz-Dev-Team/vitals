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

/// No patient has a remaining-time figure in these fixtures: none of them has been through a
/// ticker pass. That is a real state on the ward too — a patient admitted since the last pass —
/// and the board's contract for it is that her card says nothing about a clock rather than
/// guessing one.
static NO_CLOCKS: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();

fn patient(id: u64, state: u8, shifts: u32, admitted: u64, closed: u64) -> PatientOnChain {
    PatientOnChain {
        patient_id: id, state, shifts, admitted_slot: admitted, closed_slot: closed,
        lease_holder: [0; 32], lease_until_slot: 0,
    }
}

/// Somebody is in the room with her, and their lease runs to `until`.
fn leased(mut p: PatientOnChain, holder: u8, until: u64) -> PatientOnChain {
    p.lease_holder = [holder; 32];
    p.lease_until_slot = until;
    p
}

/// One read of the ward, with the fields a test does not care about filled in.
fn read<'a>(
    patients: &'a [PatientOnChain],
    shifts: &'a [ShiftOnChain],
    packs: &'a std::collections::BTreeMap<u64, vitals_web::ward::Pack>,
    since: Option<u64>,
    as_of_slot: u64,
) -> vitals_web::ward::WardRead<'a> {
    vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        patients, shifts, packs, since, as_of_slot, now_unix: 1_760_000_000, source: "devnet:ABC",
        times: nothing_dated(), seconds_per_slot: None,
        // Every tape the chain names is here, which is the ward working. The one test about the
        // other case fills this in itself.
        unrebuildable: nothing_lost(), unread: nothing_unread(),
        // No cases through the door: a level then comes from the season's table, which is what
        // the patients still mid-stay on season cases have. The test about a compiled case fills
        // this in itself.
        cases: &[],
    }
}

/// The chain's dating of a handful of slots, as `slot_times` hands it to the payload.
fn dated(entries: &[(u64, i64)]) -> std::collections::BTreeMap<u64, i64> {
    entries.iter().copied().collect()
}

/// No slot has been dated by the chain yet — a `&'static` empty map, for the tests whose subject
/// is not what time it was. A row then carries its slots and no wall time, which is exactly what
/// the board does before the ward has asked the RPC for those blocks.
fn nothing_dated() -> &'static std::collections::BTreeMap<u64, i64> {
    static NONE: std::sync::OnceLock<std::collections::BTreeMap<u64, i64>> =
        std::sync::OnceLock::new();
    NONE.get_or_init(Default::default)
}

/// Nothing is missing — a `&'static` empty map, so every `WardRead` helper can borrow it.
fn nothing_lost() -> &'static std::collections::BTreeMap<u64, String> {
    static NONE: std::sync::OnceLock<std::collections::BTreeMap<u64, String>> =
        std::sync::OnceLock::new();
    NONE.get_or_init(Default::default)
}

/// Every history refreshed this read, which is the ward on a good minute. The one test about a
/// listing that failed fills this in itself.
fn nothing_unread() -> &'static std::collections::BTreeMap<u64, String> {
    nothing_lost()
}

/// No pack has been queued for anyone — the state every test but one is written against, and the
/// state a dev deploy is in before the factory runs.
fn nobody() -> std::collections::BTreeMap<u64, vitals_web::ward::Pack> {
    std::collections::BTreeMap::new()
}

fn shift_by(patient_id: u64, signer: u8, slot: u64) -> ShiftOnChain {
    // The census counts shifts and signers; which tape each one played is the resume's business,
    // so these fixtures carry no run hash and say so by carrying zero.
    ShiftOnChain { patient_id, signer: [signer; 32], slot, run_hash: [0; 32] }
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
    // `due` is the clock, and it is a separate question from whether a bed is free. With nothing
    // due, this is the rule the ward has always had.
    assert_eq!(to_admit(0, BEDS, 10, false), 3, "an empty ward opens every bed it has");
    assert_eq!(to_admit(2, BEDS, 10, false), 1, "one free bed takes one patient");
    assert_eq!(to_admit(3, BEDS, 10, false), 0, "a full ward admits nobody");
    assert_eq!(to_admit(1, BEDS, 1, false), 1, "and never more than the queue actually holds");
    assert_eq!(to_admit(0, BEDS, 0, false), 0, "an empty queue is not an error, it is a quiet night");
    assert_eq!(to_admit(5, BEDS, 5, false), 0, "more patients than beds admits nobody on beds alone");

    // **And the founder's ruling, 22 ก.ย.: "โลกความจริงจำนวนคนไข้ไม่มีคำว่ารอ".** Sick people do
    // not wait for a bed to free. When the clock says one is due, one is admitted whether or not
    // there is a bed — three is a cap we chose and the shortage is the thing being shown.
    assert_eq!(to_admit(3, BEDS, 10, true), 1, "a full ward still admits the one that is due");
    assert_eq!(to_admit(20, BEDS, 10, true), 1,
               "and a ward well past the old cap admits one more: there is no ceiling, only a \
                queue that runs out");
    assert_eq!(to_admit(0, BEDS, 10, true), 3,
               "a due arrival does not add to a bed-filling pass — filling the beds already \
                admits somebody this minute, and the clock is satisfied by that");
    assert_eq!(to_admit(3, BEDS, 0, true), 0,
               "but never out of an empty queue: the ward admits what the factory has built and \
                invents nobody");
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

    let v = ward_payload(&read(&patients, &shifts, &nobody(), Some(20), 1234));

    // the six, cumulative and for the window, under names a stranger can read
    for k in ["admitted", "on_ward", "went_home", "died", "shifts", "keys"] {
        assert!(v["census"][k].is_u64(), "census.{k} must be a number");
        assert!(v["week"][k].is_u64(), "week.{k} must be a number");
        assert!(v["derivations"][k].is_string(), "{k} must say where it came from");
    }
    assert_eq!(v["census"]["admitted"], 2);
    assert_eq!(v["census"]["on_ward"], 1);
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

    // `keys` is the one figure that does not come off an account, and it says so in its own
    // sentence. Folding it into the shifts wording would tell a reader they can check it the way
    // they check the other five, and they cannot: it is read from transaction history and cached.
    let keys = v["derivations"]["keys"].as_str().expect("keys says where it came from");
    assert!(keys.contains("getSignaturesForAddress"),
            "it must name the call that repeats it, not just claim the chain: {keys}");
    assert!(keys.contains("cache") || keys.contains("cached"),
            "and say that it is cached, because a cache is a thing that can be stale: {keys}");
    assert_ne!(v["derivations"]["keys"], v["derivations"]["shifts"],
               "different derivation, different sentence — the five that come off accounts and the \
                one that comes off history are not checked the same way");
}

#[test]
fn the_payload_of_an_empty_ward_is_zeroes_and_still_carries_its_derivations() {
    let v = ward_payload(&read(&[], &[], &nobody(), None, 7));
    assert_eq!(v["census"]["shifts"], 0);
    assert_eq!(v["week"]["since_slot"], serde_json::Value::Null, "no window asked for, none claimed");
    assert!(v["derivations"]["keys"].is_string(), "an empty ward still says how it would have counted");
}

// ── the queue, and admission that needs nobody ──────────────────────────────

// `Queue`, `Stay` and `STAY_CASES` were tested here until 16 ก.ย. They are gone: nothing called
// them, the program closes a patient on the first discharge its engine reaches, and the policy
// sentence that promised a three-case stay was a promise the chain contradicted. What they were
// really testing — that a bed refills with nobody awake — is tested against the thing that does
// it, in `ward_chain`'s ticker and in `to_admit` below.


// ── the release policy, published rather than promised ──────────────────────

/// "How many patients a day?" has one honest answer: as many as leave. A bed frees on discharge or
/// death and on nothing else, so the rate is a consequence of how the ward is played, not a number
/// we can pick. The endpoint publishes the rule so a stranger can derive the rate themselves
/// instead of taking a promise from us.
#[test]
fn the_release_policy_is_published_and_promises_no_rate() {
    let v = ward_payload(&read(&[], &[], &nobody(), None, 1));
    let p = &v["policy"];

    // `beds` is the census, not a cap: an empty ward has nobody on it, and nothing stops a busy
    // one going past the floor the ward keeps trying to hold free.
    assert_eq!(p["beds"], 0, "nobody is on this ward, and the field says so rather than saying 3");
    assert_eq!(p["beds_kept_free"], 3, "three is what it tries to keep open for whoever walks in");
    assert_eq!(p["a_bed_frees_on"], serde_json::json!(["discharge", "death"]),
               "nothing else frees a bed — not time, not us");

    // The arrivals are on a clock now, and the block says how to check the claim rather than
    // asking to be believed.
    // Sixty, and written as a literal on purpose. This assertion is the reason the number cannot
    // drift quietly: it caught the change that brought it here, which is exactly what it is for.
    // The founder ruled sixty on 22 ก.ย., replacing the thirty this said before; the binary
    // defaulted to thirty for a day afterwards and the ruling survived only because the director
    // typed the variable on every deploy. If this ever needs editing again, that edit is somebody
    // deciding to change what the ward promises, and it should cost a diff and a sentence.
    assert_eq!(p["arrivals"]["every_minutes"], 60);
    for said in ["whether or not anybody is here", "admitted_slot", "re-count"] {
        assert!(p["arrivals"]["derivation"].as_str().unwrap_or_default().contains(said),
                "the arrivals block says how the rate is checked: {}", p["arrivals"]["derivation"]);
    }
    assert!(p["arrivals"]["who_arrives"].as_str().unwrap_or_default().contains("people per doctor"),
            "and who arrives is the shortage, which is the whole point of the ward");

    // The old sentence promised "as many as leave", which was true only while a patient could not
    // be admitted except into a bed somebody had left. It would now be false, and a policy block
    // that describes a rule the ward no longer follows is worse than one that says nothing.
    assert!(!p["admissions_per_day"].as_str().unwrap().starts_with("as many as leave"),
            "the old rule is not the rule any more: {}", p["admissions_per_day"]);
    assert!(p["admissions_per_day"].as_str().unwrap().contains("whether or not anybody is here"),
            "and the new one is said in the same place the old one was");
    assert!(p["draw"].as_str().unwrap().contains("queue"),
            "a bed is filled from the queue the factory fills, by the ticker on this host");
    // Until 22 ก.ย. this required the sentence to say a case is never drawn while another bed
    // holds it. B.3 made that a preference — with no ceiling on the census, a hard rule stops
    // admissions the moment the ward holds the whole catalogue — so what the sentence has to carry
    // now is both halves: the preference, and what happens when it cannot be honoured.
    let draw = p["draw"].as_str().unwrap();
    assert!(draw.contains("prefers a case no other bed holds"),
            "the preference is still published, because it is still the rule most of the time: {draw}");
    assert!(draw.contains("only when the queue offers nothing else"),
            "and so is the exception, because a reader who sees two beds on one case deserves to \
             find out here rather than guess we are broken: {draw}");

    // The catalogue was a list of the season's sixteen here until 16 ก.ย. It is not a list any
    // more, because the ward's cases are not a table compiled into this binary: they arrive
    // through the case door, they are counted at the read, and the whole of them is one GET away.
    // `the_policy_says_what_this_ward_actually_does` is where the counts are checked.
    let cat = &p["catalogue"];
    assert!(cat["from"].as_str().is_some_and(|s| s.contains("case factory")),
            "where the ward's cases come from: {cat}");
    assert_eq!(cat["read_them_at"], "/api/ward/cases", "and where to read them in full");
    assert!(!p.to_string().contains("ep2"),
            "no season id anywhere in the policy: this ward refuses every one of them at its door");
}

/// The failure that would do the most damage is not a wrong number — it is a zero.
///
/// If the chain cannot be read, an endpoint that answers `0` is indistinguishable from a ward
/// nobody came to, and that zero would be photographed onto a card and read out in a video. So an
/// unreadable chain says so, in the same shape, and carries no numbers at all.
#[test]
fn a_chain_that_cannot_be_read_says_so_and_never_reports_zero() {
    let v = vitals_web::ward::ward_unavailable("devnet:ABC", "rpc timed out after 8s");

    assert_eq!(v["readable"], false);
    assert_eq!(v["source"], "devnet:ABC");
    assert!(v["why"].as_str().unwrap().contains("rpc timed out"), "say what went wrong, not 'error'");
    assert!(v["census"].is_null() && v["week"].is_null(),
            "no numbers at all — a zero here would be read out as 'nobody came'");
    assert!(v["policy"].is_object(), "the rule is still true when the chain is unreachable");

    let ok = ward_payload(&read(&[], &[], &nobody(), None, 1));
    assert_eq!(ok["readable"], true, "and a readable chain says that too, so the card can tell them apart");
}

// ── the idle clock, against the catalogue it will actually run on ──────────

use std::path::PathBuf;
use vitals_replay::{resume, shift};
use vitals_web::ward::CATALOGUE;

/// The server's own resolver, not a third copy of it. A test that knew where cases live
/// independently would keep passing on the day the server stopped agreeing with it.
fn sce_path(id: &str) -> PathBuf {
    vitals_web::ward_chain::case_path(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."), id)
}

/// Untreated from the start, at the scenario's own grain: when does she arrest?
fn arrests_at(sce: &str) -> Option<u32> {
    let (mut st, _) = resume(sce, &[]).expect("scenario loads");
    let grain = st.tick_seconds();
    for i in 1..=(4 * 3600) {
        st.tick(grain);
        if st.outcome().is_some() {
            return Some((i as f64 * grain) as u32);
        }
    }
    None
}

/// **The ward kills patients nobody visits** — and the record has to be able to say so.
///
/// This test asserted the opposite until 16 ก.ย. The idle clock had a cap set below the fastest
/// untreated arrest in the catalogue, so a gap could deteriorate a patient and never finish her,
/// and the argument for it was about the record: death only inside a shift means every death on
/// this ward is attributable to what a key did or failed to do while holding her.
///
/// The founder overruled it the same day. At 1:60 a patient nobody visits deteriorates as the
/// engine says and can arrest and die unattended, because the alternative is a ward where being
/// abandoned is survivable — which is the one thing a ward is not. The record still says who and
/// when: `ward_chain`'s ticker closes her rather than a stranger, with an empty tape and the idle
/// span, so the chain reads "died, nobody on shift" and no stranger ever opens a corpse believing
/// she is alive.
///
/// So the guarantee this file holds is now the reverse one, and it is held over the whole
/// catalogue rather than one case: a gap long enough must reach the same death an untended bedside
/// would, through the idle path, at the scenario's own grain.
#[test]
fn an_unattended_patient_dies_when_the_engine_says_she_does() {
    use vitals_replay::{IDLE_SIM_PER_REAL, SLOT_SECONDS};

    // The founder's figure is that fourteen of the sixteen arrest within 3–14 real hours. The gap
    // tested with is fifteen, one clear hour past the slowest of them (osce-c3, at exactly 14.0),
    // so this asserts the deaths rather than the arithmetic of a boundary.
    let real_hours = 15.0;
    let gap_slots = (real_hours * 3600.0 / SLOT_SECONDS) as u64;

    let mut table: Vec<(String, Option<f64>, bool)> = Vec::new();
    for id in CATALOGUE {
        let p = sce_path(id);
        let sce = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));

        // When the engine finishes her, untended from the first second — in simulated seconds,
        // which the ratio turns into the real hours a bed would have stood empty.
        let arrest_real_hours =
            arrests_at(&sce).map(|sim| sim as f64 * IDLE_SIM_PER_REAL.recip() / 3600.0);

        // And what the ward's own idle path does with a gap of that length. This is the half that
        // matters: `shift` is what the server calls, and a clock that kills only when a test ticks
        // it by hand would be a promise about nothing.
        let (mut st, _) = resume(&sce, &[]).expect("scenario loads");
        shift(&mut st, &[], gap_slots as f64 * SLOT_SECONDS);
        table.push((id.to_string(), arrest_real_hours, st.outcome().is_some()));
    }

    let died: Vec<&(String, Option<f64>, bool)> = table.iter().filter(|(_, _, d)| *d).collect();
    let rendered = table
        .iter()
        .map(|(id, h, d)| match h {
            Some(h) => format!("{id}: arrests at {h:.1} real h{}", if *d { "" } else { " (but survived the gap!)" }),
            None => format!("{id}: never arrests untended"),
        })
        .collect::<Vec<_>>()
        .join(" · ");

    assert!(died.len() >= 14,
            "fourteen of the sixteen must be finished by {real_hours:.0} real hours alone — the \
             founder's own figure, and the plan quotes it. {} died. {rendered}", died.len());

    for (id, hours, _) in table.iter().filter(|(_, h, _)| h.is_some()) {
        let h = hours.unwrap();
        assert!((3.0..=14.0).contains(&h),
                "{id} arrests at {h:.1} real hours, outside the 3–14 the plan states. A case that \
                 finishes in under three real hours is a bed that empties before a stranger can \
                 reach it; one past fourteen is a promise the board's figures no longer keep. \
                 Re-measure and move the plan, or move the case. {rendered}");
    }
}

/// **A stay is one case**, and the policy says so rather than promising a chain of them.
///
/// It said three — acute, observation, ward-to-home, joined mechanically — and none of it was
/// wired: `Stay::advance` was called nowhere, and the program closes a patient the first time its
/// engine reaches a discharge. A sentence a reader can check against the chain and find false is
/// worse than no sentence, and this one was on the endpoint a judge is invited to re-derive.
///
/// It belongs in `policy` beside the beds because both answer the same question a reader has —
/// *how fast does this thing consume patients?* — and an unreadable chain must not take the answer
/// with it.
#[test]
fn a_stay_is_one_case_and_the_policy_publishes_it() {
    use vitals_web::ward::ward_unavailable;

    let live = ward_payload(&read(&[], &[], &nobody(), None, 1));
    let stay = live["policy"]["stay"].as_str().expect("the policy must say what a stay is");
    assert!(stay.contains("one case"), "it says so in words a reader can check: {stay}");
    assert!(!stay.contains('3') && !stay.to_lowercase().contains("three"),
            "and it no longer promises a chain of them: {stay}");
    assert!(stay.contains("goes home") && stay.contains("dies"),
            "with the two ways it ends, which are the engine's and the chain's: {stay}");

    let dark = ward_unavailable("devnet:ABC", "rpc timed out");
    assert_eq!(dark["policy"]["stay"], live["policy"]["stay"],
               "the rule is true whether or not the chain can be read — an unreadable chain takes \
                the numbers with it, never the policy");
}

/// The globe reads this: one entry per patient, and where she is from.
///
/// developer-4d's front page is a globe you spin to find patients (founder, 15 ก.ย. 23:15), so the
/// census alone is not enough — the board needs the patients themselves. Two of the fields on each
/// entry do not come from the chain at all: her name and her country are in the pack the factory
/// queued, and the chain holds neither. That is exactly why they carry their own derivation and
/// why they are **null rather than absent** before a pack exists: a globe that hides patients
/// without a country would hide every patient on a dev deploy, and look like an empty world.
///
/// `country` is ISO 3166-1 alpha-3 — THA, IDN, NGA — because the globe matches on it and a free
/// text country is a country nobody can match.
#[test]
fn the_board_lists_the_patients_and_where_they_are_from() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{Pack, Persona};

    let patients = vec![patient(7, OPEN, 2, 10, 0), patient(8, DIED, 1, 20, 90)];
    let mut packs = BTreeMap::new();
    packs.insert(7u64, Pack {
        difficulty: None,
        case: "ep2".into(),
        persona: Persona { name: "Ploy".into(), country: "THA".into(), age: 34, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    });

    let v = ward_payload(&read(&patients, &[], &packs, None, 1234));
    let list = v["patients"].as_array().expect("the board needs the patients themselves");
    assert_eq!(list.len(), 2, "every patient the ward ever admitted, closed ones included");

    assert_eq!(list[0]["patient_id"], 7);
    assert_eq!(list[0]["state"], "on_ward", "a word a person recognises, not the program's byte");
    assert_eq!(list[0]["shifts"], 2);
    assert_eq!(list[0]["country"], "THA");
    assert_eq!(list[0]["name"], "Ploy");
    // **The card writes a sentence about her, so the row has to say which sentence.** This key was
    // missing while the globe's `theirs(p)` read it, so every card on production said "them" about
    // patients whose sex the ward knows — and the page's own harness passed, because its fixture
    // was hand-written with a `sex` the board never sent. Asserted here, on the board's own
    // output, which is the only place that can hold the page's reading honest.
    assert_eq!(list[0]["sex"], "f", "the row carries her sex, because a card writes about her");

    // **The card's fixture comes from here, not from somebody's memory of here.**
    //
    // `world/globe_logic.mjs` tests the sentence the front page writes about a patient, and it is
    // a node harness: it cannot call this builder, so until now it fed `clock()` a row I typed by
    // hand. I gave that row a `sex` the board did not publish, the page read `p.sex`, and every
    // card on production said "them" about patients this ward knows the sex of — with the harness
    // green throughout, because it was asserting against my wish rather than against the ward.
    //
    // So the board this test just built is written out, and the node harness reads *that*. The
    // contract flows from the producer to the consumer through an artefact instead of a copy, and
    // the link is real rather than remembered: delete the assertion above and this file stops
    // being written, and the node test fails loudly instead of quietly becoming fiction.
    // `gates.sh` runs the workspace tests before the node checks, so it is always this tree's.
    let contract = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/contract");
    std::fs::create_dir_all(&contract).expect("somewhere to put the board the page is tested on");
    std::fs::write(
        contract.join("board.json"),
        serde_json::to_string_pretty(&v).expect("the board serialises"),
    )
    .expect("the card's fixture is written from the board this test built");

    assert_eq!(list[1]["state"], "died");
    assert!(list[1]["country"].is_null(),
            "a patient with no pack yet is listed with a null country, never dropped — a globe \
             that hides her shows an emptier ward than the one that exists");
    assert!(list[1]["name"].is_null());

    let d = v["derivations"]["patients"].as_str().expect("the list says where it came from");
    assert!(d.contains("pack"), "and says the persona is not on chain: {d}");
}

/// **A player chooses a patient by how hard she is**, so the ward has to know.
///
/// The founder, 15 ก.ย. 23:25: *"อยากให้มีระดับความยาก เลือกได้"* — there should be a difficulty, and
/// you can choose. Embla's catalogue carries one on every case; this ward's sixteen carry theirs in
/// `difficulty_of`, taken from the tiers the station sets and the episode list already use, so the
/// ward and the bay cannot disagree about how hard a case is.
///
/// Levels one and two are open to anyone. `resident` is the level the star gate may fence later
/// (week 3, an option and not a promise), and nothing here gates anything today.
#[test]
fn every_case_the_ward_can_admit_has_a_difficulty_and_the_board_publishes_it() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{difficulty_of, Pack, Persona, CATALOGUE};

    for case in CATALOGUE {
        let d = difficulty_of(case)
            .unwrap_or_else(|| panic!("{case} is in the catalogue with no difficulty — a patient \
                                       nobody can choose by level is a patient the panel cannot \
                                       show, and the gap is silent"));
        assert!(matches!(d, "student" | "intern" | "resident"),
                "{case} carries {d}, which is not one of Embla's three levels");
    }

    let patients = vec![patient(7, OPEN, 2, 10, 0), patient(8, OPEN, 0, 20, 0)];
    let mut packs = BTreeMap::new();
    packs.insert(7u64, Pack {
        difficulty: None,
        case: "ep2".into(),
        persona: Persona { name: "Ploy".into(), country: "THA".into(), age: 34, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    });

    let v = ward_payload(&read(&patients, &[], &packs, None, 1234));
    let list = v["patients"].as_array().expect("patients");
    assert_eq!(list[0]["case"], "ep2", "the board says which case she is");
    assert_eq!(list[0]["difficulty"], "intern", "at the level the bay already gives that case");
    assert!(list[1]["difficulty"].is_null(),
            "and a patient no pack describes yet has no case, so she has no level either — \
             guessing one would put her in a band a player chose against");

    let d = v["derivations"]["patients"].as_str().expect("derivation");
    assert!(d.contains("difficulty"), "the list says where the level came from: {d}");
}

/// **Endemic disease is epidemiology, not stereotype**, and the file that says so is checked.
///
/// The founder, 15 ก.ย. 23:25: *"ต้องมีโรคหายากประจำประเทศนั้นๆ"* — there must be the diseases that
/// belong to each country. The pairing rule is unchanged for the common draw: where a patient is
/// from never selects her disease. What a country may carry is an `endemic` list, and one draw in
/// five for a patient from that country comes from it. A Thai patient is no more likely to have
/// chest pain than anyone else; she is more likely than a Norwegian to have dengue, and that is a
/// fact about mosquitoes.
///
/// The pairing is a fact about a case now, not a row in a file. A compiled pack carries `endemic`
/// and the country it is endemic in, the compiler writes both, and the queue door checks the pack's
/// claim against the catalogue (`ward_chain::endemic_claim`). Nothing in this crate reads
/// `data/endemic.json` any more.
///
/// **The file is still there and still matters**, which is what this test is now for: `vitals-
/// factory` reads it off the repo at every tick to decide which patients get the one-draw-in-five
/// from their own country, and fails the whole tick if it cannot be read. So its shape is checked
/// here — and the ids in it are not held to a catalogue any more, because that catalogue lives in
/// a store this test cannot see. Moving the factory's own draw to the catalogue is the other half
/// of the change the door has already made.
#[test]
fn the_endemic_file_the_factory_reads_keeps_its_shape() {
    use vitals_web::ward::ENDEMIC_IN;

    assert_eq!(ENDEMIC_IN, 5, "one draw in five, for a patient from a country a case is endemic in");

    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/endemic.json"),
    )
    .expect("the factory reads this file at every tick and fails the tick without it");
    let file: serde_json::Value = serde_json::from_str(&raw).expect("and it has to parse for it");
    let lists = file["endemic"].as_object().expect("an object of country → cases");

    for (country, cases) in lists {
        assert_eq!(country.len(), 3,
                   "{country} is not ISO 3166-1 alpha-3, and the globe matches on alpha-3");
        assert!(country.bytes().all(|b| b.is_ascii_uppercase()), "{country} must be upper case");
        let cases = cases.as_array().expect("a list of case ids");
        assert!(!cases.is_empty(),
                "{country} carries an empty endemic list, which is a country that looks described \
                 and is not — leave it out instead");
    }

    // And the door's own check does not read it: the whole point of the fix is that the claim is
    // answered by the catalogue, so a file that stays for another crate cannot creep back in here.
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ward_chain.rs"),
    )
    .expect("the door");
    assert!(!src.contains("ward::endemic()") && !src.contains("include_str!(\"../data/endemic.json\")"),
            "the queue door is reading the season's file again — eighteen of the cases this ward \
             holds are tagged endemic by the compiler, and that file knows none of them");
}

/// The panel offers levels, so the policy has to say what is actually on the shelf — and the shelf
/// is what came through the case door, counted at the read. A level is on a pack now, not in a
/// table compiled into this binary, which is why the read carries the cases.
#[test]
fn the_policy_publishes_the_levels_and_the_endemic_rule() {
    use vitals_web::ward_case::CaseSummary;

    let case = |id: &str, level: &str| CaseSummary {
        case_id: id.into(),
        archetype: "septic_shock".into(),
        patient_age: Some(40),
        patient_sex: Some("female".into()),
        country: None,
        difficulty: level.into(),
        endemic: false,
        provisional: true,
        withdrawn: false,
        version: "0.1.0".into(),
        title: "a compiled case".into(),
    };
    let held = vec![case("a-1", "student"), case("a-2", "intern"), case("a-3", "intern")];

    let patients: Vec<_> = Vec::new();
    let packs = nobody();
    let mut r = read(&patients, &[], &packs, None, 1);
    r.cases = &held;
    let v = ward_payload(&r);
    let levels = &v["policy"]["difficulty"];
    assert_eq!(levels["student"], 1);
    assert_eq!(levels["intern"], 2);
    assert_eq!(levels["resident"], 0,
               "a band with nothing in it is zero and not missing: a player choosing it is told \
                there is nothing there rather than left to find out at a bed");
    assert_eq!(levels["student"].as_u64().unwrap()
                   + levels["intern"].as_u64().unwrap()
                   + levels["resident"].as_u64().unwrap(),
               held.len() as u64,
               "every case has a level and no case has two");

    let rule = v["policy"]["endemic"].as_str().expect("the endemic rule is published");
    assert!(rule.contains("never"), "it has to say what origin does not do: {rule}");
    assert!(rule.contains("the case it names is tagged endemic"),
            "and how a pack's claim is checked, which is against the catalogue and not against a \
             file written for the season: {rule}");
    // Counted off the cases the ward holds. None of these three is endemic, so the honest answer
    // is zero — and it is zero rather than absent, because "we counted and there are none" and "we
    // did not look" are the two facts this endpoint keeps apart everywhere else.
    assert_eq!(v["policy"]["endemic_cases"], 0);
    assert_eq!(v["policy"]["countries_with_an_endemic_case"], 0);
}

/// The globe renders these fields, so the endpoint answers in the globe's own words.
///
/// developer-4d's page is the consumer, and the two places it touches a patient — `row()` and
/// `stateOf()` — read exactly this. An endpoint that answers `open` where the page says `on_ward`
/// works only because the page forgives it; a field it does not forgive, like `bed` or
/// `on_shift_since`, simply disappears from the ward with nothing to say it is missing.
#[test]
fn the_globe_reads_every_field_it_renders() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{Pack, Persona};

    let lease_ends = 5_000u64;
    let patients = vec![
        leased(patient(7, OPEN, 2, 10, 0), 0xA1, lease_ends),
        patient(8, OPEN, 0, 20, 0),
        patient(9, DISCHARGED, 3, 5, 900),
    ];
    let mut packs = BTreeMap::new();
    // Both open patients are the ward's own. A bed is a patient the ward can describe, so a
    // test about beds describes them; an undescribed one is the other rule's subject.
    packs.insert(8u64, Pack {
        difficulty: None,
        case: "osce-c".into(),
        persona: Persona { name: "Fon".into(), country: "THA".into(), age: 6, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    });
    packs.insert(7u64, Pack {
        difficulty: None,
        case: "ep2".into(),
        persona: Persona { name: "Ploy".into(), country: "THA".into(), age: 34, sex: "f".into() },
        portrait: [("stable".to_string(),
                    "https://storage.googleapis.com/vitals-world-portraits/\
                     aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp"
                        .to_string())].into_iter().collect(),
        endemic: true,
    });

    let now = 1_760_000_000u64;
    // The chain's dating of the slot Ploy's lease was taken in — twenty minutes ago — and of the
    // two admissions. Nothing here is derived from `now`: these are the numbers `getBlockTime`
    // answered for those slots, and the payload may only repeat them.
    let times = dated(&[
        (lease_ends - vitals_program::LEASE_SLOTS, now as i64 - 20 * 60),
        (10, now as i64 - 6 * 3600),
        (20, now as i64 - 5 * 3600),
        (900, now as i64 - 3600),
    ]);
    let v = ward_payload(&vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 4_000, now_unix: now, source: "devnet:ABC",
        unrebuildable: nothing_lost(), unread: nothing_unread(), times: &times, seconds_per_slot: None,
    });

    assert!(v["census"]["on_ward"].is_u64(),
            "the page reads census.on_ward or the same keys at the top level, and finds neither \
             under a name of our own");

    let by_id = |id: u64| v["patients"].as_array().unwrap().iter()
        .find(|p| p["patient_id"] == id).cloned().expect("patient listed");

    let ploy = by_id(7);
    assert_eq!(ploy["state"], "on_shift", "somebody is in the room with her and the lease stands");
    // An instant, not a bare number: the page renders it in the reader's own zone and the string
    // means one moment to everybody. Checked against the clock the payload was built with.
    let since = ploy["on_shift_since"].as_str().expect("on shift since, as a UTC instant");
    assert_eq!(since, vitals_web::ward::utc_iso(now - 20 * 60),
               "the chain's time for the slot the lease was taken in, repeated and not recomputed");
    assert_eq!(ploy["bed"], 1, "first of the open patients by admission");
    assert_eq!(ploy["age"], 34);
    assert_eq!(ploy["endemic"], true, "drawn from her country's list, and the pack says so");
    let base = "https://storage.googleapis.com/vitals-world-portraits/\
                aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.webp";
    assert_eq!(ploy["portrait"], base, "she is on the ward, so her base picture is what is drawn");
    assert_eq!(ploy["portraits"]["stable"], base,
               "and the board is handed the whole set, so it can change her picture the moment it \
                learns her status without asking again");

    let waiting = by_id(8);
    assert_eq!(waiting["state"], "on_ward", "nobody is with her");
    assert_eq!(waiting["bed"], 2);
    assert!(waiting["on_shift_since"].is_null());
    assert_eq!(waiting["endemic"], false, "no pack is not a claim about where she is from");

    let home = by_id(9);
    assert_eq!(home["state"], "went_home");
    assert!(home["portrait"].is_null(),
            "she went home and there is no picture of her leaving yet — she does not borrow the \
             one of herself ill in a bed");
    assert!(home["bed"].is_null(), "a patient who went home is in nobody's bed");

    // The lease has run out but nobody has anchored: she is on the ward, not on shift. This is the
    // state an abandoned shift leaves behind, and showing it as "on shift" would tell a stranger
    // the room is taken when it is free for them to walk into.
    let expired = ward_payload(&vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: lease_ends + 1, now_unix: now, source: "devnet:ABC",
        unrebuildable: nothing_lost(), unread: nothing_unread(), times: &times, seconds_per_slot: None,
    });
    let ploy = expired["patients"].as_array().unwrap().iter()
        .find(|p| p["patient_id"] == 7).cloned().unwrap();
    assert_eq!(ploy["state"], "on_ward", "an expired lease is a free room, and the board says so");
}

/// The globe's "take a shift" link points at `/ward/<patient_id>`, so that path has to be read.
///
/// Read strictly, because the id goes into a PDA derivation and then into a page: anything that is
/// not exactly one whole number is not a patient, and guessing at `/ward/42/../43` or `/ward/42?x`
/// would open a different bed than the one somebody clicked.
#[test]
fn a_patient_page_is_addressed_by_one_whole_number() {
    use vitals_web::ward::patient_id_in_path;

    assert_eq!(patient_id_in_path("/ward/42"), Some(42));
    assert_eq!(patient_id_in_path("/ward/1789488342"), Some(1_789_488_342));

    for bad in ["/ward/", "/ward", "/ward/42/43", "/ward/42x", "/ward/-1", "/ward/ 42",
                "/ward/42/../43", "/wardrobe/42", "/ward/0x2a", "/api/ward"] {
        assert_eq!(patient_id_in_path(bad), None, "{bad} is not a patient");
    }

    // u64::MAX is a legal id — patient ids are unix seconds today, but the program takes a u64 and
    // a page that refused the top of the range would refuse a patient the chain accepts.
    assert_eq!(patient_id_in_path("/ward/18446744073709551615"), Some(u64::MAX));
    assert_eq!(patient_id_in_path("/ward/18446744073709551616"), None, "and one past it is not");
}

/// The persona pool the factory draws people from — twenty countries, three people each.
///
/// Every rule here exists because breaking it produces a patient who is wrong in a way nobody
/// would notice until a clinician did. A case names its patient's sex (`Chest pain — M 25`), so a
/// country whose people are all one sex cannot serve half the catalogue — and a pack that
/// contradicted its own case would put a woman's name on a man's presentation. A country the globe
/// cannot place is a patient who lands in the Unknown tray for ever. A repeated name across the
/// pool is two patients a reader cannot tell apart on a board.
///
/// Ages are deliberately **not** here. A case carries its own band, and the factory picks inside
/// it; an age in the pool would be an age that contradicts the case it is paired with.
#[test]
fn the_persona_pool_can_actually_fill_the_catalogue() {
    use std::collections::HashSet;
    use vitals_web::ward::persona_pool;

    let pool = persona_pool();
    assert_eq!(pool.len(), 74, "seventy-four countries, from the whole world and its worst shortages (founder, 16 Sep 2026), so the globe has something to light up everywhere");

    let globe = include_str!("../static/world/index.html");
    let mut seen_names: HashSet<String> = HashSet::new();

    for country in &pool {
        assert_eq!(country.country.len(), 3, "{} is not alpha-3", country.country);
        assert!(globe.contains(&format!("\"{}\":", country.country)),
                "the globe's own table cannot place {} — a patient from there would sit in the \
                 Unknown tray for ever", country.country);
        // Three at least; deeper where the need is (founder, 16 Sep 2026: nine for the five
        // countries with most people per doctor, six for the next five — the factory computes
        // which from physicians.json and holds the file to it in its own tests).
        assert!(country.personas.len() >= 3, "{} must carry at least three people", country.country);

        let sexes: HashSet<&str> = country.personas.iter().map(|p| p.sex.as_str()).collect();
        assert!(sexes.contains("f") && sexes.contains("m"),
                "{} carries only {:?} — a case names its patient's sex, so a country of one sex \
                 can only fill half the catalogue", country.country, sexes);

        for p in &country.personas {
            assert!(!p.name.trim().is_empty(), "{} has an unnamed person", country.country);
            assert!(p.name.contains(' '), "{} is one word — a chart carries a full name", p.name);
            assert!(seen_names.insert(p.name.clone()),
                    "{} appears twice in the pool, and two patients a reader cannot tell apart on \
                     a board is worse than one country fewer", p.name);
        }
    }
}

/// A portrait is a set, and it never shows her worse than she is.
///
/// The founder wants her picture to change with her state and stay the same person. The keys are
/// **the engine's own seven status words and no others**, because a key the engine cannot report
/// is a picture claiming a state the chain does not — and the bay already renders those seven.
///
/// The fallback is the bay's, for the bay's reason: the nearest *milder* state that exists, never
/// a worse one. Hanging an arrest over a patient who is talking to you is the frame telling a lie
/// the mark sheet then marks. It runs the other way too — a patient who went home does not borrow
/// the picture of herself ill in a bed, she simply has no picture until the one of her leaving is
/// made.
#[test]
fn a_portrait_is_a_set_and_never_shows_her_worse_than_she_is() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{portrait_for, PORTRAIT_LADDER};

    assert_eq!(PORTRAIT_LADDER,
               ["recovered", "improving", "stable", "deteriorating", "critical", "arrest", "dead"],
               "mildest first, and every word one the engine itself reports");

    let url = |n: u8| format!("https://storage.googleapis.com/vitals-world-portraits/{}.webp",
                              format!("{n:02x}").repeat(32));
    let mut set = BTreeMap::new();
    set.insert("stable".to_string(), url(1));
    set.insert("deteriorating".to_string(), url(2));

    assert_eq!(portrait_for(&set, "stable"), Some(url(1)).as_deref());
    assert_eq!(portrait_for(&set, "critical"), Some(url(2)).as_deref(),
               "no critical picture, so the nearest milder one — she is at least this ill");
    assert_eq!(portrait_for(&set, "arrest"), Some(url(2)).as_deref());
    assert_eq!(portrait_for(&set, "improving"), None,
               "there is no picture of her better than she was, and inventing one from a worse \
                state would show a patient sicker than she is");
    assert_eq!(portrait_for(&set, "recovered"), None,
               "a patient who went home does not borrow the picture of herself ill in bed");

    // Death is never generated (producer, 16 ก.ย.), so it resolves to her last living state — the
    // ladder gives that for free, and the board's own word still says died.
    let mut with_arrest = set.clone();
    with_arrest.insert("arrest".to_string(), url(3));
    assert_eq!(portrait_for(&with_arrest, "dead"), Some(url(3)).as_deref(),
               "the last living state we have a picture of");

    assert_eq!(portrait_for(&BTreeMap::new(), "stable"), None, "no set, no picture");
    assert_eq!(portrait_for(&set, "on_ward"), None,
               "a word the engine does not report resolves to nothing rather than to a guess");
}

/// The board and the voice are the same person.
///
/// The ward renames the case's patient and moves her country — that is the premise — and the case
/// file is also what the patient's voice is built from. So on a ward shift she would introduce
/// herself as the name in the file while the board beside her said something else, which is the
/// product contradicting itself out loud in the one place a learner is listening.
///
/// Only the person changes. The room, what she is presenting with, her cadence, her authored
/// dialogue and her sex are the case's and stay the case's — they are the medicine and the
/// writing, and the ward writes neither.
#[test]
fn the_ward_renames_the_patient_and_changes_nothing_else() {
    use vitals_web::ward::{voiced_as, Persona};

    let case: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/personas/osce-a.json"),
        )
        .expect("osce-a has a persona"),
    )
    .expect("it parses");

    let her = Persona {
        name: "Anan Thepwong".into(),
        country: "THA".into(),
        age: 69,
        sex: "m".into(),
    };
    let voiced = voiced_as(&case, &her);

    assert_eq!(voiced["patient"]["name"], "Anan Thepwong", "the voice uses the ward's name");
    assert_eq!(voiced["patient"]["age"], 69);
    assert_eq!(voiced["patient"]["sex"], case["patient"]["sex"],
               "sex is the case's own and is never overwritten — the door already refused a pack \
                that disagreed with it, and the pronouns in the brief are built from this");

    for untouched in ["room", "presenting", "cadence", "fallback", "dialogue", "speaker"] {
        assert_eq!(voiced[untouched], case[untouched],
                   "{untouched} is the case's writing and the ward does not write medicine");
    }
    assert_eq!(voiced["patient"]["affect"], case["patient"]["affect"],
               "how she is feeling belongs to the case too");
}

/// Every time on the ward is a slot or a UTC instant, and never a local one.
///
/// Founder's question, 16 ก.ย.: how does the ward handle time zones. The answer the payload has to
/// make true is that it does not have any. The only truth is the chain, so a time is either the
/// slot it happened at — which is what a stranger re-derives from — or a UTC instant with a Z on
/// it. A bare "2026-09-16 14:05" in a payload is a number that means a different moment to every
/// reader, and nobody can tell which one we meant.
///
/// Rendering into somebody's own zone is the browser's job, from these. Nothing about anybody's
/// zone is asked for or stored.
#[test]
fn every_time_the_ward_publishes_is_a_slot_or_a_z() {
    use vitals_web::ward::{ward_unavailable, Pack, Persona};

    let lease_ends = 5_000u64;
    let patients = vec![
        leased(patient(7, OPEN, 2, 10, 0), 0xA1, lease_ends),
        patient(9, DISCHARGED, 3, 5, 900),
    ];
    let mut packs = std::collections::BTreeMap::new();
    packs.insert(7u64, Pack {
        difficulty: None,
        case: "ep2".into(),
        persona: Persona { name: "Ploy".into(), country: "THA".into(), age: 54, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    });
    // Both of them are the ward's own patients: a bed is a patient the ward can describe, and a
    // test about beds has to describe them or it is testing the other rule.
    packs.insert(9u64, Pack {
        difficulty: None,
        case: "osce-c".into(),
        persona: Persona { name: "Fon".into(), country: "THA".into(), age: 6, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    });

    let payloads = [
        ward_payload(&vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
            patients: &patients, shifts: &[], packs: &packs,
            since: Some(1), as_of_slot: 4_000, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(), unread: nothing_unread(),
        cases: &[],
        seconds_per_slot: None,
            // Her admission, her discharge, and the slot the lease on patient 7 was taken in:
            // every slot this payload turns into a time, so the walk below has one of each to
            // look at rather than a page of nulls.
            times: &dated(&[
                (10, 1_759_996_000), (5, 1_759_995_000), (900, 1_759_997_000),
                (lease_ends - vitals_program::LEASE_SLOTS, 1_759_999_000),
            ]),
        }),
        ward_unavailable("devnet:ABC", "rpc timed out"),
    ];

    // Walk every string in the payload and refuse anything that looks like a time and does not
    // end in Z. Walked rather than listed, so a field added later is covered by this test the day
    // it appears rather than the day somebody remembers to add it here.
    fn walk(v: &serde_json::Value, path: String, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => {
                let looks_like_a_time = s.len() >= 16
                    && s.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                    && s.as_bytes()[4] == b'-'
                    && s.contains(':');
                if looks_like_a_time && !s.ends_with('Z') {
                    out.push(format!("{path} = {s}"));
                }
            }
            serde_json::Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(x, format!("{path}[{i}]"), out);
                }
            }
            serde_json::Value::Object(o) => {
                for (k, x) in o {
                    walk(x, format!("{path}.{k}"), out);
                }
            }
            _ => {}
        }
    }

    for p in &payloads {
        let mut bare = Vec::new();
        walk(p, "payload".into(), &mut bare);
        assert!(bare.is_empty(),
                "a time with no zone means a different moment to every reader: {bare:?}");
    }

    // The one wall-clock field the board has, and it carries its Z.
    let on_shift_since = payloads[0]["patients"][0]["on_shift_since"].as_str()
        .expect("on shift since is an instant, not a bare number of seconds");
    assert!(on_shift_since.ends_with('Z') && on_shift_since.contains('T'),
            "ISO 8601 in UTC, so a browser can render it in the reader's own zone: {on_shift_since}");

    // And the window says which day it counts by, because a week is a different week in Bangkok.
    assert_eq!(payloads[0]["week"]["basis"], "UTC");
}

/// **And no page on the ward has a zone of its own.**
///
/// The other half of the founder's question. The payload carries slots, unix seconds and Z-suffixed
/// instants; the pages turn those into the viewer's own local time with the browser's own
/// formatter, and that is the only place a zone exists. A hard-coded offset or a named zone
/// anywhere in the ward's pages would be Bangkok's clock rendered for a reader in Nairobi, quietly.
#[test]
fn no_page_on_the_ward_carries_a_time_zone() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static");
    for file in ["world/index.html", "world/shift.html", "bay.js", "bay-surface.html"] {
        let src = std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        // A named zone, an offset written out, or a formatter told which zone to use. The viewer's
        // own is the only correct answer and it is the browser's default.
        for banned in ["Asia/", "America/", "Europe/", "timeZone:", "timeZone =", "GMT+", "UTC+", "+07:00", "+0700"] {
            assert!(!src.contains(banned),
                    "{file} names a time zone ({banned:?}) — the ward has none, and the viewer's \
                     is the browser's to supply");
        }
        // And nothing parses a bare datetime: a string with no Z is a moment nobody can place, and
        // the globe already refuses one rather than guessing (`whenMs`).
        assert!(!src.contains("new Date(\""), "{file} builds a date from a literal string");
    }

    // The globe says so in its own words, where the next person to touch it will read it.
    let globe = std::fs::read_to_string(dir.join("world/index.html")).expect("the globe");
    assert!(globe.contains("a zone is the"), "the rule is written where the code that keeps it is");
}

/// **A bed is a patient the ward can describe.**
///
/// Founder, 16 ก.ย., on three test patients wedging staging shut: "แก้ไขระยะยาวเลย" — the long-term
/// fix. Those three reached the chain without passing the queue, so the ward could not say who
/// they were; they filled every bed, nobody could take a shift on a patient with no case, nobody
/// could discharge them, and fourteen packs waited behind them for ever.
///
/// The census still counts them — they are on the chain, and `on_ward` is chain truth. What
/// changes is what a bed means: a patient the ward cannot describe holds no bed, blocks no
/// admission, and appears in her own row saying what she is, rather than as one of the three.
#[test]
fn a_patient_the_ward_cannot_describe_holds_no_bed() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{beds_taken, Pack, Persona};

    let patients = vec![
        patient(1, OPEN, 0, 10, 0),   // admitted outside the ward — no pack
        patient(2, OPEN, 0, 20, 0),   // the same
        patient(3, OPEN, 2, 30, 0),   // hers is queued and described
        patient(4, DISCHARGED, 4, 5, 90),
    ];
    let mut packs = BTreeMap::new();
    packs.insert(3u64, Pack {
        difficulty: None,
        case: "osce-a".into(),
        persona: Persona { name: "Anan Thepwong".into(), country: "THA".into(), age: 69, sex: "m".into() },
        portrait: Default::default(),
        endemic: false,
    });

    assert_eq!(beds_taken(&patients, &packs, nothing_lost(), &[]), 1,
               "one bed is taken — the two the ward cannot describe are on the chain and not in a \
                bed, or they wedge the ward shut against a queue that is full");

    let v = ward_payload(&vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 100, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(), unread: nothing_unread(), times: nothing_dated(), seconds_per_slot: None,
    });

    assert_eq!(v["census"]["on_ward"], 3,
               "the census does not look away from them: they are on the chain and the chain is \
                what the census counts");

    let by_id = |id: u64| v["patients"].as_array().unwrap().iter()
        .find(|p| p["patient_id"] == id).cloned().expect("listed");

    assert!(by_id(1)["bed"].is_null(), "she is in no bed");
    assert_eq!(by_id(1)["state"], "off_ward",
               "and the board says what she is rather than calling her one of the three");
    let adrift = by_id(1);
    let note = adrift["note"].as_str().expect("her row says why");
    assert!(note.contains("outside the ward"), "in words a stranger can read: {note}");

    assert_eq!(by_id(3)["bed"], 1, "the described patient has the first bed, not the third");
    assert!(by_id(3)["note"].is_null(), "and needs no explanation");
    assert!(by_id(4)["bed"].is_null(), "somebody who went home is in nobody's bed");
}

/// The rail must not lie by omission: six on the ward, three in beds.
///
/// `on_ward` is chain truth and stays so — she is on the chain. But a stranger reading "on the
/// ward 6" beside three patients has been told a number that means something other than what it
/// looks like, and the difference is the thing they would most want explained: three of those six
/// cannot be treated by anybody.
///
/// So the payload publishes both, with the sentence that separates them.
#[test]
fn the_payload_publishes_how_many_are_in_beds_beside_how_many_are_on_the_chain() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{Pack, Persona};

    let patients = vec![
        patient(1, OPEN, 0, 10, 0),
        patient(2, OPEN, 0, 20, 0),
        patient(3, OPEN, 2, 30, 0),
    ];
    let mut packs = BTreeMap::new();
    packs.insert(3u64, Pack {
        difficulty: None,
        case: "osce-a".into(),
        persona: Persona { name: "Anan".into(), country: "THA".into(), age: 69, sex: "m".into() },
        portrait: Default::default(),
        endemic: false,
    });

    let v = ward_payload(&vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 100, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(), unread: nothing_unread(), times: nothing_dated(), seconds_per_slot: None,
    });

    assert_eq!(v["census"]["on_ward"], 3, "the census is what the chain says, unchanged");
    assert_eq!(v["in_beds"], 1, "and the beds are what the ward can actually hand to a stranger");

    let how = v["derivations"]["in_beds"].as_str().expect("it says how it was counted");
    assert!(how.contains("pack"), "in the terms that explain the gap: {how}");
}


/// **The board draws the small one when there is one.**
///
/// `portrait` is what to draw now and `portraits` is everything there is to draw. The board is a
/// globe of up to twenty faces and the bedside is one face at full size, so the board's `portrait`
/// takes the 256 px sibling when the factory has made it and the full-size picture when it has
/// not — never a broken image, and never a thumbnail at the bedside.
#[test]
fn the_board_takes_the_small_face_and_the_bedside_the_full_one() {
    use vitals_web::ward::{portrait_for, portrait_small_for};

    let base = "https://storage.googleapis.com/vitals-world-portraits/".to_string();
    let full = format!("{base}{}.webp", "a".repeat(64));
    let small = format!("{base}{}-256.webp", "a".repeat(64));
    let worse = format!("{base}{}.webp", "b".repeat(64));

    let mut set = std::collections::BTreeMap::new();
    set.insert("stable".to_string(), full.clone());
    set.insert("stable_256".to_string(), small.clone());
    set.insert("critical".to_string(), worse.clone());

    assert_eq!(portrait_small_for(&set, "stable"), Some(small.as_str()),
               "the board asks for a thumbnail and gets one");
    assert_eq!(portrait_for(&set, "stable"), Some(full.as_str()),
               "the bedside asks for her picture and gets the picture");

    assert_eq!(portrait_small_for(&set, "critical"), Some(worse.as_str()),
               "no sibling for this state yet, so the board draws the full one rather than nothing");

    // And the milder-state rule is the same rule: it picks the state first, then the size.
    assert_eq!(portrait_small_for(&set, "arrest"), Some(worse.as_str()),
               "arrest has no picture, so the nearest milder state's does — and its size follows it");
    assert_eq!(portrait_small_for(&set, "recovered"), None,
               "a patient who went home has no picture until one of her leaving is made");
}

/// **A patient the ward cannot rebuild is named on the board and gives up the bed.**
///
/// It happened on 16 ก.ย.: an anchor put a leaf on chain whose tape was never kept, and the
/// patient sat in bed 3 where nobody could open her. The ticker's repair puts the tape back when a
/// session still holds it; this is what the board says when nothing does.
///
/// **Not closed on chain**, and that is the founder's rule about the record read the other way:
/// the program closes a patient on death or on discharge, and a discharge nobody gave would be a
/// lie on the one record we are asking strangers to trust. So the chain keeps her open and the
/// board — which is ours, and derived — says what is true: the chain names a shift whose tape is
/// gone, so nobody can take this bed.
#[test]
fn a_patient_who_cannot_be_rebuilt_is_named_and_gives_up_the_bed() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{beds_taken, Pack, Persona};

    let patients = vec![patient(7, OPEN, 1, 10, 0), patient(8, OPEN, 0, 20, 0)];
    let describable = |id: u64, name: &str| {
        (id, Pack {
            case: "osce-c".into(),
            difficulty: None,
            persona: Persona { name: name.into(), country: "THA".into(), age: 6, sex: "f".into() },
            portrait: Default::default(),
            endemic: false,
        })
    };
    let packs: BTreeMap<u64, Pack> =
        [describable(7, "Fon"), describable(8, "Ploy")].into_iter().collect();

    let whole = ward_payload(&read(&patients, &[], &packs, None, 100));
    assert_eq!(whole["in_beds"], 2, "two describable patients, two beds");

    // Now one of them names a tape this ward does not have.
    let leaf = "9".repeat(64);
    let lost: BTreeMap<u64, String> = [(8u64, leaf.clone())].into_iter().collect();
    let mut r = read(&patients, &[], &packs, None, 100);
    r.unrebuildable = &lost;
    let v = ward_payload(&r);

    let her = v["patients"].as_array().expect("a board").iter()
        .find(|p| p["patient_id"] == 8).expect("still on the board").clone();
    assert_eq!(her["state"], "unrebuildable", "her own word — not died, and not went home");
    let note = her["note"].as_str().unwrap_or_default();
    assert!(note.contains(&leaf[..16]), "the note names the leaf it stopped at: {note}");
    assert!(her["bed"].is_null(), "and she holds no bed nobody can take");

    assert_eq!(v["in_beds"], 1, "the other bed is free for the ticker to fill");
    assert_eq!(v["census"]["unrebuildable"], 1, "counted under her own word");
    assert_eq!(v["census"]["died"], 0, "not as a death");
    assert_eq!(v["census"]["went_home"], 0, "and not as a discharge");
    assert_eq!(v["census"]["on_ward"], 2,
               "the chain still holds her open and the census is the chain's arithmetic — the \
                board is where the ward says what it can and cannot do with her");

    // The count the ticker refills against agrees with the board.
    assert_eq!(beds_taken(&patients, &packs, &lost, &[]), 1);
}

/// **At the bedside the frame is never empty.**
///
/// The board's rule is strict and right: a patient who went home does not borrow the picture of
/// herself ill in a bed, so `portrait_for` answers nothing when no picture at or below her state
/// exists. At the bedside that same nothing is a black frame in front of somebody who is still in
/// the room with her — and on a discharge, which is where it happens, it is the last thing they
/// see of a patient they just treated.
///
/// So the bedside asks a different question: her own picture if there is one, the nearest milder
/// one if not, and failing both the nearest picture that exists at all. The board's answer is
/// unchanged.
#[test]
fn the_bedside_never_shows_an_empty_frame() {
    use vitals_web::ward::{portrait_at_the_bedside, portrait_for};

    let base = "https://storage.googleapis.com/vitals-world-portraits/";
    let stable = format!("{base}{}.webp", "a".repeat(64));
    let set: std::collections::BTreeMap<String, String> =
        [("stable".to_string(), stable.clone())].into_iter().collect();

    assert_eq!(portrait_for(&set, "recovered"), None,
               "the board says nothing rather than showing her ill in a bed she has left");
    assert_eq!(portrait_at_the_bedside(&set, "recovered"), Some(stable.as_str()),
               "the bedside shows the last face there is, because the alternative is a black frame");
    assert_eq!(portrait_at_the_bedside(&set, "critical"), Some(stable.as_str()),
               "and the milder-state rule is unchanged where it applies");
    assert_eq!(portrait_at_the_bedside(&Default::default(), "stable"), None,
               "a patient with no pictures at all still has none");
}

/// **A bed is a number a patient keeps for her whole stay.**
///
/// It was her position among the open patients, so when Salma left bed 2 the man in bed 3 became
/// the man in bed 2 — while the founder was being told "Yonas เตียง 3". A bed that renumbers
/// itself when somebody else goes home is not a bed; it is an index.
///
/// Still derived, and still from the chain alone: walk the admissions and the closings in slot
/// order, give each arrival the lowest free number, and hand it back when she leaves. Two readers
/// of the same chain get the same beds, and nobody has to keep a note.
#[test]
fn a_bed_is_kept_for_the_whole_stay() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{beds_of, Pack, Persona};

    let pack = |name: &str| Pack {
        difficulty: None,
        case: "osce-c".into(),
        persona: Persona { name: name.into(), country: "THA".into(), age: 6, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    let packs: BTreeMap<u64, Pack> =
        [(1, pack("Anita")), (2, pack("Salma")), (3, pack("Yonas"))].into_iter().collect();

    // Three admitted in order; nobody has left.
    let three = vec![patient(1, OPEN, 0, 10, 0), patient(2, OPEN, 0, 20, 0), patient(3, OPEN, 0, 30, 0)];
    let beds = beds_of(&three, &packs, &Default::default(), &[]);
    assert_eq!((beds[&1], beds[&2], beds[&3]), (1, 2, 3), "in the order they arrived");

    // Salma goes home. Yonas keeps bed 3 — he was told it, and nothing about him changed.
    let after = vec![patient(1, OPEN, 0, 10, 0), patient(2, DISCHARGED, 1, 20, 40), patient(3, OPEN, 0, 30, 0)];
    let beds = beds_of(&after, &packs, &Default::default(), &[]);
    assert_eq!(beds.get(&2), None, "a patient who left holds no bed");
    assert_eq!((beds[&1], beds[&3]), (1, 3), "and nobody else moves");

    // The next admission takes the bed that was freed, which is the lowest free one.
    let next = vec![
        patient(1, OPEN, 0, 10, 0), patient(2, DISCHARGED, 1, 20, 40),
        patient(3, OPEN, 0, 30, 0), patient(4, OPEN, 0, 50, 0),
    ];
    let mut packs4 = packs.clone();
    packs4.insert(4, pack("Kwame"));
    let beds = beds_of(&next, &packs4, &Default::default(), &[]);
    assert_eq!(beds[&4], 2, "the freed bed is the one the ticker fills");
    assert_eq!((beds[&1], beds[&3]), (1, 3), "and the other two are where they were");

    // A patient the ward cannot describe or cannot rebuild holds no bed, as before.
    let lost: BTreeMap<u64, String> = [(3u64, "9".repeat(64))].into_iter().collect();
    let beds = beds_of(&next, &packs4, &lost, &[]);
    assert_eq!(beds.get(&3), None, "unrebuildable holds nothing");
    assert_eq!(beds[&4], 2, "and the numbers of the others do not move because of it");
}

/// **What a patient is on, and how hard it is, for a ward whose cases came through its own door.**
///
/// The board has carried `case` since the first globe. `difficulty` beside it was read from the
/// season's own table — the sixteen ids and their levels — so from the moment the ward started
/// playing compiled cases every patient on the board would have said `difficulty: null`. The
/// factory picks the next patient partly by what is already on the board, at a level mix, so a
/// null there is not a cosmetic gap: it is the number that mix is computed from.
///
/// The level comes from the case's own pack now, and the season's table is the fallback for the
/// patients still mid-stay on season cases.
#[test]
fn the_board_says_which_case_each_patient_is_on_and_how_hard_it_is() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{Pack, Persona};
    use vitals_web::ward_case::CaseSummary;

    let pack = |case: &str| Pack {
        case: case.into(),
        difficulty: None,
        persona: Persona { name: "Anita".into(), country: "NPL".into(), age: 34, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    let packs: BTreeMap<u64, Pack> =
        [(1, pack("dengue-npl-1")), (2, pack("osce-c"))].into_iter().collect();
    let held = vec![CaseSummary {
        case_id: "dengue-npl-1".into(),
        archetype: "dengue_shock".into(),
        patient_age: Some(34),
        patient_sex: Some("female".into()),
        country: Some("NPL".into()),
        difficulty: "intern".into(),
        endemic: true,
        provisional: true,
        withdrawn: false,
        version: "0.1.0".into(),
        title: "ไข้เลือดออก".into(),
    }];

    let patients = vec![patient(1, OPEN, 0, 10, 0), patient(2, OPEN, 0, 20, 0)];
    let mut r = read(&patients, &[], &packs, None, 100);
    r.cases = &held;
    let v = ward_payload(&r);
    let row = |id: u64| {
        v["patients"].as_array().unwrap().iter().find(|p| p["patient_id"] == id).unwrap().clone()
    };

    assert_eq!(row(1)["case"], "dengue-npl-1", "the case she was admitted onto");
    assert_eq!(row(1)["difficulty"], "intern", "at the level the case factory compiled it for");
    // `endemic` on a bed stays the word of whoever drew *her*, not the case's own tag. A case can
    // be endemic in Nepal and ordinary here, so a patient's row says whether she was drawn from
    // her country's list — and this pack was not, whatever the case is. The case's own tag is in
    // the catalogue, where it is a fact about the case.
    assert_eq!(row(1)["endemic"], false, "the drawer's word about her, never the case's about itself");

    // The three mid-stay on season cases keep their level from the season's table until they go.
    assert_eq!(row(2)["case"], "osce-c");
    assert_eq!(row(2)["difficulty"], "resident");
}

/// **The policy block says what this ward actually does.**
///
/// `/api/ward` publishes the rules beside the numbers, because a census nobody can check is a
/// claim. Three of those rules stopped being true the day the ward started playing what the case
/// factory compiles, and they were the three most specific ones:
///
///   * `catalogue` listed the season's sixteen ids — `ep2`, `osce-d4` — on a ward that refuses
///     every one of them at its own door;
///   * `difficulty` counted those sixteen into bands, so a reader filtering for a student case was
///     told there are two, from a table with nothing in it any more;
///   * `draw` said cases are drawn uniformly from that catalogue, which was never how the queue
///     works and is not how a bed is filled.
///
/// A wrong rule published beside a right number is worse than no rule: the number is checkable and
/// the sentence is what a reader uses to decide whether checking is worth their time.
#[test]
fn the_policy_says_what_this_ward_actually_does() {
    use vitals_web::ward_case::CaseSummary;

    let case = |id: &str, level: &str, provisional: bool| CaseSummary {
        case_id: id.into(),
        archetype: "septic_shock".into(),
        patient_age: Some(40),
        patient_sex: Some("female".into()),
        country: Some("BGD".into()),
        difficulty: level.into(),
        endemic: false,
        provisional,
        withdrawn: false,
        version: "0.1.0".into(),
        title: "a compiled case".into(),
    };
    let held = vec![
        case("embla-typhoid-1", "intern", true),
        case("embla-dengue-1", "intern", false),
        case("embla-malaria-1", "resident", true),
        case("embla-croup-1", "student", true),
    ];

    let patients = vec![patient(1, OPEN, 0, 10, 0)];
    let packs = nobody();
    let mut r = read(&patients, &[], &packs, None, 100);
    r.cases = &held;
    let v = ward_payload(&r);
    let policy = &v["policy"];

    // What the catalogue is, and how much of it there is at this read.
    let cat = &policy["catalogue"];
    assert_eq!(cat["held"], 4, "the cases the ward is holding, counted at this read");
    assert_eq!(cat["provisional"], 3, "compiled, not clinically reviewed");
    assert_eq!(cat["reviewed"], 1);
    let where_from = cat["from"].as_str().unwrap_or_default();
    assert!(where_from.contains("case factory") && where_from.contains("embla-cases"),
            "and where they come from: {where_from:?}");
    let placed = cat["placed_by"].as_str().unwrap_or_default();
    assert!(placed.contains("sex") && placed.contains("age"),
            "a patient is placed on a case written about somebody like her: {placed:?}");

    // The levels, as the packs themselves carry them.
    assert_eq!(policy["difficulty"]["student"], 1);
    assert_eq!(policy["difficulty"]["intern"], 2);
    assert_eq!(policy["difficulty"]["resident"], 1);

    // How a bed is actually filled.
    let draw = policy["draw"].as_str().unwrap_or_default();
    assert!(draw.contains("queue"), "a bed is filled from the queue: {draw:?}");
    assert!(!draw.contains("uniformly"), "and not uniformly from a catalogue: {draw:?}");

    // And nothing in the whole block names one of the season's sixteen.
    let said = policy.to_string();
    for season in ["ep1", "ep2", "ep5", "osce-a", "osce-d4"] {
        assert!(!said.contains(season),
                "the policy still names {season:?}, which this ward refuses at its own door: {said}");
    }
}

/// A ward that cannot read its chain still knows its own rules — and does not invent a count it
/// could not take. `held: null` is "we did not look", which is a different fact from "none".
#[test]
fn the_policy_survives_a_chain_it_cannot_read() {
    let v = vitals_web::ward::ward_unavailable("devnet:ABC", "the RPC timed out");
    assert_eq!(v["readable"], false);
    let cat = &v["policy"]["catalogue"];
    assert!(cat["from"].as_str().is_some_and(|s| s.contains("case factory")),
            "the rule is still true when the chain is down");
    assert!(cat["held"].is_null(), "and a number nobody took is null, never zero: {cat}");
}

/// **The board says when somebody last finished a shift on her.**
///
/// The globe's row renders "on the ward · handed over 4 minutes ago" for a patient nobody is with
/// right now — it reads `handed_over` off the row — and the board has never published that field.
/// So every such row falls back to "admitted 6 hours ago", which is the one number that makes an
/// active ward look abandoned: a patient treated twice in the last hour reads exactly like a
/// patient nobody has touched since she arrived.
///
/// It is not a new fact and nothing needs to be written down for it. The chain already carries one
/// leaf per anchored shift, each with the slot it landed in, so the last of those *is* the last
/// hand-over. Derived at the read the same way `on_shift_since` is: the slot difference carried
/// back to wall time through this read's own slot.
#[test]
fn the_board_says_when_she_was_last_handed_over() {
    let now = 1_760_000_000u64;
    let as_of = 100_000u64;
    let patients = vec![patient(1, OPEN, 0, as_of - 1000, 0), patient(2, OPEN, 0, as_of - 1000, 0)];
    // Two shifts on patient 1, the later one 200 slots ago. Out of order on purpose: the chain
    // gives them in whatever order the accounts came back, and "the last one" is not "the last in
    // the list".
    let shifts = vec![
        shift_by(1, 7, as_of - 900),
        shift_by(1, 9, as_of - 200),
        shift_by(1, 8, as_of - 500),
    ];

    let packs = nobody();
    // The chain's time for each of the three slots her shifts landed in. The latest slot is the
    // last hand-over — and it is the chain's dating of that slot that the row shows, never the
    // distance from this read's own slot.
    let times = dated(&[
        (as_of - 900, now as i64 - 700), (as_of - 500, now as i64 - 400),
        (as_of - 200, now as i64 - 90),
    ]);
    let mut r = read(&patients, &shifts, &packs, None, as_of);
    r.times = &times;
    let v = ward_payload(&r);
    let row = |id: u64| {
        v["patients"].as_array().unwrap().iter().find(|p| p["patient_id"] == id).unwrap().clone()
    };

    let handed = row(1)["handed_over"].as_str().expect("the last hand-over, as a time").to_string();
    assert_eq!(handed, vitals_web::ward::utc_iso(now - 90), "the latest of her shifts, as the chain dates it");

    // Nobody has finished a shift on her: the field is null, and the globe falls back to the
    // admission the way it always has. Null rather than her admission time, because "nobody has
    // treated her yet" and "she was treated the moment she arrived" are different facts.
    assert!(row(2)["handed_over"].is_null(), "no shift, no hand-over: {}", row(2));
}

/// **A time on the board is the chain's own time for that slot.**
///
/// The board said Salma was admitted at `2026-09-14T15:13:09Z`. The chain says her admission
/// landed in slot 499139724, and devnet dates that slot `2026-09-16T05:30:17Z`: the board was 38
/// hours early, and the founder had been reading "admitted 3 days ago" over a patient admitted
/// yesterday. Every date on a chart and every "… ago" on the globe carried the same error, because
/// all of them were one subtraction — this read's slot minus that slot, times 0.4 seconds — and a
/// chain does not produce slots at its nominal rate for days on end.
///
/// The rule this pins: a time shown for a chain event is that slot's block time, which the chain
/// itself publishes and which never changes once the block exists. The slot count stays where it
/// is honest — the ticker's idle arithmetic, which is in slots and never leaves them.
///
/// A slot the ward has no block time for shows no time at all. "We have not looked that slot up
/// yet" and "it happened at 15:13" are different statements, and only one of them is ours to make.
#[test]
fn a_time_on_the_board_is_the_slots_own_block_time() {
    let admitted = 499_139_724u64;
    let handed = 499_201_815u64;
    // What devnet answers for those two slots, asked on 17 ก.ย.: getBlockTime, the chain's own
    // dating of its own blocks.
    let times: std::collections::BTreeMap<u64, i64> =
        [(admitted, 1_789_536_617i64), (handed, 1_789_546_902)].into_iter().collect();
    let as_of = 499_734_346u64;
    let now = 1_789_668_000u64;

    let patients = vec![
        patient(1, OPEN, 1, admitted, 0),
        // Admitted in a slot nobody has looked up. Her row carries the slot and no time.
        patient(2, OPEN, 0, admitted - 5_000, 0),
    ];
    let shifts = vec![shift_by(1, 7, handed)];
    let packs = nobody();
    let v = ward_payload(&vitals_web::ward::WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        patients: &patients, shifts: &shifts, packs: &packs, cases: &[],
        unrebuildable: nothing_lost(), unread: nothing_unread(), since: None, as_of_slot: as_of,
        now_unix: now, source: "devnet:ABC", times: &times, seconds_per_slot: None,
    });
    let row = |id: u64| {
        v["patients"].as_array().unwrap().iter().find(|p| p["patient_id"] == id).unwrap().clone()
    };

    assert_eq!(row(1)["admitted_at"], "2026-09-16T05:30:17Z",
               "the chain's time for the slot she was admitted in, to the second");
    assert_eq!(row(1)["handed_over"], "2026-09-16T08:21:42Z",
               "and for the slot her last shift was anchored in");
    assert_eq!(row(1)["admitted_slot"], admitted, "the slot itself stays on the row: it is the fact");

    assert!(row(2)["admitted_at"].is_null(),
            "a slot with no block time gets no time — never one worked out from the read's own \
             slot, which is the 38-hour error: {}", row(2));
}

/// **A head whose page has stopped beating is a head the ward takes back.**
///
/// The director measured it on staging: take a shift, close the tab, and the board still says
/// `on_shift` a minute later — the bed freed on its own about ten minutes in, when the lease ran
/// out. On three beds that is a third of the ward held by somebody who has gone.
///
/// So the page beats while it holds the head and the ward frees the head when the beats stop.
/// Every part of that decision is here, out of the thread that acts on it: given when each page
/// last beat and the time now, which heads should the ward take back. Two missed beats is the
/// rule — one missed beat is a page on a train.
///
/// A patient nobody has beaten for is not in the map at all and is not freed by this: her lease is
/// the net, and a shift that never beat is a client older than this rule.
#[test]
fn a_head_whose_page_stopped_beating_is_taken_back() {
    use vitals_web::ward::heads_to_free;
    let beat = 30_000u64;          // the page beats every thirty seconds
    let grace = 2 * beat + 15_000; // two missed beats, and a little for the wire
    let now = 1_789_700_000_000u64;

    let beats: std::collections::BTreeMap<u64, u64> = [
        (1u64, now - 5_000),            // beating
        (2, now - 31_000),              // one missed beat: a page on a train
        (3, now - grace - 1),           // gone
        (4, now - 10 * 60_000),         // long gone
    ].into_iter().collect();

    assert_eq!(heads_to_free(&beats, now, grace), vec![3, 4],
               "two missed beats is the rule, and one is not");
    assert!(heads_to_free(&beats, now, grace).iter().all(|p| *p != 2),
            "a page that missed one beat still holds its patient — the cost of being wrong here is \
             taking a bed off somebody who is in the room");
    assert!(heads_to_free(&std::collections::BTreeMap::new(), now, grace).is_empty(),
            "a ward nobody is beating for frees nothing: a shift that never beat is a client older \
             than this rule, and her lease is the net under it");

    // A clock that went backwards (a host with NTP correcting itself) must not free the ward.
    let future: std::collections::BTreeMap<u64, u64> = [(9u64, now + 60_000)].into_iter().collect();
    assert!(heads_to_free(&future, now, grace).is_empty(),
            "a beat from the future is not a beat that stopped");
}

/// **A request never waits on a chain read when the ward already has a board.**
///
/// The founder opened the globe on 17 ก.ย. and asked where the patients had gone: the panel was
/// empty while `/api/ward` sat behind a chain read. The board is fifteen seconds old at most and
/// the read that refreshes it takes as long as devnet feels like taking — so the answer is to hand
/// over what the ward has and go and get the new one, rather than to make the person wait for it.
///
/// Three cases and they are all different: no board at all (the first request after a boot, and
/// the only one that waits), a board inside its life (serve it, touch nothing), and a board past
/// it (serve it anyway, and refresh behind the answer).
#[test]
fn a_board_is_served_while_the_next_one_is_read() {
    use std::time::Duration;
    use vitals_web::ward::{board_use, Board};
    // Nothing in the store, so this is the old question: what does the board in memory say?
    let stored = false;

    let ttl = Duration::from_secs(15);
    assert_eq!(board_use(None, stored, ttl), Board::Wait,
               "the first request after a boot is the one that waits, and it is the only one");
    assert_eq!(board_use(Some(Duration::from_secs(3)), stored, ttl), Board::Serve,
               "a board inside its life is the answer and nothing else happens");
    assert_eq!(board_use(Some(Duration::from_secs(16)), stored, ttl), Board::ServeAndRefresh,
               "a board past its life is still an answer — it says when it was read — and the \
                reading happens behind it rather than in front of the person");
    assert_eq!(board_use(Some(Duration::from_secs(600)), stored, ttl), Board::ServeAndRefresh,
               "however old it is: a board with its own `as_of` on it is a fact, and waiting ten \
                minutes for a fresher one is not an improvement on it");
}

/// **The lease is 3,450 slots, and how long that is today is measured rather than assumed.**
///
/// The page said 23 minutes because 3,450 × 0.4 s is 23 minutes. Devnet was producing slots at
/// 0.166 s on 17 ก.ย. — measured four ways against getBlockTime — so the lease was really 9.5
/// minutes, and the director's abandoned bed freed itself at exactly that. Every sentence that
/// says a number of minutes has to get it from the chain's own rate.
///
/// Both figures are published: the slots, which are the program's constant and never change, and
/// the minutes, which are what a person can act on and change with the chain's mood. A ward that
/// has not measured a rate publishes the slots and **no** minutes — never the nominal number,
/// which is the one that was wrong.
#[test]
fn the_lease_says_how_long_it_is_today_or_says_nothing() {
    let packs = nobody();
    let v = ward_payload(&read(&[], &[], &packs, None, 100));
    let p = &v["policy"]["lease"];
    assert_eq!(p["slots"], vitals_program::LEASE_SLOTS,
               "the program's own constant, which is the same on every chain");
    assert!(p["minutes_now"].is_null(),
            "a ward that has not measured its chain does not guess: {p}");
    assert!(p["measured"].as_str().unwrap_or_default().contains("block time"),
            "and it says how the measurement is made, so a reader can make it themselves: {p}");

    // A chain running at devnet's real rate on 17 ก.ย.
    let mut r = read(&[], &[], &packs, None, 100);
    r.seconds_per_slot = Some(0.166);
    let fast = ward_payload(&r);
    assert_eq!(fast["policy"]["lease"]["minutes_now"], 10,
               "3,450 slots at 0.166 s is nine and a half minutes — which a page says as about \
                ten, and may not say as twenty-three");
    assert_eq!(fast["policy"]["lease"]["seconds_per_slot_now"], 0.166);

    // And at the nominal rate the old number is right — which is the point: it is a measurement,
    // not a correction.
    let mut r = read(&[], &[], &packs, None, 100);
    r.seconds_per_slot = Some(0.4);
    assert_eq!(ward_payload(&r)["policy"]["lease"]["minutes_now"], 23);
}

/// **In preview the board publishes who is waiting, and never a bed for them.**
///
/// The founder's ruling of 18 ก.ย. A production ward with a closed door and an empty board proves
/// nothing in the week before the fair: nobody can see the patients it would hold. In preview the
/// factory fills the queue and the board says who is in it — a name, an age, a country, the case's
/// own title, the level, her face — so the globe can ring the countries they are from and a judge
/// meets a ward rather than a page about one.
///
/// What a waiting row may never carry: a bed (she is in none), and anything from her case beyond
/// its title (the sce is the case, and the case is what a learner is supposed to meet at a bedside
/// rather than read in a payload).
#[test]
fn a_waiting_patient_is_published_without_a_bed_and_without_her_case() {
    use vitals_web::ward::{Pack, Persona};

    let waiting = vec![
        ("pk-1".to_string(), Pack {
            case: "embla-typhoid-ileal-perforation".into(),
            difficulty: Some("intern".into()),
            persona: Persona { name: "Nusrat Jahan".into(), age: 64, sex: "f".into(), country: "BGD".into() },
            portrait: [("stable".to_string(),
                        "https://storage.googleapis.com/vitals-world-portraits/\
                         bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.webp".to_string())]
                .into_iter().collect(),
            endemic: true,
        }),
        ("pk-2".to_string(), Pack {
            case: "embla-dengue-shock".into(),
            difficulty: Some("student".into()),
            persona: Persona { name: "Anan Thepwong".into(), age: 69, sex: "m".into(), country: "THA".into() },
            portrait: Default::default(),
            endemic: false,
        }),
    ];

    // The catalogue the ward holds, so a waiting row can carry the case's own title with her name
    // and age in it — which is what a reader looking at a country wants to know is waiting there.
    let held = vec![vitals_web::ward_case::CaseSummary {
        case_id: "embla-typhoid-ileal-perforation".into(),
        archetype: "septic_shock".into(),
        // **Deliberately not the person waiting on it.** A case is authored about a patient of its
        // own — this one about a man of 31 — and the ward places whoever the factory drew inside
        // the band on it. The two were both 64 here until now, and a fixture where they agree is a
        // fixture that cannot tell which of them the title was filled from. Production could: a row
        // read "A woman of 64" beside an age of 31, and nobody reading the page could tell which
        // number was the patient's.
        patient_age: Some(31),
        patient_sex: Some("male".into()),
        country: Some("BGD".into()),
        difficulty: "intern".into(),
        endemic: true,
        provisional: true,
        withdrawn: false,
        version: "0.1.0".into(),
        // The placeholders the catalogue actually writes: a case is authored about a patient, and
        // which person arrives on it is the ward's business (`fill_persona` — no {name}, because a
        // case author does not know her).
        title: "A {sex_word} of {age}, fever for 3 weeks and now a sudden abdominal pain".into(),
    }];

    // The rows themselves, which is where the shape is decided. The board attaches them to its
    // queue block — `preview_door.rs` drives a real ward and reads them off `/api/ward`.
    let rows = vitals_web::ward::waiting_rows(&waiting, &held);
    assert_eq!(rows.len(), 2, "both of them");
    let first = &rows[0];
    assert_eq!(first["pack"], "pk-1", "addressed by the pack id, which is what /ward/waiting takes");
    assert_eq!(first["name"], "Nusrat Jahan");
    assert_eq!(first["age"], 64);
    assert_eq!(first["sex"], "f");
    assert_eq!(first["country"], "BGD");
    assert_eq!(first["difficulty"], "intern");
    assert_eq!(first["endemic"], true);
    assert!(first["portrait"].as_str().unwrap_or_default().ends_with(".webp"),
            "her face, the one the pack arrived with: {first}");
    assert_eq!(first["case_title"],
               "A woman of 64, fever for 3 weeks and now a sudden abdominal pain",
               "the title is filled for the person waiting on the case, not for the case's own \
                patient: she is 64 and a woman, and the case was written about a man of 31");
    // Said again as the reader sees it: the sentence and the field beside it are about one person.
    let title = first["case_title"].as_str().unwrap_or_default();
    assert!(title.contains(&first["age"].to_string()),
            "the age in the sentence is the age in the row: {title} / {}", first["age"]);
    assert!(rows[1]["case_title"].is_null(),
            "and null for a pack whose case this ward does not hold: {}", rows[1]);

    assert!(first["bed"].is_null(), "she is in no bed and the payload does not invent one");
    assert!(first["sce"].is_null() && first["patient_id"].is_null(),
            "no scenario and no patient id: she is not on the chain yet and her case is not a \
             payload to read — {first}");
    for key in ["state", "on_shift_since", "handed_over"] {
        assert!(first[key].is_null(), "{key} is a fact about a patient on the ward: {first}");
    }
}

/// **A page that says "no" offers the beds that are open.**
///
/// UX review F1. `/ward/abc`, `/shift/zzz` and a pack id nobody queued all answer with one sentence
/// and a link back to the globe. A stranger who mistyped a patient id, or followed a link to a
/// receipt that does not exist, is somebody who came here to treat somebody — and the answer sends
/// them back to a globe to start again.
///
/// So the refusal carries what the panel carries: the beds a stranger can take right now, in bed
/// order, each a link. Taken off the board this host already holds, so a refusal costs no chain
/// read; a ward whose board cannot be read offers nothing rather than guessing.
#[test]
fn a_refusal_offers_the_beds_that_are_open() {
    use vitals_web::ward::beds_to_offer;

    let board = serde_json::json!({
        "patients": [
            { "patient_id": 3, "state": "on_ward", "bed": 3, "name": "Ayesha Malik",
              "age": 57, "country": "PAK", "difficulty": "intern" },
            { "patient_id": 1, "state": "on_ward", "bed": 1, "name": "Nusrat Jahan",
              "age": 64, "country": "BGD", "difficulty": "resident" },
            // On shift: somebody is in the room with her, so she is not a bed to offer.
            { "patient_id": 2, "state": "on_shift", "bed": 2, "name": "Park Ji-woo", "age": 8 },
            // No bed: on the chain and unopenable here, which the board says in her own row.
            { "patient_id": 4, "state": "off_ward", "bed": null, "name": "Yonas Haile", "age": 16 },
            // Her stay ended.
            { "patient_id": 5, "state": "went_home", "bed": null, "name": "Grace Wanjiru", "age": 27 }
        ]
    });

    let open = beds_to_offer(&board);
    assert_eq!(open.iter().map(|p| p["bed"].as_u64().unwrap_or(0)).collect::<Vec<_>>(), vec![1, 3],
               "in bed order, and only the ones a stranger can actually take");
    assert_eq!(open[0]["name"], "Nusrat Jahan");
    assert_eq!(open[0]["patient_id"], 1, "the id is the link");

    // A board that could not be read offers nothing. "We could not look" and "there are none" are
    // different facts, and only one of them is ours to say on a page about something else.
    assert!(beds_to_offer(&serde_json::json!({ "readable": false })).is_empty());
    assert!(beds_to_offer(&serde_json::json!({})).is_empty());
    assert!(beds_to_offer(&serde_json::json!({ "patients": [] })).is_empty());
}

/// **A fresh instance answers from the board its predecessor kept.**
///
/// Measured on staging, 18 ก.ย.: the first `/api/ward` against a cold instance took **123.33 s** —
/// eighteen patients' signature listings, in turn, with a reader watching a blank panel. The board
/// is held in memory, so at MIN_INSTANCES=0 every cold start has exactly one visitor paying for it,
/// and `board_use`'s own comment admits as much: "the first request after a boot pays for the read,
/// and it is the only one that does".
///
/// It does not have to be anyone. The board is a derived value with its own `as_of`, so the last
/// one this ward read can be kept in the store and served by whichever instance starts next —
/// answered immediately, refreshed behind the answer. `Wait` then means what it should: this ward
/// has never read the chain at all.
#[test]
fn a_fresh_instance_answers_from_the_board_its_predecessor_kept() {
    use std::time::Duration;
    use vitals_web::ward::{board_use, Board};
    let ttl = Duration::from_secs(30);

    assert_eq!(board_use(None, true, ttl), Board::ServeStoredAndRefresh,
               "nothing in memory and a board in the store: answer with it and read behind it — \
                the visitor who happens to be first must not pay for a chain read");
    assert_eq!(board_use(None, false, ttl), Board::Wait,
               "and only a ward that has never read the chain at all makes anybody wait");

    // Memory wins over the store whatever the store holds: it is this instance's own read, and it
    // is never older than the one it started from.
    assert_eq!(board_use(Some(Duration::from_secs(3)), true, ttl), Board::Serve);
    assert_eq!(board_use(Some(Duration::from_secs(600)), true, ttl), Board::ServeAndRefresh);
}

/// **A bed whose case the ward no longer holds is not offered.**
///
/// Two of the three patients in beds on staging on 18 September could not be played at all: Park
/// Ji-woo on `osce-c` and Yonas Haile on `osce-b2`, both admitted from the season's stations before
/// the case door existed, both still in beds after the catalogue moved on without them. `/api/new`
/// answered with no case content, so the page said "Not on this page yet" — *after* a stranger had
/// pressed "take a shift" on the board. The globe was offering two dead ends out of three.
///
/// She is still in a bed and the census still counts her, because she is: the ward admitted her,
/// her chart is on chain, and none of that changed. What changed is that this ward can no longer
/// draw her case, so the one thing it must not do is offer a door onto a blank page.
#[test]
fn a_bed_whose_case_the_ward_cannot_draw_is_not_offered() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{Pack, Persona, WardRead};
    use vitals_web::ward_case::CaseSummary;

    let held = |case: &str| Pack {
        difficulty: None,
        case: case.into(),
        persona: Persona { name: "Park Ji-woo".into(), country: "KOR".into(), age: 8, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    let in_the_catalogue = |id: &str| CaseSummary {
        case_id: id.into(),
        archetype: "haemorrhagic_shock".into(),
        country: Some("PHL".into()),
        difficulty: "resident".into(),
        endemic: false,
        provisional: true,
        withdrawn: false,
        version: "1.0.0".into(),
        title: "a case this ward holds".into(),
        patient_age: Some(69),
        patient_sex: Some("f".into()),
    };

    let patients = vec![patient(1, OPEN, 2, 10, 0), patient(2, OPEN, 2, 20, 0)];
    let mut packs = BTreeMap::new();
    packs.insert(1u64, held("osce-c"));
    packs.insert(2u64, held("ddx-boerhaave-4-en"));
    let catalogue = [in_the_catalogue("ddx-boerhaave-4-en")];

    let v = ward_payload(&WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        cases: &catalogue,
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 100, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(), unread: nothing_unread(), times: nothing_dated(), seconds_per_slot: None,
    });

    let by_id = |id: u64| v["patients"].as_array().unwrap().iter()
        .find(|p| p["patient_id"] == id).cloned().expect("listed");

    // The ruling that followed this test: she stays open on chain, and the *bed* goes back to the
    // ward, because the bed is the ward's furniture and not the chain's. So she is listed the way
    // the board already lists somebody on the chain and in no bed — and the reason is on her row.
    let shut = by_id(1);
    // Her own word, not `off_ward`. That one means the ward knows nothing about her — no pack, no
    // name, admitted outside the queue — and the globe page drops those rows on that understanding,
    // which made two named patients with charts and receipts vanish from it. Hers says what is
    // actually true: the ward has her, and cannot open her case.
    assert_eq!(shut["state"], "caseless",
               "the board has a word for a patient whose case it no longer holds, and it is not \
                the one that means it never knew her");
    assert!(shut["bed"].is_null(), "the bed is back in service for the next patient");
    assert_eq!(v["in_beds"], 1, "so the figures count one bed taken, not two");
    assert_eq!(shut["openable"], false,
               "and nobody is offered her, because this ward cannot draw her case");
    let why = shut["why_not"].as_str().expect("a reason a reader can act on");
    assert!(why.contains("case"),
            "it says what is missing rather than that something went wrong: {why}");

    let ok = by_id(2);
    assert_eq!(ok["openable"], true, "a bed whose case the ward holds is offered as it always was");
    assert!(ok["why_not"].is_null(), "and says nothing about why not — one broken bed closes no ward");
}

/// **A bed the ward cannot open is given back to the ward.**
///
/// Director's ruling, 18 September: she stays open on chain — nothing clinical happened to her and
/// the program must not be told otherwise — but the bed is the ward's, not the chain's, so the
/// ticker takes it back and admits somebody into it. She is listed as off the ward with the reason.
///
/// The precedent is already here: a patient whose chart cannot be rebuilt holds no bed, "because a
/// bed kept for her is a bed the ward has taken out of service without saying so". A patient whose
/// case the ward no longer holds is the same fact with a different cause.
#[test]
fn a_bed_the_ward_cannot_open_is_given_back_to_the_ward() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{beds_of, beds_taken, Pack, Persona};
    use vitals_web::ward_case::CaseSummary;

    let held = |case: &str| Pack {
        difficulty: None,
        case: case.into(),
        persona: Persona { name: "Park Ji-woo".into(), country: "KOR".into(), age: 8, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    let summary = |id: &str| CaseSummary {
        case_id: id.into(), archetype: "airway_obstruction".into(), country: None,
        difficulty: "intern".into(), endemic: false, provisional: true, withdrawn: false,
        version: "1.0.0".into(), title: "one the ward holds".into(),
        patient_age: None, patient_sex: None,
    };

    // She was admitted first and has the oldest slot, so she held bed 1 for two days.
    let patients = vec![patient(1, OPEN, 2, 10, 0), patient(2, OPEN, 0, 20, 0)];
    let mut packs = BTreeMap::new();
    packs.insert(1u64, held("osce-c"));
    packs.insert(2u64, held("ddx-boerhaave-4-en"));
    let catalogue = [summary("ddx-boerhaave-4-en")];

    assert_eq!(beds_taken(&patients, &packs, nothing_lost(), &catalogue), 1,
               "one bed is taken: hers is not, because nobody can open it");

    let beds = beds_of(&patients, &packs, nothing_lost(), &catalogue);
    assert!(!beds.contains_key(&1), "she holds no bed, so the ticker can fill it");
    assert_eq!(beds.get(&2), Some(&1),
               "and the patient behind her takes bed 1 — a bed nobody can open is not a bed kept \
                warm out of politeness");

    // The guard that matters on a fresh instance: an empty catalogue judges nobody.
    assert_eq!(beds_taken(&patients, &packs, nothing_lost(), &[]), 2,
               "a ward that has not loaded its catalogue has not lost anybody's case");
}

/// **Every board says where it came from — and says the same thing every time it is asked.**
///
/// The first `/api/ward` against a fresh staging instance took 112 s and answered with the board the
/// previous revision had kept; a second read 90 s later answered the same board in 0.11 s. So the
/// keeping works and the serving works, and the time sits in front of both — and nothing in the
/// answer said which, so it could only be guessed at.
///
/// The first attempt at saying it published an age, and the second published where the *answer*
/// came from. Both changed between two reads of one board, and `egress.rs` refused them: this
/// payload's ETag is its bytes, so a field that moves gives every open page a fresh download on
/// every poll — on the endpoint whose caching exists because a room full of people opens it at
/// once. Memory is not an origin; it is a cache. A board is read from the chain by this process or
/// inherited from the last one, and it stays whichever it was for as long as it is the board.
#[test]
fn every_board_says_where_it_came_from() {
    use vitals_web::ward::{board_note, Origin};

    let mine = board_note(Origin::Chain, 1_789_740_000, None);
    assert_eq!(mine["from"], "chain", "this process read it");
    assert_eq!(mine["kept_at"], 1_789_740_000, "when, not how long ago: an age ticks");
    assert!(mine["kept_by"].is_null(), "and there is nobody else to name");

    let inherited = board_note(Origin::Store, 1_789_730_000, Some("vitals-world-00056-h8k"));
    assert_eq!(inherited["from"], "store", "a fresh instance served what the last one left");
    assert_eq!(inherited["kept_at"], 1_789_730_000);
    assert_eq!(inherited["kept_by"], "vitals-world-00056-h8k",
               "and names the deploy that read it, so a slow first request can be attributed");

    // The property the ETag rests on, and the one both earlier attempts broke.
    assert_eq!(board_note(Origin::Store, 1_789_730_000, Some("r")),
               board_note(Origin::Store, 1_789_730_000, Some("r")),
               "one board, one set of bytes — however many times it is asked for");
}

/// **A head nobody holds is held by nobody — not by the key whose bytes happen to be zero.**
///
/// `/api/ward/declare` refuses anybody who is not holding the patient, by comparing the chain's
/// `lease_holder` against the caller's key. A patient nobody holds carries `[0; 32]` there, and the
/// base58 key `1111…1` (32 ones) *is* thirty-two zero bytes — so that one caller matched the
/// sentinel and was told to go ahead.
///
/// Found by Forseti VW-API-23a, which has used that placeholder since it was written. It only
/// failed now because the bed happened to be free: for as long as somebody else held the head, the
/// comparison failed for the right reason and the scenario passed for the wrong one.
///
/// Not exploitable — nobody signs with the zero key, and the program refuses the anchor anyway
/// (`AnchorShift` checks `lease_holder != account.id`). It is the ward saying "go ahead" where it
/// means "take the shift first", which is a sentence about a bed and therefore worth being right.
/// The program has known this all along: `PatientAccount::lease_free` tests the zero sentinel
/// first, and `ward::on_shift` does the same.
#[test]
fn a_head_nobody_holds_is_held_by_nobody() {
    use vitals_web::ward::may_declare;

    let alice = [7u8; 32];
    let bob = [9u8; 32];
    let nobody = [0u8; 32];

    assert!(may_declare(alice, alice), "the key the chain says is holding her may declare");
    assert!(!may_declare(alice, bob), "and nobody else may");
    assert!(!may_declare(nobody, alice), "a head nobody holds cannot be declared on");
    assert!(!may_declare(nobody, nobody),
            "least of all by the key whose bytes are the same zeros the sentinel uses — this is \
             the one that answered 200 on staging");
}

/// **A patient whose history could not be refreshed says so on the row, and keeps the bed.**
///
/// The board is built from every patient's account (one `getProgramAccounts`, which succeeded) and
/// each patient's history (one signature listing each, one of which 429'd). Her account is right —
/// state, bed, lease, count — so the bed stays offered and nothing about `openable` changes. Her
/// chart may be a shift behind, so a stranger opening the bedside is told, in the row, before they
/// press anything. No pronoun: this code has a patient id, not a persona.
///
/// The sentence rides on the row and the list rides on the top level, so the page can show it where
/// the row is and a reader of the raw payload can see at a glance how much of the board is fresh.
#[test]
fn an_unrefreshed_history_is_said_on_the_row_and_the_bed_stays_offered() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{ward_payload, PatientOnChain, WardRead, OPEN};

    let patients = vec![
        PatientOnChain { patient_id: 1, state: OPEN, shifts: 1, admitted_slot: 50, closed_slot: 0,
                         lease_holder: [0; 32], lease_until_slot: 0 },
        PatientOnChain { patient_id: 2, state: OPEN, shifts: 1, admitted_slot: 50, closed_slot: 0,
                         lease_holder: [0; 32], lease_until_slot: 0 },
    ];
    let mut unread = BTreeMap::new();
    unread.insert(2u64, "HTTP status client error (429 Too Many Requests)".to_string());

    let v = ward_payload(&WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        patients: &patients, shifts: &[], packs: &BTreeMap::new(), since: None, as_of_slot: 1_000,
        now_unix: 1_760_000_000, source: "devnet:ABC", times: &BTreeMap::new(),
        seconds_per_slot: None, unrebuildable: &BTreeMap::new(), cases: &[],
        unread: &unread,
    });

    let rows = v["patients"].as_array().expect("rows");
    let fresh = rows.iter().find(|r| r["patient_id"] == 1).expect("patient 1");
    let stale = rows.iter().find(|r| r["patient_id"] == 2).expect("patient 2");

    assert!(fresh["history"].is_null(), "a fresh history has nothing to add to the row");
    assert_eq!(stale["history"], "history not refreshed this minute",
               "and a stale one is said in words a stranger can act on");
    assert_eq!(stale["openable"], fresh["openable"],
               "the bed is not shut for a stale chart — the account that says there is a bed was \
                read fine, and it is the chart, not the bed, that may be behind");

    let listed = v["unread"].as_array().expect("the top level names every unrefreshed patient");
    assert_eq!(listed.len(), 1);
    assert!(listed[0].as_str().unwrap_or("").contains("2"),
            "by id, so the raw payload says how much of the board is fresh: {}", listed[0]);
    assert!(listed[0].as_str().unwrap_or("").contains("429"), "and why");

    let quiet = ward_payload(&WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        patients: &patients, shifts: &[], packs: &BTreeMap::new(), since: None, as_of_slot: 1_000,
        now_unix: 1_760_000_000, source: "devnet:ABC", times: &BTreeMap::new(),
        seconds_per_slot: None, unrebuildable: &BTreeMap::new(), cases: &[],
        unread: &BTreeMap::new(),
    });
    assert_eq!(quiet["unread"].as_array().map(Vec::len), Some(0),
               "an empty list on a good read, not an absent field — the shape of the payload does \
                not change with the weather, and neither does its ETag between two reads of one board");
}

/// **`/api/ward` says the catalogue's status in the one sentence, next to the counts.**
#[test]
fn the_board_publishes_the_catalogues_status_beside_its_counts() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{ward_payload, WardRead};
    use vitals_web::ward_chain::{catalogue_status, door_here};
    let v = ward_payload(&WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        patients: &[], shifts: &[], packs: &BTreeMap::new(), since: None, as_of_slot: 1_000,
        now_unix: 1_760_000_000, source: "devnet:ABC", times: &BTreeMap::new(),
        seconds_per_slot: None, unrebuildable: &BTreeMap::new(), cases: &[], unread: &BTreeMap::new(),
    });
    // The payload composes it from the door this host is serving behind, which under test is the
    // default — so the assertion is that the board shows *the same* sentence, not a fixed one.
    assert_eq!(v["policy"]["catalogue"]["status"], catalogue_status(door_here()),
               "the same sentence the bedside and the catalogue page show, from the same constant");
}

/// **The catalogue's counts are of cases a patient can be placed on.**
///
/// `held`/`provisional`/`reviewed` counted every pack in the store, withdrawn ones included, while
/// every other rule about a withdrawn case says it is out of service — the placement rule filters
/// them (`ward_case.rs:327`), the door refuses a pack naming one, and the catalogue page folds
/// them into a drawer. So on staging 20 ก.ย. the board said `held 138` where 75 were placeable and
/// 63 were withdrawn, and `/stats` showed 138 beside a ward that could only ever use 75.
///
/// A withdrawn case is still *held* in the plain sense — the ward keeps every case it was ever
/// sent, because shifts already played on one still have to replay. So the count that a reader
/// acts on is of the placeable, and the withdrawn get their own number rather than being hidden:
/// `held + withdrawn` is what is in the store, and `held` is what the ward can use tonight.
#[test]
fn the_catalogue_counts_what_a_patient_can_be_placed_on() {
    use std::collections::BTreeMap;
    use vitals_web::ward::{ward_payload, WardRead};
    use vitals_web::ward_case::CaseSummary;

    let case = |id: &str, provisional: bool, withdrawn: bool| CaseSummary {
        case_id: id.into(), archetype: "ddx".into(), country: None, difficulty: "intern".into(),
        endemic: false, provisional, withdrawn, version: "0.1.0".into(), title: id.into(),
        patient_age: Some(40), patient_sex: Some("female".into()),
    };
    let cases = [
        case("live-provisional", true, false),
        case("live-reviewed", false, false),
        case("gone-provisional", true, true),
        case("gone-reviewed", false, true),
    ];
    let v = ward_payload(&WardRead {
        // No ticker has run against these fixtures, so nobody has a clock — the same state as a
        // patient admitted since the last pass, whose card says nothing about one.
        closes_in: &NO_CLOCKS,
        patients: &[], shifts: &[], packs: &BTreeMap::new(), since: None, as_of_slot: 1_000,
        now_unix: 1_760_000_000, source: "devnet:ABC", times: &BTreeMap::new(),
        seconds_per_slot: None, unrebuildable: &BTreeMap::new(), cases: &cases,
        unread: &BTreeMap::new(),
    });
    let cat = &v["policy"]["catalogue"];
    assert_eq!(cat["held"], 2, "the cases a patient can be placed on, not every pack in the store");
    assert_eq!(cat["provisional"], 1, "…and its split, counted over the same two");
    assert_eq!(cat["reviewed"], 1);
    assert_eq!(cat["withdrawn"], 2,
               "out of service, and said rather than hidden: the ward keeps every case it was ever \
                sent because shifts played on one still have to replay");
}

/// **The door is a fact about this host, and the board is not where it lives.**
///
/// Measured on staging, 22 Sep: a revision composed its board while the door was open, the board
/// went to the store, the door was flipped to preview and a fresh instance came up. It served the
/// kept board — and the kept board carries the whole policy block as the previous revision composed
/// it, door and all. `/api/ward/cases` said "not yet open for play", because it composes live;
/// `/api/ward` said "open for play since 21 Sep 2026", because it did not. Both were the same host
/// answering at the same second. It stayed that way until the first tick.
///
/// Note which way that points on the night that matters. The sequence is deploy-in-preview, then
/// flip the door: in that direction a fresh instance says *not yet open* for as long as it serves
/// the kept board — after the door has opened, to the people arriving because it opened.
///
/// This is not the rule that says no field may change between two reads of the same board. That
/// rule is about patients, and it is why the board is kept at all. The door is not on the board:
/// it is a fact about the process answering, like the revision stamped beside it, and the sentence
/// composed from it is too.
#[test]
fn the_door_and_its_sentence_are_stamped_on_the_way_out_not_kept_with_the_board() {
    use vitals_web::ward::stamp_host;
    use vitals_web::ward_chain::{catalogue_status, Door};

    // The shape the store keeps: a board composed by a revision that was serving an open door.
    let mut kept = serde_json::json!({
        "queue": { "door": "open", "waiting": 20 },
        "policy": { "catalogue": { "status": catalogue_status(Door::Open), "held": 75 } },
        "patients": [],
    });

    // Served now, by a process that is behind a shut one.
    stamp_host(&mut kept, Door::Preview, "vitals-world-00072-xyz");

    assert_eq!(kept["queue"]["door"], "preview",
               "the door a reader is told about is the door they are actually standing at");
    assert_eq!(kept["policy"]["catalogue"]["status"], catalogue_status(Door::Preview),
               "and the sentence composed from it is composed from the same door");
    assert!(!kept["policy"]["catalogue"]["status"].as_str().unwrap_or_default().contains("2026"),
            "a shut door makes no claim about a date: {}", kept["policy"]["catalogue"]["status"]);
    assert_eq!(kept["revision"], "vitals-world-00072-xyz",
               "stamped at the same seam as the door, because it is the same kind of fact");

    // And nothing that *is* the board's is touched. The counts came off the chain read that built
    // this board and re-stamping them here would be inventing them.
    assert_eq!(kept["policy"]["catalogue"]["held"], 75);
    assert_eq!(kept["queue"]["waiting"], 20);
    assert_eq!(kept["patients"], serde_json::json!([]));
}


/// **A patient arrives every thirty minutes, whether or not anybody is watching.**
///
/// The founder, 22 ก.ย.: "ผมต้องการผลิตคนไข้มาเรื่อยๆ เพื่อสะท้อนปัญหาแพทย์ไม่พอ". A ward that only
/// admits when a bed frees is a ward whose census is a number we chose; a ward that keeps admitting
/// is one whose census is the shortage. At one arrival every thirty minutes and ten hours from
/// admission to an unattended death, a ward nobody plays settles near twenty patients and around
/// forty-eight deaths a day — and that is the figure to show, not hide.
///
/// The clock is read from **the chain**, not from a counter we keep: the last admission is
/// `admitted_slot` on the patients themselves, dated by the same slot-dater everything else on the
/// board uses. So the arrival rate is re-countable by anybody holding the program id, like every
/// other number here.
///
/// A ward that cannot say when the last patient arrived does not invent one. `None` is that case —
/// an unreadable chain, or a dater that could not date — and the answer is no. The bed-filling rule
/// above already covers a ward that has never admitted anybody, so nothing is lost by refusing, and
/// what is avoided is a burst of arrivals every time the dating fails.
#[test]
fn a_patient_is_due_on_the_clock_and_a_ward_that_cannot_tell_the_time_invents_nobody() {
    use vitals_web::ward::arrival_due;

    let half_hour = 30;
    let now = 1_790_000_000i64;

    assert!(arrival_due(Some(now - 1800), now, half_hour),
               "thirty minutes to the second is due");
    assert!(arrival_due(Some(now - 4000), now, half_hour), "and long past due is due");
    assert!(!arrival_due(Some(now - 1799), now, half_hour), "a second short is not");
    assert!(!arrival_due(Some(now), now, half_hour), "and one just admitted is not");

    assert!(!arrival_due(None, now, half_hour),
               "a ward that cannot say when the last patient arrived invents nobody — filling the \
                beds already covers a ward that has never admitted, so refusing costs nothing and \
                avoids a burst every time the dating fails");

    assert!(!arrival_due(Some(now - 99_999), now, 0),
               "and zero minutes turns the clock off rather than admitting on every pass");

    // A clock running backwards — a slot dated later than this host's own now — is not an arrival.
    assert!(!arrival_due(Some(now + 600), now, half_hour),
               "a last admission in the future is a disagreement about time, not a patient due");
}

// ── hand-overs read off the chain, not tallied as they happen ───────────────

/// **The Bangkok day a window opened, as the unix second a block time can be compared to.**
///
/// `funnel_since` is a Bangkok day and a slot carries a unix second. One of them has to cross, and
/// crossing it in two places is how a figure comes to mean two things. The case that matters is the
/// seven-hour overlap: a shift anchored at 16:00 UTC on 22 Sep is *before* a window that opened on
/// 23 Sep in Bangkok, even though a UTC reader would call 17:00 the same evening.
#[test]
fn a_bangkok_day_starts_seven_hours_before_utc_midnight() {
    use vitals_web::ward_chain::day_start_ict;

    // 2026-09-23 00:00 +07 is 2026-09-22 17:00 UTC.
    let start = day_start_ict("2026-09-23").expect("a day the ward wrote itself");
    // 1790096400 == 2026-09-22T17:00:00Z, checked against a date library rather than arithmetic
    // done by hand: the first constant written here was a day out, and the function was right.
    assert_eq!(start, 1_790_096_400, "23 Sep in Bangkok begins at 17:00 UTC on the 22nd");

    // A day is exactly a day later, leap years and month ends included.
    assert_eq!(day_start_ict("2026-09-24").unwrap() - start, 86_400);
    assert_eq!(
        day_start_ict("2026-10-01").unwrap() - day_start_ict("2026-09-30").unwrap(),
        86_400,
        "a month boundary is still one day"
    );
    assert_eq!(
        day_start_ict("2028-03-01").unwrap() - day_start_ict("2028-02-29").unwrap(),
        86_400,
        "and a leap day is a day"
    );

    // It is asked about a published figure, so it refuses anything that is not a date rather than
    // guessing a number out of it.
    for bad in ["", "2026-09", "2026-09-23T00:00", "yesterday", "2026-13-01", "2026-09-32"] {
        assert!(day_start_ict(bad).is_none(), "{bad} is not a day");
    }
}

/// **A hand-over is a shift a stranger anchored inside the window, and the chain already says so.**
///
/// Counted at read time off the board rather than tallied as it happens: a counter introduced today
/// starts at zero while the takes beside it have run since the window opened, and publishing those
/// two as a gap is the "2% of those who arrived" bug wearing a new field name. Every anchored shift
/// carries a signer and a slot, so nothing needs to be remembered.
///
/// Excluded **by signer, not by state**: the ward closes patients by anchoring a shift of its own,
/// and Amelia was handed over by a stranger and closed by the ward fifteen minutes later. Her
/// hand-over is one of these; the closure that followed is not.
#[test]
fn hand_overs_are_the_strangers_shifts_the_chain_dates_inside_the_window() {
    use vitals_web::ward::ShiftOnChain;
    use vitals_web::ward_chain::hand_overs_in_window;

    let ward = [0xAAu8; 32];
    let her = [0x11u8; 32];
    let him = [0x22u8; 32];
    let shift = |signer: [u8; 32], slot: u64| ShiftOnChain {
        patient_id: 1,
        run_hash: [0; 32],
        signer,
        slot,
    };
    let window = 1_000i64;
    // Slot 1 is before the window, 2 and 3 inside it, 9 is a slot nothing has dated yet.
    let dated = |slot: u64| -> Option<i64> {
        match slot {
            1 => Some(900),
            2 => Some(1_000),
            3 => Some(5_000),
            _ => None,
        }
    };

    let board = vec![
        (1u64, shift(her, 2)),    // a stranger, inside
        (2u64, shift(him, 3)),    // another stranger, inside
        (3u64, shift(her, 1)),    // a stranger, but before the window opened
        (4u64, shift(ward, 3)),   // the ward closing a patient: never a hand-over
        (5u64, shift(ward, 2)),   // and again
        (6u64, shift(him, 9)),    // a stranger whose slot has no block time yet
    ];

    let (n, undatable) = hand_overs_in_window(&board, &ward, &dated, window);
    assert_eq!(n, 2, "two strangers handed over inside the window");
    assert_eq!(undatable, 1, "and one could not be placed, which is said rather than swallowed");

    // The boundary belongs to the window: a shift anchored in the window's first second is in it.
    assert_eq!(hand_overs_in_window(&[(1, shift(her, 2))], &ward, &dated, 1_000).0, 1);
    assert_eq!(hand_overs_in_window(&[(1, shift(her, 2))], &ward, &dated, 1_001).0, 0);

    // A ward that has anchored nothing but closures has no hand-overs, not an error.
    let only_closures = vec![(1u64, shift(ward, 2)), (2u64, shift(ward, 3))];
    assert_eq!(hand_overs_in_window(&only_closures, &ward, &dated, window), (0, 0));
}

/// **When a gap is capped, and when it is the gap that actually happened.**
///
/// The founder's ruling has two halves that must both stay true: she gets worse for every hour
/// nobody comes, and whoever comes finds her with a shift's worth of time in hand. The first is the
/// ticker's uncapped rule; the second is this cap. What decides between them is not the patient and
/// not the clock — it is **whose moment the chart is being brought up to**.
///
/// Two conditions, and the devnet count is why each exists. An unconditional cap would have
/// rewritten every already-anchored shift's starting state: 23 production charts, every one of them
/// a ward closure, would have stopped re-deriving the leaf the chain holds.
///
/// * **Before the boundary slot, nothing is capped.** A shift anchored before the cap shipped was
///   played from the uncapped state and must keep deriving from it, or its leaf moves.
/// * **A shift the ward signed is never capped**, whenever it happened. The ticker's closures are
///   the uncapped half of the ruling; capping them would make the ward close patients on a
///   different patient than the one it killed.
///
/// A live arrival has no anchored shift at that slot at all — nobody has signed anything yet — and
/// that is the case the cap exists for.
#[test]
fn a_gap_is_capped_only_for_a_stranger_arriving_after_the_boundary() {
    use vitals_web::ward_chain::cap_on_arrival;

    let ward = [0xAAu8; 32];
    let her = [0x11u8; 32];
    let boundary = 1_000u64;

    // The case the cap is for: somebody arriving now, after the boundary, nothing signed yet.
    assert!(cap_on_arrival(1_500, None, boundary, Some(&ward)),
            "a stranger opening a bed after the boundary meets the capped state");

    // A human shift anchored after the boundary: capped, and it was played that way too, so its
    // leaf re-derives exactly as it was anchored.
    assert!(cap_on_arrival(1_500, Some(&her), boundary, Some(&ward)));

    // History. A human shift anchored before the boundary was played from the uncapped state and
    // keeps deriving from it — this is the condition that saves the charts already on chain.
    assert!(!cap_on_arrival(999, Some(&her), boundary, Some(&ward)),
            "a shift anchored before the cap shipped must derive as it was played");
    assert!(!cap_on_arrival(999, None, boundary, Some(&ward)));

    // The ward's own closures, never capped, on either side of the boundary. She still dies of
    // being alone; the cap is only what the person who comes is handed.
    assert!(!cap_on_arrival(1_500, Some(&ward), boundary, Some(&ward)),
            "a closure the ward signed is the uncapped half of the ruling");
    assert!(!cap_on_arrival(999, Some(&ward), boundary, Some(&ward)));

    // The boundary slot itself is inside the new rule: it is where the cap begins, not the last
    // slot before it.
    assert!(cap_on_arrival(1_000, None, boundary, Some(&ward)));
    assert!(!cap_on_arrival(999, None, boundary, Some(&ward)));

    // A ward that cannot name its own key cannot tell a closure from a stranger's shift, so it caps
    // nothing: the safe direction is the one that leaves every anchored chart deriving as it was.
    assert!(!cap_on_arrival(1_500, Some(&ward), boundary, None));
    assert!(!cap_on_arrival(1_500, None, boundary, None));

    // A boundary nobody has set is a cap that is off, so an unset deploy changes no patient.
    assert!(!cap_on_arrival(u64::MAX - 1, None, u64::MAX, Some(&ward)));
}
