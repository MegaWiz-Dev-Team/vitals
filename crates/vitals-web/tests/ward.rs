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
        patients, shifts, packs, since, as_of_slot, now_unix: 1_760_000_000, source: "devnet:ABC",
        // Every tape the chain names is here, which is the ward working. The one test about the
        // other case fills this in itself.
        unrebuildable: nothing_lost(),
        // No cases through the door: a level then comes from the season's table, which is what
        // the patients still mid-stay on season cases have. The test about a compiled case fills
        // this in itself.
        cases: &[],
    }
}

/// Nothing is missing — a `&'static` empty map, so every `WardRead` helper can borrow it.
fn nothing_lost() -> &'static std::collections::BTreeMap<u64, String> {
    static NONE: std::sync::OnceLock<std::collections::BTreeMap<u64, String>> =
        std::sync::OnceLock::new();
    NONE.get_or_init(Default::default)
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

    assert_eq!(p["beds"], 3);
    assert_eq!(p["a_bed_frees_on"], serde_json::json!(["discharge", "death"]),
               "nothing else frees a bed — not time, not us");
    assert!(p["admissions_per_day"].as_str().unwrap().contains("as many as leave"),
            "the rate is derived from the ward, never promised by us");
    assert!(p["draw"].as_str().unwrap().contains("queue"),
            "a bed is filled from the queue the factory fills, by the ticker on this host");
    assert!(p["draw"].as_str().unwrap().contains("another bed already holds"),
            "a case is not drawn while another copy of it is in a bed");

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
        shift(&mut st, &[], gap_slots);
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
/// The list is data, so the test is about the data: it may only name cases the ward can actually
/// serve. A file naming a case we never converted would put a patient on the board that no shift
/// could open, and the failure would arrive as a blank screen at a bed.
#[test]
fn the_endemic_list_may_only_name_cases_the_ward_can_serve() {
    use vitals_web::ward::{endemic, CATALOGUE, ENDEMIC_IN};

    assert_eq!(ENDEMIC_IN, 5, "one draw in five, for a country that has a list");

    for (country, cases) in endemic() {
        assert_eq!(country.len(), 3,
                   "{country} is not ISO 3166-1 alpha-3, and the globe matches on alpha-3");
        assert!(country.bytes().all(|b| b.is_ascii_uppercase()), "{country} must be upper case");
        assert!(!cases.is_empty(),
                "{country} carries an empty endemic list, which is a country that looks described \
                 and is not — leave it out instead");
        for case in cases {
            assert!(CATALOGUE.contains(&case.as_str()),
                    "{country} names {case}, which is not in the catalogue — a patient the board \
                     can show and no shift can open is a blank screen at a bed");
            assert!(vitals_web::ward::difficulty_of(&case).is_some(),
                    "{case} has no difficulty, so a player could not choose her by level");
        }
    }
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
    assert_eq!(v["policy"]["countries_with_an_endemic_list"], 0,
               "and today the honest count is zero — none of the converted sixteen belongs to a \
                place, and pairing one with a country anyway is the thing this rule exists to \
                stop");
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
    let v = ward_payload(&vitals_web::ward::WardRead {
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 4_000, now_unix: now, source: "devnet:ABC",
        unrebuildable: nothing_lost(),
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
    assert!(since.ends_with('Z'), "{since}");
    assert!(since < vitals_web::ward::utc_iso(now).as_str()
                && since > vitals_web::ward::utc_iso(now - 2 * 60 * 60).as_str(),
            "her shift started a plausible time ago, derived from the lease rather than from a \
             note this server kept: {since} against {}", vitals_web::ward::utc_iso(now));
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
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: lease_ends + 1, now_unix: now, source: "devnet:ABC",
        unrebuildable: nothing_lost(),
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
    assert_eq!(pool.len(), 20, "twenty countries, so the globe has something to light up");

    let globe = include_str!("../static/world/index.html");
    let mut seen_names: HashSet<String> = HashSet::new();

    for country in &pool {
        assert_eq!(country.country.len(), 3, "{} is not alpha-3", country.country);
        assert!(globe.contains(&format!("\"{}\":", country.country)),
                "the globe's own table cannot place {} — a patient from there would sit in the \
                 Unknown tray for ever", country.country);
        assert_eq!(country.personas.len(), 3, "{} must carry three people", country.country);

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
            patients: &patients, shifts: &[], packs: &packs,
            since: Some(1), as_of_slot: 4_000, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(),
        cases: &[],
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

    assert_eq!(beds_taken(&patients, &packs, nothing_lost()), 1,
               "one bed is taken — the two the ward cannot describe are on the chain and not in a \
                bed, or they wedge the ward shut against a queue that is full");

    let v = ward_payload(&vitals_web::ward::WardRead {
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 100, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(),
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
        cases: &[],
        patients: &patients, shifts: &[], packs: &packs,
        since: None, as_of_slot: 100, now_unix: 1_760_000_000, source: "devnet:ABC",
        unrebuildable: nothing_lost(),
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
    assert_eq!(beds_taken(&patients, &packs, &lost), 1);
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
    let beds = beds_of(&three, &packs, &Default::default());
    assert_eq!((beds[&1], beds[&2], beds[&3]), (1, 2, 3), "in the order they arrived");

    // Salma goes home. Yonas keeps bed 3 — he was told it, and nothing about him changed.
    let after = vec![patient(1, OPEN, 0, 10, 0), patient(2, DISCHARGED, 1, 20, 40), patient(3, OPEN, 0, 30, 0)];
    let beds = beds_of(&after, &packs, &Default::default());
    assert_eq!(beds.get(&2), None, "a patient who left holds no bed");
    assert_eq!((beds[&1], beds[&3]), (1, 3), "and nobody else moves");

    // The next admission takes the bed that was freed, which is the lowest free one.
    let next = vec![
        patient(1, OPEN, 0, 10, 0), patient(2, DISCHARGED, 1, 20, 40),
        patient(3, OPEN, 0, 30, 0), patient(4, OPEN, 0, 50, 0),
    ];
    let mut packs4 = packs.clone();
    packs4.insert(4, pack("Kwame"));
    let beds = beds_of(&next, &packs4, &Default::default());
    assert_eq!(beds[&4], 2, "the freed bed is the one the ticker fills");
    assert_eq!((beds[&1], beds[&3]), (1, 3), "and the other two are where they were");

    // A patient the ward cannot describe or cannot rebuild holds no bed, as before.
    let lost: BTreeMap<u64, String> = [(3u64, "9".repeat(64))].into_iter().collect();
    let beds = beds_of(&next, &packs4, &lost);
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
    let v = ward_payload(&read(&patients, &shifts, &packs, None, as_of));
    let row = |id: u64| {
        v["patients"].as_array().unwrap().iter().find(|p| p["patient_id"] == id).unwrap().clone()
    };

    let handed = row(1)["handed_over"].as_str().expect("the last hand-over, as a time").to_string();
    let want = vitals_web::ward::utc_iso(now - (200.0 * vitals_replay::SLOT_SECONDS) as u64);
    assert_eq!(handed, want, "the latest of her shifts, carried to wall time");

    // Nobody has finished a shift on her: the field is null, and the globe falls back to the
    // admission the way it always has. Null rather than her admission time, because "nobody has
    // treated her yet" and "she was treated the moment she arrived" are different facts.
    assert!(row(2)["handed_over"].is_null(), "no shift, no hand-over: {}", row(2));
}
