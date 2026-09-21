//! The ward's arithmetic: who is in a bed, and what the census says.
//!
//! Two jobs, both small on purpose. [`to_admit`] decides how many patients the queue may release
//! into free beds — the ward has to keep running while nobody is watching, and a release that
//! needs a human is a ward that stops on a Friday. [`census`] answers the six numbers the weekly
//! video reads out (CWF_PLAN.md).
//!
//! **Every number here is derived from what the chain says.** The patient accounts carry admitted,
//! went home and died; the anchored leaves carry the shifts and who signed them. Nothing in this
//! module counts anything itself, and *on the ward* is subtraction rather than a tally of its own:
//! a second count is a second source of truth, and when two sources disagree neither can be
//! trusted without a third.

use std::collections::HashSet;
use vitals_program::LEASE_SLOTS;

/// Beds on the ward. Three to start (CWF_PLAN.md's beds ruling); a release is automatic, so this
/// is the only thing standing between the queue and the world.
pub const BEDS: usize = 3;

/// The patient states the program writes. Mirrors `vitals_program::PATIENT_*`, kept as plain
/// numbers because this module reads chain bytes and does no Solana work of its own.
/// The cases a stay may be drawn from, as they exist today: the four story episodes and the twelve
/// OSCE stations. `ep1` is deliberately absent — it is the practice case, the one a newcomer plays
/// to learn the room, and a practice case does not belong to a real patient.
///
/// A longer queue is more of these, never new clinical writing (CWF_PLAN.md's beds ruling).
pub const CATALOGUE: [&str; 16] = [
    // The bay's own ids, not the scenario filenames. One vocabulary: `scenario_path`, the persona
    // files, the age table and the station sets all key on these, and a second spelling here made
    // the bay resolve an episode to EP1's file under another patient's name.
    "ep2", "ep3", "ep4", "ep5",
    "osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3", "osce-c",
    "osce-c2", "osce-c3", "osce-d", "osce-d2", "osce-d3", "osce-d4",
];

// A stay was going to be three cases joined end to end — acute, observation, ward and home — and
// `STAY_CASES`, `Stay` and an in-memory `Queue` were written for it. None of it was ever wired up:
// `Stay::advance` was called nowhere, the program closes a patient on the first discharge its
// engine reaches, and the ticker admits from Firestore. The policy sentence said three anyway,
// which is the worst kind of sentence for a judge to read — a promise the chain contradicts.
//
// Deleted rather than left as a promise in the code. A stay is one case: the patient arrives with
// one pack, and she goes home or dies when that case's engine says so. The three-case stay is a
// program change (AdmitPatient carrying the stay's case hashes, a close only at the last one) and
// it is the founder's to ask for after 26 Sep — CWF_PLAN.md ruling 1 carries that.

pub const OPEN: u8 = 0;
pub const DISCHARGED: u8 = 1;
pub const DIED: u8 = 2;

/// One patient account, as read from the chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatientOnChain {
    pub patient_id: u64,
    pub state: u8,
    pub shifts: u32,
    pub admitted_slot: u64,
    pub closed_slot: u64,
    /// Who is in the room with her, zero when nobody is. An **account id**, not a device key.
    pub lease_holder: [u8; 32],
    /// When their time runs out. With `LEASE_SLOTS` it also gives when they started, which is the
    /// only way the board can say *on shift since* without a server keeping its own note.
    pub lease_until_slot: u64,
}

/// One anchored shift, as read from the chain.
///
/// Serialisable because the signers are read from transaction history and cached — see
/// [`crate::ward_chain::Seen`]. Nothing here is a number the server keeps; the cache holds what
/// the chain said, keyed on the signature it said it in.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShiftOnChain {
    pub patient_id: u64,
    /// What the leaf commits to: the hash of the tape this shift played.
    ///
    /// The chain names her past by this, and the tape is looked up by it — so a tape nobody
    /// anchored is never part of her, and a tape we have lost is a rebuild that stops rather than
    /// a patient nobody can check.
    #[serde(default)]
    pub run_hash: [u8; 32],
    /// The key that signed the leaf. A key, not a person: there is no signup here, so one human
    /// may hold several and a shared machine may be many humans behind one.
    pub signer: [u8; 32],
    pub slot: u64,
}

/// Who the patient is, as opposed to what is wrong with her.
///
/// The chain holds her chart and never her name: a patient account carries the scenario she was
/// admitted with, her head, her shifts and her state, and nothing a person is called. The persona
/// comes from the pack the factory queued (CWF_PLAN.md ruling 10) and is joined to the chain by
/// patient id at read time — which is why every entry that uses it says so, and why a patient the
/// factory has not described yet is published with a null name rather than an invented one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Persona {
    pub name: String,
    /// Inside the band her case is written for. The factory picks it; the case decides the band,
    /// so a twelve-year-old never arrives with a disease authored for a woman of seventy.
    pub age: u16,
    /// `f` or `m`, and it has to match the case's own patient — the dialogue, the examination and
    /// the differential are written for it. Here rather than only in the pool because the door's
    /// job is to check it, and a field the door cannot see is a rule the door cannot keep.
    pub sex: String,
    /// ISO 3166-1 alpha-3 — `THA`, `IDN`, `NGA`. The globe matches on this, and a country written
    /// freely is a country nobody can match: "Thailand", "ไทย" and "TH" are three countries to a
    /// renderer and one to a reader.
    pub country: String,
}

impl Persona {
    /// Is the country a shape the globe can match? Three letters, upper case, and nothing else.
    ///
    /// Checked where a pack arrives rather than where it is displayed: a bad country that reaches
    /// the board is a patient nobody can find on a globe, and by then nothing says why.
    pub fn country_is_alpha3(&self) -> bool {
        self.country.len() == 3 && self.country.bytes().all(|b| b.is_ascii_uppercase())
    }
}

/// What the factory queues for one bed: an existing case, the person it is happening to, and her
/// picture.
///
/// The three parts are separate on purpose. The **case** is content we already have and did not
/// write for the ward (ruling 1); the **persona** is manufactured, and is the only manufactured
/// thing here; the **portrait** is an image, absent until one has been made, and a missing picture
/// is never a reason to withhold a patient.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pack {
    /// The case the factory built her for — an id the ward holds, or empty for "the ward
    /// chooses".
    ///
    /// It was a season id until 16 ก.ย. and is now one of the case factory's: the ward plays what
    /// comes through `/api/ward/case` and the season's sixteen are vitals.academy's. Empty rather
    /// than `Option` because that is what the packs already queued deserialise to, and the
    /// nineteen waiting when this changed were not deleted to make a field prettier.
    #[serde(default)]
    pub case: String,
    /// The level she was built for, when the factory knows it: `student`, `intern` or `resident`.
    ///
    /// The ticker uses it to pick a case when the pack names none. Absent means any — better a
    /// patient at whatever level the ward has a case for than an empty bed, because a bed nobody
    /// can take teaches nothing.
    #[serde(default)]
    pub difficulty: Option<String>,
    pub persona: Persona,
    /// Her pictures, keyed on the state she is in — see [`PORTRAIT_LADDER`] and [`portrait_for`].
    ///
    /// A set rather than one image because the founder wants her picture to change with her state
    /// and stay the same person. Packs arrive with `stable` filled and the rest empty; the factory
    /// makes the others at admission, when it knows a bed actually opened for her, so a patient
    /// who never gets worse never costs a picture of her getting worse.
    #[serde(default)]
    pub portrait: std::collections::BTreeMap<String, String>,
    /// True when this case came from her country's endemic list rather than the common draw.
    ///
    /// Recorded by whoever drew her, never inferred later: a case can be endemic somewhere and
    /// ordinary here, so working it out from the country afterwards would relabel patients who
    /// were drawn uniformly — which is the claim the endemic rule is careful not to make.
    #[serde(default)]
    pub endemic: bool,
}

