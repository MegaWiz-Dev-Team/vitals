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
/// The commitment a player makes before a run: `hash(case ‖ player ‖ nonce)`.
///
/// One definition, used by every client and every verifier. The program never recomputes this —
/// it cannot, without the nonce — it only stores what was committed and stamps it into the leaf.
/// The check happens at reveal: the player discloses the nonce, anyone recomputes this hash and
/// compares it with the leaf. The nonce is what keeps the case hidden from chain observers
/// between commit and reveal; without it, watching the chain would tell you which station a
/// candidate is about to sit.
///
/// Domain-separated for the same reason the leaf is: a hash that could be confused with some
/// other 32-byte value in this system is a collision waiting to be constructed.
///
/// `mode` (practice = 0, exam = 1) is bound in for the same reason the case is: whether a run
/// was an exam is decided before it is played, and the chain holds the proof of that ordering.
/// Without it, a good practice run could be anchored as an exam after the outcome was known —
/// undetectable by any verifier, because no public byte would say when the label was chosen.
/// A single byte at a fixed position; the domain tag moves v1→v2 so no v1 preimage (which had
/// no mode) can ever collide with a v2 one.
pub fn commitment_hash(case: &[u8; 32], player: &[u8; 32], nonce: &[u8; 32], mode: u8) -> [u8; 32] {
    // Through the crate's own sha256, which is the syscall on chain and the sha2 crate off it —
    // the same split the Merkle tree already uses, so both worlds compute the same bytes.
    crate::merkle::sha256(&[b"vitals.commit.v2\n", case, player, nonce, &[mode]])
}

/// The hash pinned in a leaf so a verifier picks the exact rubric that marked a run, not whatever
/// version HEAD happens to hold. Over the rubric's authored bytes, domain-separated like
/// `commitment_hash` — reword the rubric and old leaves stop validating against it, which is the
/// correct behaviour: a re-marked run is a different claim.
pub fn rubric_hash(rubric: &[u8]) -> [u8; 32] {
    crate::merkle::sha256(&[b"vitals.rubric.v1\n", rubric])
}

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

    // ── vt02 ────────────────────────────────────────────────────────────────
    /// `hash(case ‖ player ‖ nonce)` — *which* commitment this run answers.
    ///
    /// In the leaf rather than left to the program's behaviour, and that is the whole argument:
    /// the program is upgradeable, so "the program required a commitment" is only as strong as
    /// "you can prove which version ran at that slot". A later upgrade that accepted uncommitted
    /// anchors would make old and new records indistinguishable. The commitment account is also
    /// consumed on use — closed accounts leave state, and most RPCs prune history — so the leaf is
    /// the only place this survives.
    pub commitment: [u8; 32],
    /// The slot the commitment was made at — *that it came first*.
    ///
    /// The hash alone binds which commitment; the slot binds the ordering. Either alone is
    /// insufficient, which is why both are here rather than one.
    pub committed_slot: u64,
    /// The scorer's inputs, pinned separately from the case's presentation content.
    ///
    /// A case can be reworded without changing how it is marked, and can be re-marked without a
    /// word changing. Two hashes say which happened.
    pub rubric_hash: [u8; 32],

    /// The part of the score a verifier can recompute by re-running the pinned engine.
    pub det_score: u16,
    /// What `det_score` was out of.
    pub det_max: u16,
    /// The part attested by a judge rather than derived.
    pub judged_score: u16,
    /// What `judged_score` was out of. **Zero means nothing here required a witness** — a true and
    /// useful thing for a deterministic run to state about itself.
    ///
    /// Never sum these into one number, at any layer. "Does the deterministic part alone predict
    /// passing?" is the strongest result this project can produce, because a third party can
    /// recompute it without trusting our model, our version, or us — and summing makes it
    /// permanently unanswerable.
    pub judged_max: u16,
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
    pub fn encode(&self) -> [u8; 217] {
        let mut out = [0u8; 217];
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
        // ── vt02 appends here. Bytes 0..133 above keep the offsets vt01 gave them, because a
        // version tag lets a reader pick a layout and does not excuse moving a field inside one.
        out[133..165].copy_from_slice(&self.commitment);
        out[165..173].copy_from_slice(&self.committed_slot.to_le_bytes());
        out[173..205].copy_from_slice(&self.rubric_hash);
        out[205..207].copy_from_slice(&self.det_score.to_le_bytes());
        out[207..209].copy_from_slice(&self.det_max.to_le_bytes());
        out[209..211].copy_from_slice(&self.judged_score.to_le_bytes());
        out[211..213].copy_from_slice(&self.judged_max.to_le_bytes());
        // Version tag last, so a future encoding can be told apart from this one.
        out[213..217].copy_from_slice(b"vt02");
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
            det_score: self.det_score,
            det_max: self.det_max,
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
            commitment: [0u8; 32],
            committed_slot: 0,
            rubric_hash: [0u8; 32],
            det_score: 0,
            det_max: 0,
            judged_score: 0,
            judged_max: 0,
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

    /// Exam-ness is decided before the run is played, and the commitment is where that ordering
    /// lives — so the mode must change the hash, or relabelling after the outcome would be
    /// invisible to every verifier.
    #[test]
    fn the_commitment_binds_the_mode() {
        let (case, player, nonce) = ([1u8; 32], [2u8; 32], [3u8; 32]);
        let practice = commitment_hash(&case, &player, &nonce, 0);
        let exam = commitment_hash(&case, &player, &nonce, 1);
        assert_ne!(practice, exam);
        // And every other input still binds as it did.
        assert_ne!(practice, commitment_hash(&[9u8; 32], &player, &nonce, 0));
        assert_ne!(practice, commitment_hash(&case, &[9u8; 32], &nonce, 0));
        assert_ne!(practice, commitment_hash(&case, &player, &[9u8; 32], 0));
    }
}
