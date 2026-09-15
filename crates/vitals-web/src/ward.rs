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