/// How hard a case is: `student`, `intern` or `resident`.
///
/// `None` for a case this ward does not serve, which is the honest answer and also what
/// `every_case_the_ward_can_admit_has_a_difficulty_and_the_board_publishes_it` uses to refuse a
/// catalogue entry nobody gave a level.
///
/// **One table, not a second opinion.** Every level below is the tier the bay already publishes
/// for that case — the station sets in `main.rs` for the twelve stations, the episode list in
/// `vitals-cli` for the four episodes. A case that was intern in the bay and resident on the ward
/// would be two products disagreeing about the same patient in front of the same learner.
pub fn difficulty_of(case: &str) -> Option<&'static str> {
    Some(match case {
        // the episodes, from vitals-cli's own list
        "ep2" => "intern",
        "ep3" | "ep4" | "ep5" => "resident",
        // the stations, from SETS in main.rs
        "osce-a" | "osce-a2" => "student",
        "osce-b" | "osce-b2" | "osce-b3" | "osce-c2" | "osce-c3" | "osce-d" | "osce-d3" => "intern",
        "osce-c" | "osce-d2" | "osce-d4" => "resident",
        _ => return None,
    })
}

/// A unix instant as ISO 8601 in UTC, with the Z that says so.
///
/// The ward computes in no other zone and stores nobody's. A browser renders this in the reader's
/// own zone with `Intl`, which needs no question asked and nothing kept — and the string itself
/// means exactly one moment to every reader, which a bare local time does not.
pub fn utc_iso(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Days-from-civil, the same arithmetic `usage::day_key` uses; one algorithm for dates, so the
    // funnel's days and the ward's instants can never disagree about which day it is.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// What to do with the board this ward is holding.
///
/// The board is a chain read kept for a few seconds. When it goes stale the question is who pays
/// for the next read — and it must not be the person who happened to ask. A board carries its own
/// `as_of`, so handing over one that is a minute old is a fact a reader can see; making them wait
/// for devnet is a blank panel and the founder asking where the patients went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// Nothing anywhere: read the chain now. The only request that ever waits, on a ward that has
    /// never read the chain at all.
    Wait,
    /// Inside its life. Answer with it and touch nothing.
    Serve,
    /// Past its life. Answer with it anyway, and read the chain behind the answer.
    ServeAndRefresh,
    /// Nothing in this instance's memory, and the last board this ward read is in its store: answer
    /// with that and read the chain behind it.
    ///
    /// A process that has just started has no board of its own, and the visitor who happens to
    /// knock first must not pay for one — 123 s of signature listings on staging, measured, with a
    /// blank panel in front of them. The stored board says when it was read, which is a fact a
    /// reader can see; a wait is not.
    ServeStoredAndRefresh,
}

/// Where the board in an answer came from, how old it is, and who wrote it.
///
/// Published on every `/api/ward`, because the alternative is what we had: a first read that took
/// 112 s and an answer that could not say whether it had been served from the store, rebuilt from
/// the chain, or simply waited on a container that was still starting. A reader gets it too — a
/// board two minutes old is a fact, and leaving its age out would not make it fresher.
/// Where a board came from — a fact about the board, not about the answer carrying it.
///
/// The difference is the whole bug. "Served from memory" describes *this request*, and the same
/// board answers `chain` once and `memory` for ever after, so its bytes change between two reads of
/// one board. Memory is not an origin; it is a cache. A board is read from the chain by this
/// process, or inherited from the one the last process left, and it stays whichever it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// This process read it from the chain.
    Chain,
    /// It was kept in the store by an earlier process, and this one served it rather than making
    /// somebody wait for a read.
    Store,
}

/// Stamped onto a board once, when it enters this process — never assembled per answer.
///
/// **No field on `/api/ward` may change between two reads of the same board.** The payload's ETag
/// is its bytes, and a field that ticks gives every open page a fresh download on every poll, on
/// the one endpoint whose caching exists because a room full of people opens it at once. An age
/// ticked; "served from memory" flipped on the second request. `egress.rs` holds the line by
/// pairing two assertions — a gzip response decodes to exactly the uncompressed bytes, and a second
/// request carrying the tag gets a 304 — and any field added here has to survive both.
pub fn board_note(from: Origin, kept_at: u64, kept_by: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "from": match from { Origin::Chain => "chain", Origin::Store => "store" },
        // When it was read, not how long ago: a reader who wants the age subtracts.
        "kept_at": kept_at,
        "kept_by": kept_by,
    })
}

pub fn board_use(
    age: Option<std::time::Duration>,
    stored: bool,
    ttl: std::time::Duration,
) -> Board {
    match (age, stored) {
        // Memory first, always: it is this instance's own read and never older than the board it
        // started from.
        (Some(a), _) if a < ttl => Board::Serve,
        (Some(_), _) => Board::ServeAndRefresh,
        (None, true) => Board::ServeStoredAndRefresh,
        (None, false) => Board::Wait,
    }
}

/// The beds a stranger can take right now, off a board this host already has.
///
/// For the pages that say no. A refusal is read by somebody who came to treat a patient — they
/// mistyped an id, or followed a link to a receipt nobody anchored — and a sentence plus a way back
/// to the globe makes them start again. These are the same patients the panel offers, chosen by the
/// same rule: a bed of her own, and nobody in the room with her.
///
/// In bed order, because that is the order the ward is arranged in and the order the board shows.
/// An unreadable board offers nothing: on a page about something else, "we could not look" must not
/// be dressed as "there are none".
pub fn beds_to_offer(board: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut open: Vec<serde_json::Value> = board["patients"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|p| p["state"] == "on_ward" && p["bed"].as_u64().is_some())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    open.sort_by_key(|p| p["bed"].as_u64().unwrap_or(u64::MAX));
    open
}

/// Which heads the ward should take back, given when each page last beat.
///
/// The page beats while it holds a head; the ward frees the head when the beats stop. Two missed
/// beats is the rule — one is a page on a train, and the cost of being wrong is taking a bed off
/// somebody standing in the room.
///
/// Pure, and in milliseconds since the epoch rather than an `Instant`, so the decision can be
/// tested at any distance from the thread that acts on it. A beat from the future is treated as a
/// beat now: a host whose clock stepped backwards has not been abandoned by anybody.
pub fn heads_to_free(
    beats: &std::collections::BTreeMap<u64, u64>,
    now_ms: u64,
    grace_ms: u64,
) -> Vec<u64> {
    beats
        .iter()
        .filter(|(_, last)| now_ms.saturating_sub(**last) > grace_ms)
        .map(|(patient, _)| *patient)
        .collect()
}

/// When a chain event happened, as the chain itself dates the slot it landed in.
///
/// `None` for slot zero (the program's "never") and for a slot this ward has no block time for.
/// A missing block time is not an invitation to work one out: the board carried times derived as
/// "this read's slot, minus that slot, times 0.4 seconds, before now" and they were 38 hours early
/// on a two-day-old admission, because a chain does not produce slots at its nominal rate for days
/// on end. A row with no time says the state and stops.
pub fn at_slot(times: &std::collections::BTreeMap<u64, i64>, slot: u64) -> Option<String> {
    if slot == 0 {
        return None;
    }
    times.get(&slot).map(|t| utc_iso((*t).max(0) as u64))
}

/// Every slot a payload of this read would want a time for, so the caller can ask the chain for
/// them in one pass and cache them.
///
/// Admissions, closures, the slot each patient's latest shift was anchored in, and — for a patient
/// somebody is with — the slot their lease was taken in, which is `lease_until_slot` less the
/// lease's own length and is therefore a block that really was produced.
pub fn slots_to_date(patients: &[PatientOnChain], shifts: &[ShiftOnChain]) -> std::collections::BTreeSet<u64> {
    let mut want = std::collections::BTreeSet::new();
    for p in patients {
        want.insert(p.admitted_slot);
        want.insert(p.closed_slot);
        if p.lease_until_slot > 0 {
            want.insert(p.lease_until_slot.saturating_sub(LEASE_SLOTS));
        }
        if let Some(last) = shifts.iter().filter(|s| s.patient_id == p.patient_id).map(|s| s.slot).max() {
            want.insert(last);
        }
    }
    want.remove(&0);
    want
}

