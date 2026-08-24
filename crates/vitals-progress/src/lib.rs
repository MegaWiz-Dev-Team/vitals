//! Integer twin of Embla's competency arithmetic.
//!
//! Embla scores learners with `f64` (`embla-cloud/src/competency.rs`). A Solana program cannot
//! carry that arithmetic across machines and stay bit-identical, so this crate restates the same
//! rules over integers: **basis points** for anything that was a percentage, hundredths for the
//! difficulty and mode multipliers.
//!
//! The two implementations must agree at every threshold. That is not a hope — [`tests`] pins the
//! same boundary cases the upstream `dreyfus_stages()` test pins, plus the exact-boundary values
//! that float and integer arithmetic are most likely to disagree on. **A silent disagreement here
//! denies a student a level they earned**, which is the worst failure this project has.
//!
//! `no_std` and allocation-free: it compiles unchanged into the Anchor program, the verifier, and
//! the wasm on the public verify page.

// No unsafe, enforced rather than observed. Nothing in scoring and the Merkle tree needs it, and in a codebase whose
// product is verifiability, "the compiler checked every memory access" should be a property a
// stranger can confirm from one line. (vitals-program cannot carry this: Solana's entrypoint!
// macro expands to the unsafe input deserialisation every program has.)
#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod merkle;
pub mod record;

/// Case difficulty tier. Weights mirror `xp_for`: resident 1.6×, intern 1.2×, otherwise 1.0×.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Student,
    Intern,
    Resident,
}

impl Difficulty {
    /// Weight × 100.
    pub const fn weight_x100(self) -> u32 {
        match self {
            Difficulty::Resident => 160,
            Difficulty::Intern => 120,
            Difficulty::Student => 100,
        }
    }
    /// Resident tier is what `dreyfus` counts as a "hard" case.
    pub const fn is_hard(self) -> bool {
        matches!(self, Difficulty::Resident)
    }
}

/// Dreyfus stage. Ordinal, so a claim can be compared against what the chain computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dreyfus {
    Novice = 0,
    AdvancedBeginner = 1,
    Competent = 2,
    Proficient = 3,
    Expert = 4,
}

impl Dreyfus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Dreyfus::Novice => "Novice",
            Dreyfus::AdvancedBeginner => "Advanced beginner",
            Dreyfus::Competent => "Competent",
            Dreyfus::Proficient => "Proficient",
            Dreyfus::Expert => "Expert",
        }
    }
    /// Thai label, matching `dreyfus_th` upstream.
    pub const fn as_str_th(self) -> &'static str {
        match self {
            Dreyfus::Novice => "มือใหม่",
            Dreyfus::AdvancedBeginner => "เริ่มเรียนรู้",
            Dreyfus::Competent => "ทำได้",
            Dreyfus::Proficient => "ชำนาญ",
            Dreyfus::Expert => "เชี่ยวชาญ",
        }
    }
}

/// One anchored attempt, as the program reconstructs it from a revealed leaf.
///
/// `case` is the case-id hash rather than the id itself: distinctness must be checkable onchain
/// without carrying case names into program memory.
#[derive(Debug, Clone, Copy)]
pub struct Attempt {
    pub case: [u8; 32],
    /// Deterministic points earned. See `docs/RISKS.md` §3 — only `det_score` is anchored as
    /// re-derivable, so only `det_score` may drive an escrow-backed claim.
    pub score: u32,
    /// Points available. Zero max scores as zero, matching upstream `norm`.
    pub max: u32,
    pub difficulty: Difficulty,
    /// `mode == "exam"` upstream: 1.5× XP.
    pub exam_mode: bool,
}

/// What the chain recomputes for one specialty, and compares a claim against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub attempts: u32,
    pub distinct_cases: u32,
    pub hard_cases: u32,
    /// Mean of `norm_bps`, 0..=10_000.
    pub avg_bps: u32,
    /// Population variance of `norm_bps`, in the same squared scale as upstream's `variance`
    /// divided back down — see [`dreyfus`].
    pub variance_bps: u64,
    pub xp: i64,
    pub level: i64,
    pub dreyfus: Dreyfus,
}

/// Score as basis points of the maximum, clamped — upstream `norm`, without the float.
pub const fn norm_bps(score: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let bps = (score as u64 * 10_000) / max as u64;
    if bps > 10_000 {
        10_000
    } else {
        bps as u32
    }
}

/// XP for a single attempt.
///
/// Upstream: `(norm * diff * modew / 8.0).round()`. Here every factor is scaled by 100, so the
/// divisor carries the same scaling: `norm_bps × diff×100 × mode×100 / 8_000_000`, rounded
/// half-up (matching `f64::round` for the non-negative values this can produce).
pub fn xp_for(a: &Attempt) -> i64 {
    let mode_x100: u64 = if a.exam_mode { 150 } else { 100 };
    let num = norm_bps(a.score, a.max) as u64 * a.difficulty.weight_x100() as u64 * mode_x100;
    const DEN: u64 = 8_000_000;
    ((num + DEN / 2) / DEN) as i64
}

