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

/// No pack has been queued for anyone — the state every test but one is written against, and the
/// state a dev deploy is in before the factory runs.
fn nobody() -> std::collections::BTreeMap<u64, vitals_web::ward::Pack> {
    std::collections::BTreeMap::new()
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

    let v = ward_payload(&patients, &shifts, &nobody(), Some(20), 1234, "devnet:ABC");

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
    let v = ward_payload(&[], &[], &nobody(), None, 7, "devnet:ABC");
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
    let v = ward_payload(&[], &[], &nobody(), None, 1, "devnet:ABC");
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
    assert!(v["cumulative"].is_null() && v["week"].is_null(),
            "no numbers at all — a zero here would be read out as 'nobody came'");
    assert!(v["policy"].is_object(), "the rule is still true when the chain is unreachable");

    let ok = ward_payload(&[], &[], &nobody(), None, 1, "devnet:ABC");
    assert_eq!(ok["readable"], true, "and a readable chain says that too, so the card can tell them apart");
}

// ── the idle clock, against the catalogue it will actually run on ──────────

use std::path::PathBuf;
use vitals_replay::{resume, shift};
use vitals_web::ward::CATALOGUE;

fn sce_path(id: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if id.starts_with("osce-") {
        root.join("demo/stations").join(format!("{id}.sce.json"))
    } else {
        root.join("demo/scenarios").join(format!("{id}.json"))
    }
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

/// **The ward does not kill patients nobody visited.**
///
/// The idle clock is what makes an unwatched bed a ward rather than a save file, and the cap is
/// what keeps it a ward rather than a mortuary: it is set below the fastest untreated arrest in
/// the catalogue, so a gap — however long — can only ever deteriorate her. Death then happens
/// only inside a shift, which is what "the record says who did it" has to mean. A key's actions
/// or inaction during their own shift is the whole claim; a patient dying of a gap nobody chose
/// would have the harm on nobody's record at all.
///
/// This walks every case the ward can admit, because the guarantee is about the catalogue and not
/// about one case. A new case that arrests faster than the cap fails here, which is the point:
/// the constant then has to be revisited rather than quietly becoming false.
#[test]
fn no_case_in_the_catalogue_dies_of_the_idle_clock_alone() {
    let mut killed = Vec::new();
    let mut fastest: Option<(String, u32)> = None;

    for id in CATALOGUE {
        let p = sce_path(id);
        let sce = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));

        if let Some(t) = arrests_at(&sce) {
            if fastest.as_ref().is_none_or(|(_, best)| t < *best) {
                fastest = Some((id.to_string(), t));
            }
        }

        // The longest gap the chain can express. The cap is the only thing between it and her.
        let (mut st, _) = resume(&sce, &[]).expect("scenario loads");
        shift(&mut st, &[], u64::MAX);
        if let Some(o) = st.outcome() {
            killed.push(format!("{id}: {o:?} at {:.0} s", st.t_sec()));
        }
    }

    assert!(killed.is_empty(),
            "the idle clock killed {} of {} catalogue patients with nobody in the room — the cap \
             has to sit below the fastest untreated arrest ({}), and these died: {}",
            killed.len(), CATALOGUE.len(),
            fastest.map(|(id, t)| format!("{id} at {t} s")).unwrap_or_else(|| "none".into()),
            killed.join(" · "));
}

/// **A stay is three cases**, and the endpoint says so rather than leaving it to be inferred.
///
/// Producer's ruling of 16 ก.ย. under the founder's go: a patient's chain is three existing cases
/// joined mechanically — acute, then observation, then ward-to-home — drawn by the same no-repeat
/// rule as the beds. What it buys is that one patient spans at least three shifts, so the ward
/// turns over slowly and a stranger arriving at noon meets somebody another stranger already
/// treated rather than a fresh admission nobody has touched.
///
/// It belongs in `policy` beside the beds because both answer the same question a reader has —
/// *how fast does this thing consume patients?* — and an unreadable chain must not take the answer
/// with it.
#[test]
fn a_stay_is_three_cases_and_the_policy_publishes_it() {
    use vitals_web::ward::{ward_unavailable, Stay, STAY_CASES};

    assert_eq!(STAY_CASES, 3);

    let mut stay = Stay::new(1, vec!["osce-a".into(), "osce-c".into(), "ep2-stemi".into()]);
    let mut shifts = 1;
    while stay.advance().is_some() {
        shifts += 1;
    }
    assert_eq!(shifts, STAY_CASES,
               "three cases is three handovers' worth of patient, which is the point of the rule");

    let live = ward_payload(&[], &[], &nobody(), None, 1, "devnet:ABC");
    let stay_rule = live["policy"]["stay"].as_str().expect("the policy must publish the stay length");
    assert!(stay_rule.contains('3'), "it says three, in digits a reader can check: {stay_rule}");

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
        case: "ep2-stemi".into(),
        persona: Persona { name: "Ploy".into(), country: "THA".into() },
        portrait: None,
    });

    let v = ward_payload(&patients, &[], &packs, None, 1234, "devnet:ABC");
    let list = v["patients"].as_array().expect("the board needs the patients themselves");
    assert_eq!(list.len(), 2, "every patient the ward ever admitted, closed ones included");

    assert_eq!(list[0]["patient_id"], 7);
    assert_eq!(list[0]["state"], "open", "a word, not a byte — the board renders this");
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
        case: "ep2-stemi".into(),
        persona: Persona { name: "Ploy".into(), country: "THA".into() },
        portrait: None,
    });

    let v = ward_payload(&patients, &[], &packs, None, 1234, "devnet:ABC");
    let list = v["patients"].as_array().expect("patients");
    assert_eq!(list[0]["case"], "ep2-stemi", "the board says which case she is");
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

/// The panel offers levels, so the policy has to say what is actually on the shelf.
#[test]
fn the_policy_publishes_the_levels_and_the_endemic_rule() {
    use vitals_web::ward::{difficulty_of, CATALOGUE};

    let v = ward_payload(&[], &[], &nobody(), None, 1, "devnet:ABC");
    let levels = &v["policy"]["difficulty"];
    for level in ["student", "intern", "resident"] {
        let want = CATALOGUE.iter().filter(|c| difficulty_of(c) == Some(level)).count();
        assert_eq!(levels[level], want,
                   "the policy must count {level} the way the catalogue does, or a player picks a \
                    level the ward cannot fill");
    }
    assert_eq!(levels["student"].as_u64().unwrap()
                   + levels["intern"].as_u64().unwrap()
                   + levels["resident"].as_u64().unwrap(),
               CATALOGUE.len() as u64,
               "every case has a level and no case has two");

    let rule = v["policy"]["endemic"].as_str().expect("the endemic rule is published");
    assert!(rule.contains("never"), "it has to say what origin does not do: {rule}");
    assert_eq!(v["policy"]["countries_with_an_endemic_list"], 0,
               "and today the honest count is zero — none of the converted sixteen belongs to a \
                place, and pairing one with a country anyway is the thing this rule exists to \
                stop");
}