/// The case's persona with the ward's patient in it: her name, her age, nothing else.
///
/// The voice is built from the case's persona file, and on the ward the person in the bed is not
/// the person that file names. Without this she introduces herself as the case's patient while the
/// board beside her says somebody else — the product contradicting itself out loud, in the one
/// place a learner is listening.
///
/// **Only the person changes.** The room, what she is presenting with, her cadence, her authored
/// dialogue and her affect are the case's writing; her sex is the case's too, and is what the
/// brief's pronouns are built from — the door has already refused any pack that disagreed with it.
/// The ward renames a patient. It does not write medicine.
pub fn voiced_as(persona: &serde_json::Value, who: &Persona) -> serde_json::Value {
    let mut out = persona.clone();
    if let Some(p) = out.get_mut("patient").and_then(|p| p.as_object_mut()) {
        p.insert("name".into(), serde_json::Value::String(who.name.clone()));
        p.insert("age".into(), serde_json::json!(who.age));
    }
    out
}

/// The patient a case was written about: the one the dialogue, the examination and the
/// differential all assume.
#[derive(Debug, Clone, PartialEq)]
pub struct CasePatient {
    /// `m` or `f`, lower-cased from the persona file.
    pub sex: String,
    pub age: u16,
    /// What the case itself calls her. The ward renames her — a patient from twenty countries is
    /// the whole premise — but the name is here because the voice reads this same file, and the
    /// board and the voice must not disagree out loud about who she is.
    pub name: String,
}

/// The case's own patient, for the twelve stations that carry a persona file.
///
/// `None` for the four episodes, which have none today: their patients live in the series bible
/// and the scenario's prose rather than in a field, so a pack paired with one is taken at its
/// word. That gap is in the test beside this, not hidden — a rule that silently covers three
/// quarters of a catalogue is a rule nobody can rely on.
///
/// Read from `demo/personas/<case>.json`, which is the same file the patient's voice is built
/// from. One source: a second table of ages and sexes would drift, and the drift would be a board
/// that says one thing while the patient says another.
pub fn case_patient(case: &str) -> Option<CasePatient> {
    let raw = match case {
        "osce-a" => include_str!("../../../demo/personas/osce-a.json"),
        "osce-a2" => include_str!("../../../demo/personas/osce-a2.json"),
        "osce-b" => include_str!("../../../demo/personas/osce-b.json"),
        "osce-b2" => include_str!("../../../demo/personas/osce-b2.json"),
        "osce-b3" => include_str!("../../../demo/personas/osce-b3.json"),
        "osce-c" => include_str!("../../../demo/personas/osce-c.json"),
        "osce-c2" => include_str!("../../../demo/personas/osce-c2.json"),
        "osce-c3" => include_str!("../../../demo/personas/osce-c3.json"),
        "osce-d" => include_str!("../../../demo/personas/osce-d.json"),
        "osce-d2" => include_str!("../../../demo/personas/osce-d2.json"),
        "osce-d3" => include_str!("../../../demo/personas/osce-d3.json"),
        "osce-d4" => include_str!("../../../demo/personas/osce-d4.json"),
        _ => return None,
    };
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let p = v.get("patient")?;
    Some(CasePatient {
        sex: p.get("sex")?.as_str()?.to_lowercase(),
        age: p.get("age")?.as_u64()? as u16,
        name: p.get("name")?.as_str()?.to_string(),
    })
}

/// How far a pack's age may sit from the case's own.
///
/// A tenth of the case's age, never less than two years: proportional because the distance that
/// matters is proportional. Five years is nothing at seventy and is a different patient entirely
/// at three — different airway, different doses, different disease.
pub fn age_band(case_age: u16) -> std::ops::RangeInclusive<u16> {
    let slack = (case_age / 10).max(2);
    case_age.saturating_sub(slack).max(1)..=case_age + slack
}

/// The engine's own status words, mildest first.
///
/// Not a vocabulary of ours: these are `vitals_sce::PatientStatus`, the words the scenarios carry
/// and the bay already renders. A portrait keyed on anything else would be a picture claiming a
/// state the chain cannot report, and two vocabularies for one thing is how that happens.
///
/// `dead` sits at the worst end although no picture is ever made for it (producer, 16 ก.ย.), which
/// is what makes [`portrait_for`] resolve a dead patient to her last living state without a rule
/// of its own.
pub const PORTRAIT_LADDER: [&str; 7] = [
    "recovered", "improving", "stable", "deteriorating", "critical", "arrest", "dead",
];

/// Her picture for the state she is in — or the nearest **milder** one that exists.
///
/// Never a worse one, which is the bay's rule and the bay's reason: hanging an arrest over a
/// patient who is talking to you is the frame telling a lie the mark sheet then marks. The rule
/// runs both ways, and the other way is the one worth saying out loud — a patient who went home
/// does not borrow the picture of herself ill in a bed. She has no picture until the one of her
/// leaving is made, and no picture is the honest state.
pub fn portrait_for<'a>(
    set: &'a std::collections::BTreeMap<String, String>,
    state: &str,
) -> Option<&'a str> {
    set.get(portrait_state_for(set, state)?).map(String::as_str)
}

/// The suffix a 256 px sibling is filed under: `stable` is the picture, `stable_256` is the
/// thumbnail of that same picture, and both are content-addressed objects in the same bucket.
pub const SMALL: &str = "_256";

/// Which state's picture is the one to draw — the state itself, or the nearest milder one that
/// exists. The size is a separate question, asked after this one, so a thumbnail can never be a
/// picture of a different moment than the full-size face it stands in for.
pub fn portrait_state_for<'a>(
    set: &std::collections::BTreeMap<String, String>,
    state: &'a str,
) -> Option<&'a str> {
    let at = PORTRAIT_LADDER.iter().position(|s| *s == state)?;
    PORTRAIT_LADDER[..=at].iter().rev().find(|s| set.contains_key(**s)).copied()
}

/// The picture at the bedside: hers, the nearest milder one, or failing both the nearest there is.
///
/// The board's rule is strict and right — a patient who went home does not borrow the picture of
/// herself ill in a bed, so [`portrait_for`] answers nothing when no picture at or below her state
/// exists. At the bedside that nothing is a black frame in front of the person still in the room
/// with her, at the moment she is discharged, which is exactly when it happens. The last face
/// there is beats no face at all.
pub fn portrait_at_the_bedside<'a>(
    set: &'a std::collections::BTreeMap<String, String>,
    state: &str,
) -> Option<&'a str> {
    portrait_for(set, state).or_else(|| {
        PORTRAIT_LADDER.iter().find_map(|s| set.get(*s)).map(String::as_str)
    })
}

/// The board's picture: the small sibling when the factory has made it, the full one when it has
/// not.
///
/// Twenty patients on a globe is twenty full-size faces over a mobile connection for a board
/// nobody has clicked yet. The bedside asks [`portrait_for`] and gets the picture; this is the
/// other half of that sentence, and the fallback is never a broken image — a sibling that does not
/// exist yet simply is not used.
pub fn portrait_small_for<'a>(
    set: &'a std::collections::BTreeMap<String, String>,
    state: &str,
) -> Option<&'a str> {
    let at = portrait_state_for(set, state)?;
    set.get(&format!("{at}{SMALL}")).or_else(|| set.get(at)).map(String::as_str)
}