/// Cumulative XP → level. Upstream is already integer-only, so this is a transcription.
/// Highest level the curve is defined for.
///
/// The loop below is bounded because this is a public library: reachable inputs are small
/// (a claim carries at most sixteen attempts) but a caller elsewhere can pass anything, and an
/// unbounded loop over `i64::MAX` is a hang off chain and a compute exhaustion on it. The
/// multiply is checked for the same reason — `25 * (lvl+1) * (lvl+2)` overflows i64 well before
/// the loop would otherwise stop.
pub const MAX_LEVEL: i64 = 4096;

pub fn level_for(xp: i64) -> i64 {
    let mut lvl = 1i64;
    while lvl < MAX_LEVEL {
        let Some(need) = (lvl + 1).checked_mul(lvl + 2).and_then(|v| v.checked_mul(25)) else {
            break;
        };
        if need > xp {
            break;
        }
        lvl += 1;
    }
    lvl
}

/// Dreyfus stage from weighted average, breadth, difficulty and consistency.
///
/// Upstream thresholds are `avg` 50/65/75/85 and `variance < 144.0` (population stddev < ~12).
/// In basis points those become 5_000/6_500/7_500/8_500 and `variance_bps < 144 * 10_000`.
pub fn dreyfus(avg_bps: u32, distinct: u32, hard: u32, variance_bps: u64) -> Dreyfus {
    const CONSISTENT_MAX: u64 = 144 * 10_000;
    let consistent = variance_bps < CONSISTENT_MAX;
    if distinct == 0 || avg_bps < 5_000 {
        Dreyfus::Novice
    } else if avg_bps >= 8_500 && distinct >= 8 && hard >= 2 && consistent {
        Dreyfus::Expert
    } else if avg_bps >= 7_500 && distinct >= 5 && hard >= 1 {
        Dreyfus::Proficient
    } else if avg_bps >= 6_500 && distinct >= 3 {
        Dreyfus::Competent
    } else {
        Dreyfus::AdvancedBeginner
    }
}

/// The whole recomputation, over the attempts a claim presents proofs for.
///
/// Allocation-free: distinctness is counted by scanning backwards, which is O(n²) but bounded by
/// the batch size a single `claim_progress` can carry.
pub fn summarize(attempts: &[Attempt]) -> Summary {
    if attempts.is_empty() {
        return Summary {
            attempts: 0,
            distinct_cases: 0,
            hard_cases: 0,
            avg_bps: 0,
            variance_bps: 0,
            xp: 0,
            level: level_for(0),
            dreyfus: Dreyfus::Novice,
        };
    }

    let n = attempts.len() as u64;
    let mut sum_bps: u64 = 0;
    let mut xp: i64 = 0;
    let mut distinct: u32 = 0;
    let mut hard: u32 = 0;

    for (i, a) in attempts.iter().enumerate() {
        sum_bps += norm_bps(a.score, a.max) as u64;
        xp += xp_for(a);
        let seen_before = attempts[..i].iter().any(|p| p.case == a.case);
        if !seen_before {
            distinct += 1;
            if a.difficulty.is_hard() {
                hard += 1;
            }
        }
    }

    let avg_bps = (sum_bps / n) as u32;
    let mut sq: u64 = 0;
    for a in attempts {
        let d = norm_bps(a.score, a.max) as i64 - avg_bps as i64;
        sq += (d * d) as u64;
    }
    let variance_bps = sq / n;

    Summary {
        attempts: attempts.len() as u32,
        distinct_cases: distinct,
        hard_cases: hard,
        avg_bps,
        variance_bps,
        xp,
        level: level_for(xp),
        dreyfus: dreyfus(avg_bps, distinct, hard, variance_bps),
    }
}

/// Verdict for a `claim_progress` instruction: the chain never grants what it cannot recompute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Granted(Dreyfus),
    Rejected { claimed: Dreyfus, computed: Dreyfus },
}

