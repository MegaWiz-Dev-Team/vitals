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
use vitals_replay::SLOT_SECONDS;

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

/// How many cases one patient's stay is made of.
///
/// A stay is a chain of cases that already exist, joined mechanically: an acute presentation, then
/// observation, then the ward and home (CWF_PLAN.md ruling 1 — the joins are state handoff, never
/// new clinical writing). Three is the founder's turnover rather than a clinical claim: it means
/// one patient spans at least three shifts, so a stranger arriving at noon meets a patient other
/// strangers have already treated, and three beds do not eat the catalogue in an afternoon.
pub const STAY_CASES: usize = 3;

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
    /// A case id from [`CATALOGUE`]. A pack naming anything else is a patient the ward cannot
    /// serve, which is why the door that accepts packs checks it.
    pub case: String,
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
    /// What the place is called, for a reader of the file rather than for the product.
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

/// The endemic lists, by ISO 3166-1 alpha-3.
///
/// **What this is, and what it is not.** Where a patient is from never selects her disease for the
/// common draw. A country with a list here contributes one draw in [`ENDEMIC_IN`] for patients
/// from it — because dengue is about mosquitoes, thalassemia about carrier frequency and altitude
/// sickness about altitude. None of that is a claim about people, and the difference is why this
/// is reviewed data rather than a rule somebody wrote into a draw function.
///
/// Held to the catalogue by `the_endemic_list_may_only_name_cases_the_ward_can_serve`: a case
/// named here that the ward cannot serve would be a patient the board shows and no shift can open.
///
/// Empty today, and the file says why in its own words.
pub fn endemic() -> std::collections::BTreeMap<String, Vec<String>> {
    #[derive(serde::Deserialize)]
    struct File {
        endemic: std::collections::BTreeMap<String, Vec<String>>,
    }
    serde_json::from_str::<File>(include_str!("../data/endemic.json"))
        .map(|f| f.endemic)
        // Baked into the binary by `include_str!`, so a parse failure is a build somebody shipped
        // broken rather than a file that went missing at runtime — and an empty map is the safe
        // shape: the ward draws uniformly, which is what it does for every country without a list.
        .unwrap_or_default()
}

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
) -> usize {
    patients
        .iter()
        .filter(|p| {
            p.state == OPEN
                && packs.contains_key(&p.patient_id)
                // A patient whose chart cannot be rebuilt holds no bed either: nobody can open
                // her, so a bed kept for her is a bed the ward has taken out of service without
                // saying so. The row says why, and the ticker fills the bed.
                && !unrebuildable.contains_key(&p.patient_id)
        })
        .count()
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
    /// This server's wall clock, for the one field that needs one: *on shift since*, which a
    /// browser renders as a time of day. Everything else on the payload comes off the chain.
    pub now_unix: u64,
    /// Which cluster and which program. "The chain says" means nothing until you know which.
    pub source: &'a str,
    /// Patients whose chain names a shift this ward has no tape for, and the leaf it stopped at.
    ///
    /// They are open on chain and unopenable here, which is a fact about this ward rather than
    /// about them — so it is said on the board, in words, and never written to the chain as an
    /// ending they did not have.
    pub unrebuildable: &'a std::collections::BTreeMap<u64, String>,
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
    let mut open: Vec<&PatientOnChain> = patients
        .iter()
        .filter(|p| {
            p.state == OPEN
                && packs.contains_key(&p.patient_id)
                && !lost.contains_key(&p.patient_id)
        })
        .collect();
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
            serde_json::json!({
                "patient_id": p.patient_id,
                // Words, because the board is what reads this. A renderer switching on 0, 1 and 2
                // would have to know the program's byte layout to draw a ward.
                "state": if stuck.is_some() { "unrebuildable" }
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
                // Derived: the lease ends a known number of slots after it is taken, so the start
                // is the end minus that, carried back to wall time through this read's own slot.
                "on_shift_since": on_shift.then(|| {
                    let took = p.lease_until_slot.saturating_sub(LEASE_SLOTS);
                    let ago = as_of_slot.saturating_sub(took) as f64 * SLOT_SECONDS;
                    utc_iso(r.now_unix.saturating_sub(ago as u64))
                }),
                "bed": bed_of(p.patient_id),
                "shifts": p.shifts,
                "admitted_slot": p.admitted_slot,
                "closed_slot": (p.closed_slot > 0).then_some(p.closed_slot),
                "name": pack.map(|k| k.persona.name.clone()),
                "age": pack.map(|k| k.persona.age),
                "country": pack.map(|k| k.persona.country.clone()),
                "case": pack.map(|k| k.case.clone()),
                // Null rather than a default: a patient filed under a level somebody chose against
                // is worse than a patient with no level yet.
                "difficulty": pack.and_then(|k| difficulty_of(&k.case)),
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
        "in_beds": beds_taken(patients, packs, lost),
        "beds": BEDS,
        "week": w,
        "patients": board,
        "readable": true,
        "policy": policy(),
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
                         id, and are null for a patient no pack describes yet. The difficulty is \
                         that case's own level in the catalogue, the same one the bay publishes, \
                         never a second opinion. `on_shift` is the patient's lease standing at \
                         this read's slot, and `on_shift_since` is that lease's start carried to \
                         wall time; `bed` is a place among the open patients in admission order \
                         **among the patients the ward can describe**; one it cannot has no case \
                         to open and holds no bed, and says so in its own row. `portraits` is the \
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

use std::collections::VecDeque;

/// One patient's stay: the chain of cases she will be taken through.
///
/// A stay is made of cases that already exist, and the joins between them are mechanical — the
/// state one case ends in is the state the next begins from (`vitals_replay::shift`). Nothing here
/// writes medicine, and a longer queue is more of the cases we have rather than new ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Stay {
    pub patient_id: u64,
    pub cases: Vec<String>,
    at: usize,
}

impl Stay {
    pub fn new(patient_id: u64, cases: Vec<String>) -> Stay {
        Stay { patient_id, cases, at: 0 }
    }

    /// The case being played now, or `None` once the chain has run out.
    pub fn current(&self) -> Option<&str> {
        self.cases.get(self.at).map(String::as_str)
    }

    /// Move to the next case in the chain and return it.
    pub fn advance(&mut self) -> Option<&str> {
        self.at += 1;
        self.current()
    }

    /// Nothing left to hand over. The stay ends here whatever the ward does next — a terminal
    /// outcome closes a patient earlier, and that is the program's decision, not this one's.
    pub fn finished(&self) -> bool {
        self.at >= self.cases.len()
    }
}

/// Patients waiting for a bed.
///
/// `admit` is the whole automatic-release rule: it needs no argument but the state of the ward, so
/// the server can call it on a timer and nobody has to be awake for a bed to refill.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    waiting: VecDeque<Vec<String>>,
    next_id: u64,
}

impl Queue {
    /// Build the queue from chains of existing case ids. `first_id` is where patient ids start:
    /// the program seeds a patient PDA on it, so it must never repeat for one operator.
    pub fn from_catalogue(catalogue: Vec<Vec<String>>, first_id: u64) -> Queue {
        Queue { waiting: catalogue.into_iter().collect(), next_id: first_id }
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Release as many patients as there are free beds and patients to fill them.
    pub fn admit(&mut self, open: usize, beds: usize) -> Vec<Stay> {
        (0..to_admit(open, beds, self.waiting.len()))
            .filter_map(|_| {
                let cases = self.waiting.pop_front()?;
                let id = self.next_id;
                self.next_id += 1;
                Some(Stay::new(id, cases))
            })
            .collect()
    }
}


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
fn portrait_state(state: u8) -> &'static str {
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
fn state_word(state: u8) -> &'static str {
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
fn policy() -> serde_json::Value {
    serde_json::json!({
        "beds": BEDS,
        "a_bed_frees_on": ["discharge", "death"],
        "admissions_per_day": "as many as leave — a bed frees on discharge or death and on \
                               nothing else, so the rate is a consequence of how the ward is \
                               played rather than a number we choose. Read it off the census.",
        "draw": "uniformly from the catalogue, skipping any case already on the ward, so no \
                 two beds hold the same case at once",
        "where_they_come_from": "admissions are weighted by each country's people per doctor \
                                 (World Bank/WHO, latest year), so a country with twice the \
                                 shortage sends twice the patients. The weights and their source \
                                 are published by the factory: the ward does not choose countries, \
                                 it admits what the queue holds",
        "stay": format!("a stay is {STAY_CASES} cases, joined mechanically — the state one case \
                         ends in is the state the next begins from — so one patient spans at \
                         least {STAY_CASES} shifts and no case is authored for the ward"),
        "catalogue": CATALOGUE,
        // Counted off the catalogue rather than written down, so a case added without a level
        // cannot quietly shrink a band the panel is still offering.
        "difficulty": serde_json::json!({
            "student": CATALOGUE.iter().filter(|c| difficulty_of(c) == Some("student")).count(),
            "intern": CATALOGUE.iter().filter(|c| difficulty_of(c) == Some("intern")).count(),
            "resident": CATALOGUE.iter().filter(|c| difficulty_of(c) == Some("resident")).count(),
        }),
        "endemic": format!(
            "where a patient is from never selects the disease. A country with an endemic list \
             contributes one draw in {ENDEMIC_IN} for patients from it — dengue is about \
             mosquitoes and thalassemia about carrier frequency, which is epidemiology and not a \
             claim about people. The list is reviewed data and may only name cases the ward can \
             serve"),
        "countries_with_an_endemic_list": endemic().len(),
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
        "policy": policy(),
    })
}