/// May this session be advanced?
///
/// The page refuses to play a patient before the head is taken, and until 16 ก.ย. the page was the
/// only thing that refused — so a scripted client could play a stranger's patient to the end.
/// The chain still refuses the anchor, but the work is done, the tape exists, and the board says
/// nothing about it.
///
/// The gate is **the declaration**, not a flag of our own: it is the thing the chain stamps, and
/// the server only prepares one for a player the chain says is holding her head. A run that is not
/// a shift is not gated at all — there is no head to take in the season's bay.
pub fn may_step(on_the_ward: bool, declared: bool, handed_over: bool) -> Result<(), String> {
    if !on_the_ward {
        return Ok(());
    }
    // The other end of the same shift. The hand-over reduces the tape and names the leaf that is
    // about to go on chain; a step after it lands on a tape that has already been counted, and
    // that is how a patient came to have two hashes for one shift and be openable by nobody.
    if handed_over {
        return Err("this shift has been handed over — nothing more goes on this tape".into());
    }
    if declared {
        return Ok(());
    }
    Err("take the shift first — the chart is the chain, and nothing you do is on it until the \
         head is yours"
        .into())
}

/// Is this read of the chain behind a shift this server just anchored?
///
/// `Some(sentence)` when it is, and the sentence is what the person waiting should be told. A read
/// taken in the seconds after an anchor comes back before the transaction is finalized: the count
/// is one short, and the page then says *shift 1 · 0 anchored shifts* over a patient who has just
/// been treated. On a page whose whole claim is that the chart is the chain, a stale chart is the
/// worst thing to show — worse than a wait.
///
/// `remembered` is what this server last anchored for this patient and how long ago. **After a
/// minute it stops mattering**: the chain is the authority, whatever we think we wrote, and a
/// server that argued with it for ever would be a server with an opinion of the record.
pub fn behind_the_head(
    chain_shifts: u32,
    remembered: Option<(u32, std::time::Duration)>,
) -> Option<String> {
    let (wrote, ago) = remembered?;
    if ago > std::time::Duration::from_secs(60) || chain_shifts >= wrote {
        return None;
    }
    Some(format!(
        "the last shift on this patient is still landing on chain — {chain_shifts} of {wrote} \
         anchored so far. A few seconds, then open the page again"
    ))
}

/// One person the factory can make a patient of.
///
/// No age: the case carries the band and the factory picks inside it, so an age here would be one
/// that contradicts the case it is paired with. `sex` is not decoration either — a case names its
/// patient's sex, and a pack that disagreed with its own case would put a woman's name on a man's
/// presentation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PoolPerson {
    /// Romanised, and what the ward renders.
    pub name: String,
    /// The name as it is written at home, where the file is confident of the script.
    #[serde(default)]
    pub local: Option<String>,
    /// `f` or `m`.
    pub sex: String,
}

/// One country's three people.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PoolCountry {
    /// ISO 3166-1 alpha-3, and only a code the globe can place.
    pub country: String,
    /// What the place is called — published as `country_name`, and read.
    ///
    /// This comment said "for a reader of the file rather than for the product" and that was
    /// false: the string reaches a learner. It is what `/api/ward`, the shift endpoint and the
    /// chart publish as `country_name`, what the bay prints as "· from South Korea", what the
    /// no-JS pages and the meta description say, and what the factory hands to a portrait prompt.
    ///
    /// So it is held equal to the name the atlas draws for the same code
    /// (`tests/world/globe_logic.mjs`): nobody is from Korea on one screen and South Korea on the
    /// next. A code with no polygon at 110m has no on-screen name to agree with and is free.
    #[serde(default)]
    pub place: String,
    pub personas: Vec<PoolPerson>,
}

/// The people the factory draws from: twenty countries, three each, all invented.
///
/// Shipped in the binary so the ward can check a pack against it, and read from the repository by
/// the factory on the mini, which pairs a person with an existing case and an age from that case's
/// band. `the_persona_pool_can_actually_fill_the_catalogue` holds the rules that make the pool
/// usable: both sexes in every country, no repeated names, and no country the globe cannot place.
///
/// Twenty is a starting spread and not a claim about where medicine happens. Adding a country is
/// adding three people to `data/personas.json`.
pub fn persona_pool() -> Vec<PoolCountry> {
    #[derive(serde::Deserialize)]
    struct File {
        countries: Vec<PoolCountry>,
    }
    serde_json::from_str::<File>(include_str!("../data/personas.json"))
        .map(|f| f.countries)
        .unwrap_or_default()
}

/// One draw in five, for a patient from a country that has an endemic list.
///
/// Not a knob: the number says how strongly place shows in the ward without place ever *deciding*
/// what somebody has. Four draws in five are the ordinary uniform draw, in which origin plays no
/// part at all.
pub const ENDEMIC_IN: u32 = 5;

// `endemic()` read `data/endemic.json` — six country→case pairs for the season's sixteen, baked in
// by `include_str!` — and the queue door checked a pack's endemic claim against it. The ward plays
// what the case factory compiles now, and a compiled case carries its own endemic tag and its own
// country, so the claim is checked against the catalogue in `ward_chain::endemic_claim` and nothing
// in this crate reads that file any more.
//
// **The file stays.** `vitals-factory` reads it off the repo at every tick (`tick.rs`, "the endemic
// list could not be read") and fails the whole tick when it cannot, so deleting it would stop the
// factory admitting anybody. Moving the factory's draw to the catalogue is the other half of this,
// and it is that crate's to make.

/// The six numbers, in the order the card shows them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Census {
    pub admitted: u64,
    pub on_ward: u64,
    pub went_home: u64,
    pub died: u64,
    pub shifts: u64,
    pub keys: u64,
}

/// The census over everything, or over a window.
///
/// `since` is a slot: `None` is the ward's whole life, `Some(slot)` is "this week" — the same
/// arithmetic, so the cumulative figure and the weekly one can never drift apart by being computed
/// two different ways.
///
/// Windowing asks each fact when it happened. A patient counts as admitted in the window if she
/// was released in it; as gone if she closed in it — which means a patient admitted before the
/// window and discharged inside it counts as a discharge and not as an admission, exactly as a
/// ward register would read.
pub fn census(patients: &[PatientOnChain], shifts: &[ShiftOnChain], since: Option<u64>) -> Census {
    let within = |slot: u64| since.is_none_or(|s| slot >= s);

    let admitted = patients.iter().filter(|p| within(p.admitted_slot)).count() as u64;
    let went_home = patients.iter()
        .filter(|p| p.state == DISCHARGED && within(p.closed_slot))
        .count() as u64;
    let died = patients.iter()
        .filter(|p| p.state == DIED && within(p.closed_slot))
        .count() as u64;

    let counted: Vec<&ShiftOnChain> = shifts.iter().filter(|s| within(s.slot)).collect();
    let keys: HashSet<[u8; 32]> = counted.iter().map(|s| s.signer).collect();

    Census {
        admitted,
        // Subtraction, never a tally. Floored, because a chain read halfway through a write can
        // show a close whose admission is outside the window, and a negative census is a bug that
        // reads as a scandal.
        on_ward: admitted.saturating_sub(went_home).saturating_sub(died),
        went_home,
        died,
        shifts: counted.len() as u64,
        keys: keys.len() as u64,
    }
}

/// How many beds are taken.
///
/// **A bed is a patient the ward can describe.** She is in one when she is open on the chain *and*
/// a pack says who she is: a patient the ward cannot name has no case, so nobody can take a shift
/// on her, so nobody can ever discharge her — and counted as a bed she would wedge the ward shut
/// against a full queue. Not hypothetical: it is what three test patients did to staging on
/// 16 ก.ย.
///
/// The census is unaffected and must stay so. `on_ward` counts what is open on the chain, because
/// she is on the chain; this is about beds, which are the ward's own arithmetic.
pub fn beds_taken(
    patients: &[PatientOnChain],
    packs: &std::collections::BTreeMap<u64, Pack>,
    unrebuildable: &std::collections::BTreeMap<u64, String>,
    cases: &[crate::ward_case::CaseSummary],
) -> usize {
    patients
        .iter()
        .filter(|p| can_open(p, packs, unrebuildable, cases))
        .count()
}