/// Grant a claim only if the recomputed stage is at least the claimed one.
pub fn adjudicate(claimed: Dreyfus, attempts: &[Attempt]) -> Verdict {
    let computed = summarize(attempts).dreyfus;
    if computed >= claimed {
        Verdict::Granted(computed)
    } else {
        Verdict::Rejected { claimed, computed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(tag: u8) -> [u8; 32] {
        let mut c = [0u8; 32];
        c[0] = tag;
        c
    }

    fn attempt(tag: u8, pct: u32, difficulty: Difficulty) -> Attempt {
        Attempt { case: case(tag), score: pct, max: 100, difficulty, exam_mode: false }
    }

    /// Mirrors upstream `dreyfus_stages()` — same inputs, same expected stages.
    #[test]
    fn dreyfus_matches_upstream_stages() {
        assert_eq!(dreyfus(4_000, 5, 2, 100_000), Dreyfus::Novice); // low avg
        assert_eq!(dreyfus(7_000, 1, 0, 100_000), Dreyfus::AdvancedBeginner); // too few cases
        assert_eq!(dreyfus(7_000, 3, 0, 100_000), Dreyfus::Competent);
        assert_eq!(dreyfus(8_000, 5, 1, 100_000), Dreyfus::Proficient);
        assert_eq!(dreyfus(9_000, 8, 2, 100_000), Dreyfus::Expert);
        assert_eq!(dreyfus(9_000, 8, 2, 4_000_000), Dreyfus::Proficient); // inconsistent → capped
    }

    /// Exact boundaries are where a float and an integer implementation drift apart.
    #[test]
    fn dreyfus_exact_boundaries() {
        assert_eq!(dreyfus(4_999, 3, 0, 0), Dreyfus::Novice);
        assert_eq!(dreyfus(5_000, 3, 0, 0), Dreyfus::AdvancedBeginner);
        assert_eq!(dreyfus(6_499, 3, 0, 0), Dreyfus::AdvancedBeginner);
        assert_eq!(dreyfus(6_500, 3, 0, 0), Dreyfus::Competent);
        assert_eq!(dreyfus(7_500, 5, 1, 0), Dreyfus::Proficient);
        assert_eq!(dreyfus(7_499, 5, 1, 0), Dreyfus::Competent);
        assert_eq!(dreyfus(8_500, 8, 2, 1_439_999), Dreyfus::Expert);
        assert_eq!(dreyfus(8_500, 8, 2, 1_440_000), Dreyfus::Proficient); // variance cap, exact
    }

    #[test]
    fn zero_max_scores_zero() {
        assert_eq!(norm_bps(40, 0), 0);
        assert_eq!(norm_bps(200, 100), 10_000); // clamped
    }

    #[test]
    fn xp_matches_upstream_rounding() {
        // norm 100, student, practice → 100 * 1.0 * 1.0 / 8 = 12.5 → round → 13
        assert_eq!(xp_for(&attempt(1, 100, Difficulty::Student)), 13);
        // norm 100, resident, practice → 100 * 1.6 / 8 = 20
        assert_eq!(xp_for(&attempt(1, 100, Difficulty::Resident)), 20);
        // norm 70, intern, exam → 70 * 1.2 * 1.5 / 8 = 15.75 → round → 16
        let mut a = attempt(1, 70, Difficulty::Intern);
        a.exam_mode = true;
        assert_eq!(xp_for(&a), 16);
    }

    #[test]
    fn level_curve() {
        assert_eq!(level_for(0), 1);
        assert_eq!(level_for(149), 1);
        assert_eq!(level_for(150), 2); // 25 * 2 * 3
        assert_eq!(level_for(300), 3); // 25 * 3 * 4
    }

    #[test]
    fn replaying_one_case_does_not_buy_breadth() {
        let grind = [attempt(1, 90, Difficulty::Student); 20];
        let s = summarize(&grind);
        assert_eq!(s.distinct_cases, 1);
        assert_eq!(s.dreyfus, Dreyfus::AdvancedBeginner); // 20 attempts, still no breadth
    }

    /// The demo: three cardiology cases, three tiers. Three distinct clears `distinct >= 3`
    /// (Competent) and cannot clear `distinct >= 5` (Proficient) — so the rejection the demo
    /// shows is the real threshold refusing, not a staged failure.
    #[test]
    fn three_case_demo_tops_out_at_competent() {
        let run = [
            attempt(1, 72, Difficulty::Student),  // stable angina
            attempt(2, 78, Difficulty::Intern),   // anterior STEMI
            attempt(3, 70, Difficulty::Resident), // aortic dissection
        ];
        let s = summarize(&run);
        assert_eq!(s.distinct_cases, 3);
        assert_eq!(s.hard_cases, 1);
        assert_eq!(s.dreyfus, Dreyfus::Competent);

        assert_eq!(
            adjudicate(Dreyfus::Proficient, &run),
            Verdict::Rejected { claimed: Dreyfus::Proficient, computed: Dreyfus::Competent }
        );
        assert_eq!(adjudicate(Dreyfus::Competent, &run), Verdict::Granted(Dreyfus::Competent));
    }
}

#[cfg(test)]
mod bounds {
    use super::*;

    #[test]
    fn level_for_terminates_and_never_overflows() {
        // The loop used to run until 25*(l+1)*(l+2) exceeded xp, which for a large xp is a
        // billion iterations and an overflow on the way. Reachable inputs are tiny; a public
        // library still has to survive the unreachable ones.
        assert_eq!(level_for(i64::MAX), MAX_LEVEL);
        assert_eq!(level_for(i64::MIN), 1);
        assert_eq!(level_for(-1), 1);
        // and the curve is unchanged where it matters
        assert_eq!(level_for(0), 1);
        assert_eq!(level_for(149), 1);
        assert_eq!(level_for(150), 2);
        assert_eq!(level_for(300), 3);
    }

    #[test]
    fn summarize_survives_saturated_scores() {
        let a = [Attempt { case: [1; 32], score: u32::MAX, max: 1, difficulty: Difficulty::Resident, exam_mode: true }; 8];
        let s = summarize(&a);
        assert_eq!(s.avg_bps, 10_000, "norm is clamped, so the average cannot exceed full marks");
        assert!(s.xp > 0 && s.level >= 1);
    }
}
