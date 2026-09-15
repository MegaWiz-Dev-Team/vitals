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
    "ep2-stemi", "ep3-epiglottitis", "ep4-pulmonary-embolism", "ep5-the-night-the-stars-fell",
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
}

/// One anchored shift, as read from the chain.
///
/// Serialisable because the signers are read from transaction history and cached — see
/// [`crate::ward_chain::Seen`]. Nothing here is a number the server keeps; the cache holds what
/// the chain said, keyed on the signature it said it in.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShiftOnChain {
    pub patient_id: u64,
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
    /// Where her portrait lives, when one has been made.
    #[serde(default)]
    pub portrait: Option<String>,
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
        "ep2-stemi" => "intern",
        "ep3-epiglottitis" | "ep4-pulmonary-embolism" | "ep5-the-night-the-stars-fell" => "resident",
        // the stations, from SETS in main.rs
        "osce-a" | "osce-a2" => "student",
        "osce-b" | "osce-b2" | "osce-b3" | "osce-c2" | "osce-c3" | "osce-d" | "osce-d3" => "intern",
        "osce-c" | "osce-d2" | "osce-d4" => "resident",
        _ => return None,
    })
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
pub fn ward_payload(
    patients: &[PatientOnChain],
    shifts: &[ShiftOnChain],
    packs: &std::collections::BTreeMap<u64, Pack>,
    since: Option<u64>,
    as_of_slot: u64,
    source: &str,
) -> serde_json::Value {
    let all = census(patients, shifts, None);
    let week = census(patients, shifts, since);
    let six = |c: &Census| serde_json::json!({
        "admitted": c.admitted,
        "on_ward": c.on_ward,
        "went_home": c.went_home,
        "died": c.died,
        "shifts": c.shifts,
        "keys": c.keys,
    });
    let mut w = six(&week);
    w["since_slot"] = match since {
        Some(s) => serde_json::json!(s),
        None => serde_json::Value::Null,
    };
    let board: Vec<serde_json::Value> = patients
        .iter()
        .map(|p| {
            let pack = packs.get(&p.patient_id);
            serde_json::json!({
                "patient_id": p.patient_id,
                // A word, because the board is what reads this. A renderer switching on 0, 1 and 2
                // would have to know the program's byte layout to draw a ward.
                "state": state_word(p.state),
                "shifts": p.shifts,
                "admitted_slot": p.admitted_slot,
                "closed_slot": (p.closed_slot > 0).then_some(p.closed_slot),
                "name": pack.map(|k| k.persona.name.clone()),
                "country": pack.map(|k| k.persona.country.clone()),
                "case": pack.map(|k| k.case.clone()),
                // Null rather than a default: a patient filed under a level somebody chose against
                // is worse than a patient with no level yet.
                "difficulty": pack.and_then(|k| difficulty_of(&k.case)),
            })
        })
        .collect();

    serde_json::json!({
        "as_of_slot": as_of_slot,
        "source": source,
        "cumulative": six(&all),
        "week": w,
        "patients": board,
        "readable": true,
        "policy": policy(),
        "derivations": {
            "admitted": "patient accounts on chain, counted by admitted_slot",
            "on_ward": "admitted - went_home - died, floored at zero — never a separate tally",
            "went_home": "patient accounts whose state is discharged, counted by closed_slot",
            "died": "patient accounts whose state is died, counted by closed_slot",
            "shifts": "anchored leaves, one per shift",
            "patients": "one entry per patient account on chain — id, state, shifts and slots \
                         read from the account. Her case, her name and her country are not on \
                         chain at all: they come from the pack that was queued for her, joined by \
                         patient id, and are null for a patient no pack describes yet. The \
                         difficulty is that case's own level in the catalogue, the same one the \
                         bay publishes, never a second opinion",
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


/// The program's state byte as the word the board renders.
///
/// `unknown` rather than a panic or a guess: a byte this build does not know means the program
/// moved ahead of the server, and the honest answer to a reader is that we do not know what she
/// is, not a state we picked.
fn state_word(state: u8) -> &'static str {
    match state {
        OPEN => "open",
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
        "stay": format!("a stay is {STAY_CASES} cases, joined mechanically — the state one case \
                         ends in is the state the next begins from — so one patient spans at \
                         least {STAY_CASES} shifts and no case is authored for the ward"),
        "catalogue": CATALOGUE,
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
        "cumulative": serde_json::Value::Null,
        "week": serde_json::Value::Null,
        "policy": policy(),
    })
}
