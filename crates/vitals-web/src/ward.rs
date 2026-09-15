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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShiftOnChain {
    pub patient_id: u64,
    /// The key that signed the leaf. A key, not a person: there is no signup here, so one human
    /// may hold several and a shared machine may be many humans behind one.
    pub signer: [u8; 32],
    pub slot: u64,
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
    serde_json::json!({
        "as_of_slot": as_of_slot,
        "source": source,
        "cumulative": six(&all),
        "week": w,
        "derivations": {
            "admitted": "patient accounts on chain, counted by admitted_slot",
            "on_ward": "admitted - went_home - died, floored at zero — never a separate tally",
            "went_home": "patient accounts whose state is discharged, counted by closed_slot",
            "died": "patient accounts whose state is died, counted by closed_slot",
            "shifts": "anchored leaves, one per shift",
            "keys": "distinct signers of those leaves — keys, not humans: there is no signup, so \
                     one holder may have several and a shared machine may be many behind one",
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