/// Can this ward actually put somebody at this bedside?
///
/// Three ways the answer is no, and all three mean the same thing to a bed: nobody can open her, so
/// a bed kept for her is a bed the ward has taken out of service without saying so. The row says
/// why, and the ticker fills the bed.
///
///   * the ward has no pack for her — she was admitted by something other than the queue;
///   * her chart cannot be rebuilt — the chain names a shift whose tape this ward does not have;
///   * her pack names a case the catalogue no longer holds. Two patients sat in beds like this for
///     two days on staging, admitted on the season's stations before the case door existed, with
///     "take a shift" beside each of them and nothing behind it.
///
/// **An empty catalogue judges nobody.** A ward that has not loaded its cases yet has not lost
/// anybody's case, and the alternative is a fresh instance shutting every bed before its first read
/// finishes.
pub fn can_open(
    p: &PatientOnChain,
    packs: &std::collections::BTreeMap<u64, Pack>,
    unrebuildable: &std::collections::BTreeMap<u64, String>,
    cases: &[crate::ward_case::CaseSummary],
) -> bool {
    p.state == OPEN
        && !unrebuildable.contains_key(&p.patient_id)
        && case_is_held(packs.get(&p.patient_id), cases)
}

/// May this key declare on this patient?
///
/// Only the key the chain says is holding her — and **a head nobody holds is held by nobody**, not
/// by the one caller whose bytes happen to be the sentinel's. `lease_holder` is `[0; 32]` when the
/// bed is free, and the base58 key `1111…1` decodes to exactly those bytes, so a straight equality
/// told that caller to go ahead. Forseti VW-API-23a sends that key and had never once tested the
/// rule it is named for: it passed only while somebody else held the bed.
///
/// The zero test comes first everywhere else this is asked — `PatientAccount::lease_free` in the
/// program, `on_shift` below — and now here too.
pub fn may_declare(lease_holder: [u8; 32], who: [u8; 32]) -> bool {
    lease_holder != [0; 32] && lease_holder == who
}

/// Does this ward still hold the case this pack names?
///
/// The half of [`can_open`] that a chart page needs on its own: it is opened one patient at a time
/// and holds an account rather than a board row, and the question it has to answer is the same one.
/// One function for it, because a page and a row disagreeing about one patient is the bug this
/// exists to prevent.
///
/// No pack is no case. An empty catalogue judges nobody — see [`can_open`].
pub fn case_is_held(pack: Option<&Pack>, cases: &[crate::ward_case::CaseSummary]) -> bool {
    match pack {
        None => false,
        Some(k) => cases.is_empty() || cases.iter().any(|c| c.case_id == k.case),
    }
}

/// Which bed each open patient is in — and keeps for her whole stay.
///
/// It used to be her position among the open patients, which meant a patient renumbered herself
/// whenever somebody admitted before her went home: Salma left bed 2 and the man in bed 3 became
/// the man in bed 2, while he was being told "เตียง 3". A bed that moves under a patient is an
/// index, not a bed.
///
/// Still derived, and still from the chain alone — walk the admissions and the closings in slot
/// order, give each arrival the lowest free number and hand it back when she leaves. Two readers
/// of the same chain arrive at the same beds, and this server keeps no note that could disagree
/// with them. A patient the ward cannot describe or cannot rebuild holds no bed, as everywhere
/// else, and takes no number out of the sequence.
pub fn beds_of(
    patients: &[PatientOnChain],
    packs: &std::collections::BTreeMap<u64, Pack>,
    unrebuildable: &std::collections::BTreeMap<u64, String>,
    cases: &[crate::ward_case::CaseSummary],
) -> std::collections::BTreeMap<u64, usize> {
    // (slot, arriving, patient) — a closing at the same slot as an admission frees the bed first,
    // so the arrival can take it. `false < true` orders them that way.
    let mut events: Vec<(u64, bool, &PatientOnChain)> = Vec::new();
    for p in patients {
        // `can_open` asks about an open patient; the walk also needs the ones who have left,
        // because a closing frees the bed it held. So the chart-level reasons are asked directly.
        if !packs.contains_key(&p.patient_id)
            || unrebuildable.contains_key(&p.patient_id)
            || packs.get(&p.patient_id).is_some_and(|k| {
                !cases.is_empty() && !cases.iter().any(|c| c.case_id == k.case)
            })
        {
            continue;
        }
        events.push((p.admitted_slot, true, p));
        if p.state != OPEN && p.closed_slot > 0 {
            events.push((p.closed_slot, false, p));
        }
    }
    events.sort_by_key(|(slot, arriving, p)| (*slot, *arriving, p.patient_id));

    let mut held: std::collections::BTreeMap<u64, usize> = std::collections::BTreeMap::new();
    for (_, arriving, p) in events {
        if !arriving {
            held.remove(&p.patient_id);
            continue;
        }
        let taken: std::collections::BTreeSet<usize> = held.values().copied().collect();
        let free = (1..).find(|n| !taken.contains(n)).unwrap_or(1);
        held.insert(p.patient_id, free);
    }
    held
}

/// How many patients to release right now.
///
/// Free beds, capped by what the queue actually holds. `open` above `beds` — a bed count that
/// shrank under a ward already full — admits nobody rather than going negative.
pub fn to_admit(open: usize, beds: usize, queue: usize) -> usize {
    beds.saturating_sub(open).min(queue)
}

/// The `/api/ward` payload: the six numbers, twice, each beside where it came from.
///
/// The endpoint is the source and the weekly card is a photograph of it (WEEKLY_VIDEO_SYSTEM §9b),
/// so the discipline lives here rather than in whoever builds the card. `as_of_slot` is the slot
/// the chain was read at — a number without its read time is not evidence — and `source` names
/// the cluster and program it was read from, because "the chain says" means nothing until you know
/// which chain.
///
/// `on_ward` publishes its own subtraction. A reader who wants to check it does not have to guess
/// whether we counted open patients separately, and a reader who re-counts them another way and
/// gets a different answer knows immediately that one of the two is wrong.
pub struct WardRead<'a> {
    pub patients: &'a [PatientOnChain],
    pub shifts: &'a [ShiftOnChain],
    /// What the factory queued, by patient id. Empty is a working ward.
    pub packs: &'a std::collections::BTreeMap<u64, Pack>,
    /// The start of *this week*, as a slot. `None` asks for the ward's whole life.
    pub since: Option<u64>,
    /// The slot the chain was read at. A number without its read time is not evidence.
    pub as_of_slot: u64,
    /// This server's wall clock. Nothing a reader sees is measured from it any more — every time
    /// on the board is the chain's own dating of a slot — and it stays for the arithmetic that is
    /// about this server: how stale a remembered read is, and what "today" means to the funnel.
    pub now_unix: u64,
    /// How long a slot is taking on this chain right now, in seconds, when the ward has measured
    /// it — two block times a few thousand slots apart, divided.
    ///
    /// `None` is "not measured here", and the payload then publishes the lease in slots and no
    /// minutes at all. The nominal 0.4 s is not a fallback: it is the number that made the page say
    /// twenty-three minutes about a lease that was running for nine.
    pub seconds_per_slot: Option<f64>,
    /// What the chain says the clock read at each slot this payload names, by slot.
    ///
    /// Filled by `ward_chain::slot_times`, which asks the RPC for `getBlockTime` once per slot and
    /// keeps the answer for ever — a block's time never changes. A slot that is not in here has no
    /// time on the board: see `at_slot`.
    pub times: &'a std::collections::BTreeMap<u64, i64>,
    /// Which cluster and which program. "The chain says" means nothing until you know which.
    pub source: &'a str,
    /// The cases this ward holds, as the catalogue reads them.
    ///
    /// The board says how hard each patient's case is, and that is the case's own word now rather
    /// than a lookup in the season's table of sixteen — which answered `null` for every case the
    /// factory compiles.
    pub cases: &'a [crate::ward_case::CaseSummary],
    /// Patients whose chain names a shift this ward has no tape for, and the leaf it stopped at.
    ///
    /// They are open on chain and unopenable here, which is a fact about this ward rather than
    /// about them — so it is said on the board, in words, and never written to the chain as an
    /// ending they did not have.
    pub unrebuildable: &'a std::collections::BTreeMap<u64, String>,
    /// Patients whose history could not be refreshed this read, and why.
    ///
    /// Their account was read — that is how they are on the list — so the row is right and the bed
    /// is still offered. The chart may be a shift behind, and the row says so before a stranger
    /// presses anything. Never a reason for the board to be unreadable: one rate-limited listing
    /// blacked out staging for an hour on 20 ก.ย., and this is the field that replaced the blackout.
    pub unread: &'a std::collections::BTreeMap<u64, String>,
}

