//! The attempt record — what a replay reduces to, and what the tree actually stores.
//!
//! This type is the seam between the two halves. `vitals-replay` produces one from a run;
//! `merkle` anchors its leaf; `claim_progress` reconstructs it from a proof and feeds it to
//! [`crate::summarize`]. Every field is either a hash or a small integer, so the encoding is
//! fixed-width and the program can rebuild the leaf without allocating.

use crate::{merkle, Attempt, Difficulty};

/// Terminal state of a run. `NoTerminal` is a real result, not a missing one: a tape that ends
/// with the patient still alive and undecided is a run the player walked away from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Outcome {
    NoTerminal = 0,
    WinDischarge = 1,
    WinIcu = 2,
    DeathBiphasic = 3,
    DeathArrest = 4,
}

impl Outcome {
    pub fn from_u8(v: u8) -> Option<Outcome> {
        Some(match v {
            0 => Outcome::NoTerminal,
            1 => Outcome::WinDischarge,
            2 => Outcome::WinIcu,
            3 => Outcome::DeathBiphasic,
            4 => Outcome::DeathArrest,
            _ => return None,
        })
    }

    /// Parse the reference engine's rendering. Unknown ids are rejected rather than defaulted —
    /// a scenario that reaches an outcome this build has never heard of must not score as zero
    /// and carry on.
    pub fn parse(s: &str) -> Option<Outcome> {
        Some(match s {
            "WinDischarge" => Outcome::WinDischarge,
            "WinIcu" => Outcome::WinIcu,
            "DeathBiphasic" => Outcome::DeathBiphasic,
            "DeathArrest" => Outcome::DeathArrest,
            _ => return None,
        })
    }

    /// Points before harm is deducted.
    pub const fn base_score(self) -> u32 {
        match self {
            Outcome::WinDischarge => 100,
            Outcome::WinIcu => 80,
            // Walking away is worth more than killing the patient and less than treating them.
            Outcome::NoTerminal => 40,
            // A biphasic death follows a patient who was stabilised and then discharged too
            // early — more knowledge shown than an arrest, and a worse failure of judgement.
            Outcome::DeathBiphasic => 25,
            Outcome::DeathArrest => 0,
        }
    }
}

/// Points lost per recorded harm event.
pub const HARM_PENALTY: u32 = 15;

pub const MAX_SCORE: u32 = 100;

/// What one anchored run commits to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptRecord {
    /// Binds the run to a player, so an anchored leaf cannot be claimed by somebody else.
    pub player: [u8; 32],
    /// Binds it to an exact scenario definition. Rewrite the scenario and old leaves stop
    /// proving anything about the new one, which is the correct behaviour.
    pub sce_hash: [u8; 32],
    /// Which case, for distinctness. Replaying one scenario cannot buy breadth.
    pub case: [u8; 32],
    pub difficulty: Difficulty,
    pub exam_mode: bool,
    pub outcome: Outcome,
    pub harm_count: u16,
    /// Commits to the tape, the beats and the harm events — the detail the chain does not need
    /// to hold but must be able to check when someone reveals it.
    pub run_hash: [u8; 32],
}

impl AttemptRecord {
    /// Deterministic score. No model, no rubric, no judgement call — outcome and harm only,
    /// both of which survive the float tolerance in the simulation.
    pub fn score(&self) -> u32 {
        self.outcome
            .base_score()
            .saturating_sub(HARM_PENALTY.saturating_mul(self.harm_count as u32))
    }

    pub const fn max_score(&self) -> u32 {
        MAX_SCORE
    }

    /// Canonical fixed-width encoding. Field order is part of the specification: change it and
    /// every leaf ever anchored stops verifying.
    pub fn encode(&self) -> [u8; 137] {
        let mut out = [0u8; 137];
        out[0..32].copy_from_slice(&self.player);
        out[32..64].copy_from_slice(&self.sce_hash);
        out[64..96].copy_from_slice(&self.case);
        out[96..128].copy_from_slice(&self.run_hash);
        out[128] = match self.difficulty {
            Difficulty::Student => 0,
            Difficulty::Intern => 1,
            Difficulty::Resident => 2,
        };
        out[129] = self.exam_mode as u8;
        out[130] = self.outcome as u8;
        out[131..133].copy_from_slice(&self.harm_count.to_le_bytes());
        // Version tag last, so a future encoding can be told apart from this one.
        out[133..137].copy_from_slice(b"vt01");
        out
    }

    pub fn leaf(&self) -> [u8; 32] {
        merkle::hash_leaf(&self.encode())
    }

    pub fn to_attempt(&self) -> Attempt {
        Attempt {
            case: self.case,
            score: self.score(),
            max: self.max_score(),
            difficulty: self.difficulty,
            exam_mode: self.exam_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(outcome: Outcome, harm: u16) -> AttemptRecord {
        AttemptRecord {
            player: [7; 32],
            sce_hash: [9; 32],
            case: [1; 32],
            difficulty: Difficulty::Student,
            exam_mode: false,
            outcome,
            harm_count: harm,
            run_hash: [3; 32],
        }
    }

    #[test]
    fn scoring_is_ordered_the_way_the_clinic_would_order_it() {
        assert_eq!(rec(Outcome::WinDischarge, 0).score(), 100);
        assert_eq!(rec(Outcome::WinIcu, 0).score(), 80);
        assert_eq!(rec(Outcome::NoTerminal, 0).score(), 40);
        assert_eq!(rec(Outcome::DeathBiphasic, 0).score(), 25);
        assert_eq!(rec(Outcome::DeathArrest, 0).score(), 0);
    }

    #[test]
    fn harm_costs_the_same_whether_or_not_the_patient_lived() {
        // The stood-up run: she survived, and it still costs. That difference is the reason the
        // leaf exists at all.
        assert_eq!(rec(Outcome::WinDischarge, 1).score(), 85);
        assert_eq!(rec(Outcome::WinDischarge, 2).score(), 70);
    }

    #[test]
    fn harm_cannot_drive_the_score_below_zero() {
        assert_eq!(rec(Outcome::DeathArrest, 40).score(), 0);
    }

    #[test]
    fn every_field_changes_the_leaf() {
        let base = rec(Outcome::WinDischarge, 0);
        let mut seen = alloc_vec(base.leaf());
        for mutated in [
            AttemptRecord { player: [8; 32], ..base },
            AttemptRecord { sce_hash: [8; 32], ..base },
            AttemptRecord { case: [8; 32], ..base },
            AttemptRecord { run_hash: [8; 32], ..base },
            AttemptRecord { difficulty: Difficulty::Resident, ..base },
            AttemptRecord { exam_mode: true, ..base },
            AttemptRecord { outcome: Outcome::WinIcu, ..base },
            AttemptRecord { harm_count: 1, ..base },
        ] {
            let l = mutated.leaf();
            assert!(!seen.contains(&l), "a changed field left the leaf untouched");
            seen.push(l);
        }
    }

    fn alloc_vec(first: [u8; 32]) -> std::vec::Vec<[u8; 32]> {
        std::vec![first]
    }

    #[test]
    fn unknown_outcomes_are_rejected_not_defaulted() {
        assert_eq!(Outcome::parse("WinDischarge"), Some(Outcome::WinDischarge));
        assert_eq!(Outcome::parse("SomethingNewInV2"), None);
        assert_eq!(Outcome::from_u8(9), None);
    }
}