/// The queue, as people rather than a number.
///
/// One row per waiting pack: the pack id it is addressed by, who she is, the level she was built
/// for, whether her case is endemic where she is from, and the face the pack arrived with. The
/// case's own title too, filled with her name and age the way the catalogue fills it — a reader
/// looking at a country wants to know what is waiting there, and "Young woman from Bangladesh,
/// fever for 3 weeks" is that in the case author's words.
///
/// What is deliberately absent: a bed (she is in none), a patient id (she is not on the chain), a
/// state (nothing has happened to her), and anything else from her case. The sce is the case, and
/// the case is what a learner meets at a bedside rather than reads in a payload.
pub fn waiting_rows(
    waiting: &[(String, Pack)],
    cases: &[crate::ward_case::CaseSummary],
) -> Vec<serde_json::Value> {
    waiting
        .iter()
        .map(|(id, pack)| {
            let held = cases.iter().find(|c| c.case_id == pack.case);
            serde_json::json!({
                "pack": id,
                "name": pack.persona.name,
                "age": pack.persona.age,
                "sex": pack.persona.sex,
                "country": pack.persona.country,
                "difficulty": pack.difficulty.clone().or_else(|| held.map(|c| c.difficulty.clone())),
                "endemic": pack.endemic,
                // Filled for **her**, not for the case's own patient. A case is authored about
                // a patient of its own and the ward places whoever the factory drew inside the
                // band it allows; both are patients and only one of them is waiting here. Filled
                // from the case, this sentence disagrees with the age printed beside it.
                "case_title": held.map(|c| {
                    crate::ward_case::fill_persona(&c.title, &pack.persona)
                }),
                "portrait": portrait_for(&pack.portrait, "stable"),
            })
        })
        .collect()
}

pub fn ward_payload(r: &WardRead) -> serde_json::Value {
    let (patients, shifts, packs, since, as_of_slot, source) =
        (r.patients, r.shifts, r.packs, r.since, r.as_of_slot, r.source);
    let lost = r.unrebuildable;
    let all = census(patients, shifts, None);
    let week = census(patients, shifts, since);
    // How many of the open patients this ward cannot rebuild. Not part of `Census`, which is
    // chain-derived arithmetic and nothing else: this is a fact about what *this ward* is holding,
    // and it is published beside the census rather than folded into it.
    let stuck_now = patients
        .iter()
        .filter(|p| p.state == OPEN && lost.contains_key(&p.patient_id))
        .count() as u64;
    let six = |c: &Census| serde_json::json!({
        "admitted": c.admitted,
        "on_ward": c.on_ward,
        "went_home": c.went_home,
        "died": c.died,
        "shifts": c.shifts,
        "keys": c.keys,
    });
    let mut all_json = six(&all);
    all_json["unrebuildable"] = serde_json::json!(stuck_now);
    let mut w = six(&week);
    w["since_slot"] = match since {
        Some(s) => serde_json::json!(s),
        None => serde_json::Value::Null,
    };
    // Said, because a week is a different week in Bangkok and somebody comparing two screenshots
    // has no other way to know which one we counted.
    w["basis"] = serde_json::json!("UTC");
    // Which bed each open patient is in. Not on chain — the program knows patients, not furniture
    // — so it is derived the one way that is stable between two reads: open patients in the order
    // they were admitted. A patient who leaves frees her number for the next admission, which is
    // what a bed is.
    // Only the patients the ward can describe are in beds — see `beds_taken`.
    let mut open: Vec<&PatientOnChain> =
        patients.iter().filter(|p| can_open(p, packs, lost, r.cases)).collect();
    open.sort_by_key(|p| (p.admitted_slot, p.patient_id));
    let bed_of = |id: u64| open.iter().position(|p| p.patient_id == id).map(|i| i + 1);

    let board: Vec<serde_json::Value> = patients
        .iter()
        .map(|p| {
            let pack = packs.get(&p.patient_id);
            let on_shift = p.state == OPEN
                && p.lease_holder != [0; 32]
                && as_of_slot < p.lease_until_slot;
            // On the chain and not on the ward: admitted by something other than the queue, so
            // there is no case to play and no bed to hold. Named rather than hidden — she exists,
            // and a board that quietly dropped her would disagree with its own census.
            let adrift = p.state == OPEN && pack.is_none();
            // Open on chain and unopenable here: the chain names a shift whose tape this ward does
            // not have, so nobody can rebuild the chart and nobody can take the bed. Said in her
            // own word rather than written to the chain as an ending she did not have.
            let stuck = (p.state == OPEN).then(|| lost.get(&p.patient_id)).flatten();
            // Her pack names a case this ward no longer holds. Two patients sat in beds on staging
            // like this for two days — admitted on the season's stations before the case door
            // existed, still in beds after the catalogue moved on — and the board offered a shift
            // on each. What a stranger got for pressing it was "Not on this page yet".
            //
            // She keeps her bed and the census keeps counting her, because she is in one and her
            // chart is on chain. The board simply stops offering a door onto a page nothing can
            // draw.
            //
            // A catalogue this ward has not loaded yet is not a catalogue that lost her case: with
            // nothing in it, nothing is judged missing, or a fresh instance would shut every bed on
            // the ward before its first read finished.
            let caseless = p.state == OPEN
                && !r.cases.is_empty()
                && pack.is_some_and(|k| !r.cases.iter().any(|c| c.case_id == k.case));
            serde_json::json!({
                "patient_id": p.patient_id,
                // Whether a stranger can be offered this bed at all. The page asks it before it
                // draws a link, so the answer lives here rather than being worked out twice.
                "openable": !(stuck.is_some() || adrift || caseless),
                // No pronoun: this code has a patient id, not a persona, and the ward admits men
                // and women. `plain_words.rs` holds every sentence in this file to that.
                "why_not": caseless.then_some(
                    "the ward no longer holds this case, so nothing here can open this bed"),
                // The chart may be a shift behind; the bed is not shut for it. The account that
                // says there is a bed was read fine — it is the history that was not.
                "history": r.unread.contains_key(&p.patient_id)
                    .then_some("history not refreshed this minute"),
                // Words, because the board is what reads this. A renderer switching on 0, 1 and 2
                // would have to know the program's byte layout to draw a ward.
                // Her own word. `off_ward` means this ward knows nothing about her — no pack,
                // admitted outside the queue — and the globe page drops those rows on exactly that
                // understanding. A patient whose case the catalogue lost is the opposite: the ward
                // has her pack, her chart and her receipts. One word for each fact.
                "state": if stuck.is_some() { "unrebuildable" }
                         else if caseless { "caseless" }
                         else if adrift { "off_ward" }
                         else if on_shift { "on_shift" }
                         else { state_word(p.state) },
                "note": match (stuck, adrift) {
                    (Some(leaf), _) => Some(format!(
                        "the chain names a shift whose tape this ward does not have — leaf {leaf}. \
                         The chart cannot be rebuilt, so no shift can be taken and no bed is held. \
                         Still open on chain: nothing was written to say otherwise"
                    )),
                    (None, true) => Some(
                        "admitted outside the ward · no bed — this patient is on the chain and the \
                         ward has no pack, so there is no case to open and no bed held"
                            .to_string(),
                    ),
                    _ => None,
                },
                // When the person in the room with her started, as a time a browser can render.
                // The lease ends a known number of slots after it is taken, so the start is the
                // end minus that — a slot a block was really produced in, which the chain dates.
                "on_shift_since": on_shift
                    .then(|| at_slot(r.times, p.lease_until_slot.saturating_sub(LEASE_SLOTS)))
                    .flatten(),
                // When somebody last finished a shift on her, as a time a browser can render.
                // The chain carries one leaf per anchored shift with the slot it landed in, so the
                // latest of hers is the last hand-over — the max rather than the last in the list,
                // because the accounts come back in whatever order the RPC gives them. Null when
                // nobody has: the globe then says when she was admitted, which is the honest thing
                // to say about a patient nobody has treated yet.
                "handed_over": shifts
                    .iter()
                    .filter(|s| s.patient_id == p.patient_id)
                    .map(|s| s.slot)
                    .max()
                    .and_then(|slot| at_slot(r.times, slot)),
                "bed": bed_of(p.patient_id),
                "shifts": p.shifts,
                "admitted_slot": p.admitted_slot,
                "closed_slot": (p.closed_slot > 0).then_some(p.closed_slot),
                // The slots above are the chain's facts; these two are what a person reads, and
                // they are the chain's own dating of those same slots rather than arithmetic on
                // them. Null until the ward has asked for the block time.
                "admitted_at": at_slot(r.times, p.admitted_slot),
                "closed_at": at_slot(r.times, p.closed_slot),
                "name": pack.map(|k| k.persona.name.clone()),
                "age": pack.map(|k| k.persona.age),
                "country": pack.map(|k| k.persona.country.clone()),
                "case": pack.map(|k| k.case.clone()),
                // Null rather than a default: a patient filed under a level somebody chose against
                // is worse than a patient with no level yet.
                // Her case's own level, and the season's table only for the patients still
                // mid-stay on a season case.
                "difficulty": pack.and_then(|k| {
                    r.cases
                        .iter()
                        .find(|c| c.case_id == k.case)
                        .map(|c| c.difficulty.clone())
                        .or_else(|| difficulty_of(&k.case).map(str::to_string))
                }),
                "endemic": pack.map(|k| k.endemic).unwrap_or(false),
                // What to draw now, and everything there is to draw. The board gets both so it
                // can change her picture the moment it learns her status without asking again —
                // and so a reader can see that the set is a set.
                "portrait": pack.and_then(|k| portrait_small_for(&k.portrait, portrait_state(p.state))
                                                  .map(str::to_string)),
                "portraits": pack.map(|k| k.portrait.clone()).unwrap_or_default(),
            })
        })
        .collect();

    serde_json::json!({
        "as_of_slot": as_of_slot,
        "source": source,
        "census": all_json,
        // Beside the census and never inside it: the census is what the chain says, and this is
        // what the ward can actually hand to a stranger. A rail that published only the first
        // would be telling somebody six when three of those six cannot be treated by anybody.
        "in_beds": beds_taken(patients, packs, lost, r.cases),
        "beds": BEDS,
        "week": w,
        "patients": board,
        // Every patient whose history could not be refreshed this read, by id and reason. An empty
        // list on a good read rather than an absent field: the payload's shape does not change with
        // the weather, and neither does its ETag between two reads of one board.
        "unread": r.unread.iter()
            .map(|(id, why)| format!("patient {id}: {why}"))
            .collect::<Vec<_>>(),
        "readable": true,
        "policy": policy(Some(r.cases), r.seconds_per_slot),
        "derivations": {
            "admitted": "patient accounts on chain, counted by admitted_slot",
            "in_beds": "open patients the ward holds a pack for, counted at this read. The \
                        difference from `on_ward` is patients that reached the chain some other \
                        way: they are on it, and the ward has no case for them, so nobody can take \
                        a shift on them and they hold no bed",
            "on_ward": "admitted - went_home - died, floored at zero — never a separate tally",
            "went_home": "patient accounts whose state is discharged, counted by closed_slot",
            "died": "patient accounts whose state is died, counted by closed_slot",
            "unrebuildable": "open patients whose chain names a shift this ward has no tape for. \
                              Not a chain fact and not an ending: they are open on chain, nobody \
                              here can rebuild the chart, and nothing was written to the chain to \
                              say otherwise. They hold no bed, so the ticker refills it",
            "shifts": "anchored leaves, one per shift",
            "patients": "one entry per patient account on chain — id, state, shifts and slots \
                         read from the account. The case, the name and the country are not on \
                         chain at all: they come from the pack that was queued, joined by patient \
                         id, and are null for a patient no pack describes yet. The difficulty is that case's own level, \
                         read from the pack the case factory sent — and from the season's table \
                         for the patients still mid-stay on a season case, until the last of them \
                         goes home. `on_shift` is the patient's lease standing at \
                         this read's slot, and `on_shift_since` is that lease's start carried to \
                         wall time. `handed_over` is the slot of the latest anchored shift on \
                         this patient, \
                         carried to wall time the same way — when somebody last finished a shift, \
                         and null when nobody has. `bed` is the number this patient has held since \
                         admission: the admissions and the closings are walked in slot order \
                         and each arrival takes the lowest free one, so a bed never moves under \
                         somebody because a different patient went home. A patient the ward \
                         cannot describe or cannot rebuild holds none, and says so in its own row. `portraits` is the \
                         whole set and `portrait` is the one to draw now — the nearest picture no \
                         worse than the state reported, at 256 px when that sibling exists \
                         (`<state>_256` in the set) and full size when it does not. The bedside \
                         asks for the full one; this is a board. Physiological status is not \
                         derived here yet, so a patient on the ward is drawn with the base \
                         picture and only the endings are exact",
            "keys": "distinct signers of AnchorShift transactions on the ward's patient accounts, \
                     read from transaction history and cached; repeatable with \
                     getSignaturesForAddress. Keys, not humans: there is no signup, so one holder \
                     may have several and a shared machine may be many behind one",
        },
    })
}

// The in-memory `Queue` stood here, and it went with `Stay` for the same reason: nothing called
// it. The ward's queue is the store's — the factory writes packs into it through a door and the
// ticker admits from it every minute — and two queues, one of them unreachable, is one queue and
// a decoy. `to_admit` above is the rule both of them used and the only part worth keeping.

/// The patient id in `/ward/<id>`, or `None` if that is not what this path is.
///
/// Strict on purpose. The id is derived into a patient account and then rendered into a page, so
/// the only thing accepted is exactly one segment of decimal digits that fits a `u64`: no trailing
/// segment, no query, no sign, no hex, no space. Every rejected shape above is a path that would
/// otherwise open a different bed than the one somebody clicked on.
pub fn patient_id_in_path(path: &str) -> Option<u64> {
    let rest = path.strip_prefix("/ward/")?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// The case a review path names: `/ward/review/<case_id>`.
///
/// The same shape the case door accepts, checked here for the same reason it is checked there — the
/// id is rendered into a page and read out of a store, so it is the narrow shape both can carry.
/// `None` for `/ward/review` itself, which is the list, and for anything else.
pub fn review_case_in_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/ward/review/")?.trim_end_matches('/');
    let ok = !rest.is_empty()
        && rest.len() <= 120
        && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then(|| rest.to_string())
}

/// Which portrait a patient in this chain state should be drawn with, today.
///
/// **This is the honest half of a thing that is not finished.** Her physiological status —
/// stable, deteriorating, critical — comes from replaying her tapes, which the ward host does not
/// do yet; until it does, a patient who is still on the ward is drawn with her base picture and a
/// patient who went home is drawn with the picture of her leaving. A dead patient resolves to her
/// last living state through the ladder, and the board's own word says died.
///
/// When the replay lands, this is the one place that changes: the board is already given the whole
/// set, so it will not need asking twice.
/// Which picture a patient in this chain state is drawn with. Public because the chart page — one
/// patient's whole stay — asks the same question the board does and must get the same answer.
pub fn portrait_state(state: u8) -> &'static str {
    match state {
        DISCHARGED => "recovered",
        DIED => "dead",
        _ => "stable",
    }
}

/// The program's state byte as the word the board renders.
///
/// `unknown` rather than a panic or a guess: a byte this build does not know means the program
/// moved ahead of the server, and the honest answer to a reader is that we do not know what she
/// is, not a state we picked.
/// The board's word for a chain state, used by the board and by one patient's own page.
pub fn state_word(state: u8) -> &'static str {
    match state {
        // The board's word, not the program's. `on_ward` rather than `open` because a reader of
        // this endpoint is looking at a ward, and because the page that renders it says on the
        // ward, on shift, went home, died — four states a person recognises.
        OPEN => "on_ward",
        DISCHARGED => "went_home",
        DIED => "died",
        _ => "unknown",
    }
}

/// The policy, on its own — true whether or not the chain can be reached.
///
/// `cases` is what the ward is holding at this read, when the read could take it. `None` is "we
/// did not look", which is why the counts are null rather than zero there: an endpoint that
/// answers `0` because it could not read the store looks exactly like a ward with no cases, and
/// that zero would be read as a fact about the ward rather than about the answer.
fn policy(
    cases: Option<&[crate::ward_case::CaseSummary]>,
    seconds_per_slot: Option<f64>,
) -> serde_json::Value {
    let level = |want: &str| {
        cases.map(|c| c.iter().filter(|c| c.difficulty == want).count())
    };
    serde_json::json!({
        "beds": BEDS,
        "a_bed_frees_on": ["discharge", "death"],
        // The lease, in the program's unit and in a person's. The slots never change; how long
        // they take does — devnet was running at 0.166 s a slot on 17 ก.ย., which made this lease
        // nine and a half minutes rather than the twenty-three the nominal rate implies. A ward
        // that has not measured the rate says nothing about minutes.
        "lease": {
            "slots": vitals_program::LEASE_SLOTS,
            "seconds_per_slot_now": seconds_per_slot,
            "minutes_now": seconds_per_slot
                .map(|s| (vitals_program::LEASE_SLOTS as f64 * s / 60.0).round() as u64),
            // The page counts down in seconds and must not multiply two published numbers to get
            // them: a countdown is the one figure a stranger watches, and it is this ward's to say.
            "seconds_now": seconds_per_slot
                .map(|s| (vitals_program::LEASE_SLOTS as f64 * s).round() as u64),
            "measured": "seconds a slot, from the block time of a recent slot and the block time \
                         of one a few thousand slots earlier — both facts the chain publishes and \
                         anybody can ask it for",
        },
        "admissions_per_day": "as many as leave — a bed frees on discharge or death and on \
                               nothing else, so the rate is a consequence of how the ward is \
                               played rather than a number we choose. Read it off the census.",
        "draw": "from the queue the case factory fills, by a ticker on this host every minute. It \
                 takes the difficulty band with fewest patients on the ward, and never a case \
                 another bed already holds, so no two beds hold the same case at once and a \
                 stranger looking for one level is not told the ward is full of another",
        "where_they_come_from": "admissions are weighted by each country's people per doctor \
                                 (World Bank/WHO, latest year), so a country with twice the \
                                 shortage sends twice the patients. The weights and their source \
                                 are published by the factory: the ward does not choose countries, \
                                 it admits what the queue holds",
        "stay": "a stay is one case. The patient arrives with one pack — the scenario, the mark \
                 sheet and the patient's own words — and it ends when that case's engine ends it: \
                 the patient goes home, or dies. The chain closes a patient on the first of those \
                 it is told about, so nothing here can span more than one case without the program \
                 changing first",
        "catalogue": {
            "from": "cases come through the case door from the case factory — embla-cases, \
                     compiled by vitals-casefactory. None of the season’s sixteen is here: \
                     those are vitals.academy’s, and this ward refuses them at the door",
            // Counted at this read rather than written down: a number in prose is a number that
            // goes stale the next time the compiler sends anything.
            // Of the cases a patient can be placed on. A withdrawn case is out of service —
            // the placement rule, the door and the catalogue page all already treat it so — and a
            // `held` that counted it read 138 on staging beside a ward that could use 75.
            "held": cases.map(|c| c.iter().filter(|c| !c.withdrawn).count()),
            "provisional": cases.map(|c| c.iter().filter(|c| !c.withdrawn && c.provisional).count()),
            "reviewed": cases.map(|c| c.iter().filter(|c| !c.withdrawn && !c.provisional).count()),
            // Said rather than hidden: the ward keeps every case it was ever sent, because shifts
            // already played on one still have to replay. `held + withdrawn` is what is in the store.
            "withdrawn": cases.map(|c| c.iter().filter(|c| c.withdrawn).count()),
            // Both facts in one sentence, from the one constant — the bedside and the catalogue
            // page show this same string rather than composing their own.
            "status": crate::ward_chain::catalogue_status(crate::ward_chain::door_here()),
            "placed_by": "a patient is placed on a case written about somebody of the same sex \
                          and near the same age — within twelve years of the case’s own, \
                          and a child only on a child’s case. The cases of that patient’s \
                          own country are tried first, and a case from anywhere is better than a \
                          bed nobody can be put in",
            "read_them_at": "/api/ward/cases",
        },
        // Off the packs themselves, so a level nobody compiled cannot be offered and a band that
        // filled up cannot be advertised as empty.
        "difficulty": serde_json::json!({
            "student": level("student"),
            "intern": level("intern"),
            "resident": level("resident"),
        }),
        "endemic": format!(
            "where a patient is from never selects the disease. Four draws in {ENDEMIC_IN} ignore \
             it completely; the fifth, for a patient from a country one of these cases is endemic \
             in, may take that case — dengue is about mosquitoes and meningococcal disease about \
             the dry season in the belt, which is epidemiology and not a claim about people. A \
             pack may call itself endemic only when the case it names is tagged endemic and that \
             case's country is the patient's; the tag is the compiler's, on the case, and the door \
             checks the pairing"),
        // Counted off the catalogue, which is where the tags are. The old field counted the
        // entries of a static file written for the season's sixteen, which is a number about a
        // table this ward no longer plays from.
        "endemic_cases": cases.map(|c| c.iter().filter(|c| c.endemic).count()),
        "countries_with_an_endemic_case": cases.map(|c| {
            c.iter()
                .filter(|c| c.endemic)
                .filter_map(|c| c.country.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        }),
    })
}

/// What `/api/ward` answers when the chain could not be read.
///
/// The dangerous failure here is not a wrong number, it is a zero: an endpoint that answers `0`
/// because an RPC timed out looks exactly like a ward nobody came to, and that zero would be
/// photographed onto the weekly card and read out loud. So this carries no numbers at all, says
/// what actually went wrong rather than "error", and keeps the policy — the rule is still true
/// when the chain is unreachable.
pub fn ward_unavailable(source: &str, why: &str) -> serde_json::Value {
    serde_json::json!({
        "readable": false,
        "source": source,
        "why": why,
        "census": serde_json::Value::Null,
        "week": serde_json::Value::Null,
        // The rules are still true with no chain; the counts are not taken here, and say so.
        "policy": policy(None, None),
    })
}
