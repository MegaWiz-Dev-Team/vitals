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
//! `no_std` and allocation-free: it compiles unchanged into the Solana program (a native
//! `solana-program` entrypoint — there is no Anchor in this workspace), the verifier, and the wasm
//! on the public verify page.

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
    /// The outcome-based score (did the patient do well), out of `max`. Drives `summarize`/level —
    /// the story-mode grade. Not what a star is measured on.
    pub score: u32,
    /// Points available for `score`. Zero max scores as zero, matching upstream `norm`.
    pub max: u32,
    /// The deterministic rubric score — the re-derivable 40. `docs/RISKS.md` §3: only this is
    /// anchored as re-derivable, so only this may earn a star — or, if sponsor money is ever put
    /// behind a badge, drive that claim too. Nothing on chain holds money against delivery today. Zero
    /// for a run that was not marked against a rubric (i.e. any non-exam run).
    pub det_score: u16,
    /// Points available for `det_score` (the rubric's total).
    pub det_max: u16,
    pub difficulty: Difficulty,
    /// `mode == "exam"` upstream: 1.5× XP.
    pub exam_mode: bool,
}

/// The canonical bar a deterministic rubric score must clear to earn a star: 70%, a standard OSCE
/// pass mark. One number for the whole system — every rubric file's `pass_bps` is enforced equal to
/// it (see the vitals-osce test) and the server's `VITALS_STAR_PASS_BPS` defaults to it, so a star
/// a verifier re-derives from the pinned rubric and a star the tally counts can never disagree.
/// Provisional, tunable alongside the rubrics pending clinical review.
pub const STAR_PASS_BPS: u32 = 7000;

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

/// Stars earned: distinct cases cleared **in exam mode** at or above `pass_bps`.
///
/// The star is the exam gate the story rides on, and three properties make it mean something:
/// practice runs (`exam_mode == false`) never earn one, so a learner cannot farm stars outside the
/// exam; a case cleared more than once is still a single star; and only a `det_score`-backed
/// attempt should ever reach here, because a star that is not re-derivable is not one a stranger
/// can check (docs/RISKS.md §3).
///
/// Additive on purpose: `summarize`/`dreyfus` are untouched, so the level the live demo computes is
/// unchanged. Allocation-free, counting distinct the same O(n²) way `summarize` does over the same
/// bounded attempt buffer, so it compiles into the on-chain program unchanged.
pub fn stars(attempts: &[Attempt], pass_bps: u32) -> u32 {
    // A star is measured on the deterministic rubric score, never the outcome — process gates
    // outcome. `det_score`/`det_max` are the re-derivable 40; a lucky good outcome earns nothing.
    let cleared = |a: &Attempt| a.exam_mode && norm_bps(a.det_score as u32, a.det_max as u32) >= pass_bps;
    let mut count: u32 = 0;
    for (i, a) in attempts.iter().enumerate() {
        if !cleared(a) {
            continue;
        }
        // Only the first cleared attempt of a case scores its star, mirroring `summarize`'s
        // distinctness so replaying a station cannot buy a second star.
        if !attempts[..i].iter().any(|p| p.case == a.case && cleared(p)) {
            count += 1;
        }
    }
    count
}

/// The excellence bar: 85% on the deterministic rubric, the second star of the three-tier star
/// (Station Sets v2, DECISIONS.md 27 ส.ค.). Same scale and same discipline as [`STAR_PASS_BPS`]:
/// one number for the whole system, measured only on the re-derivable det score. 8_500 bps also
/// happens to be the Dreyfus expert bar — excellence here means the same thing it means there.
pub const STAR_EXCELLENT_BPS: u32 = 8500;

/// The flawless bar: 95% on the deterministic rubric, the third star (DECISIONS.md 27 ส.ค.,
/// "ดาว 3 ขั้น" — supersedes the two-tier version above it). It is deliberately not 100%: a
/// rubric is a list of things a clinician should have done, and demanding every single point
/// makes the top star a lottery on the one item the tape happened not to catch. 95% says
/// "nothing that mattered was missed" and stays reachable in a real run.
pub const STAR_FLAWLESS_BPS: u32 = 9500;

/// The most stars one case can be worth. The set ceilings, the door prices and the season ring
/// are all `members × this` — one number rather than a 3 written in four places.
pub const STAR_TIERS: u32 = 3;

/// The three bars a case's star is measured against, in basis points.
///
/// A struct rather than three `u32` arguments because they are the same type and the order is
/// invisible at the call site: `(7000, 8500, 9500)` transposed is a silently wrong star, which is
/// the failure this crate's module docs call the worst one this project has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarBars {
    pub pass: u32,
    pub excellent: u32,
    pub flawless: u32,
}

impl StarBars {
    /// The published bars — the ones a verifier re-derives from the pinned rubric.
    pub const CANON: StarBars = StarBars {
        pass: STAR_PASS_BPS,
        excellent: STAR_EXCELLENT_BPS,
        flawless: STAR_FLAWLESS_BPS,
    };

    /// The canonical bars with the pass mark moved — the server's one supported override
    /// (`VITALS_STAR_PASS_BPS`). The upper two stay where they are published.
    pub const fn with_pass(pass: u32) -> StarBars {
        StarBars { pass, ..StarBars::CANON }
    }

    /// Which star a deterministic score in basis points earns: 3 / 2 / 1 / 0.
    pub const fn tier(self, bps: u32) -> u32 {
        if bps >= self.flawless {
            3
        } else if bps >= self.excellent {
            2
        } else if bps >= self.pass {
            1
        } else {
            0
        }
    }

    /// The bar the next star sits on, or `None` at the top. What a debrief needs to say "you
    /// are N points short" without the page keeping its own copy of the ladder.
    pub const fn next_bar(self, bps: u32) -> Option<u32> {
        match self.tier(bps) {
            0 => Some(self.pass),
            1 => Some(self.excellent),
            2 => Some(self.flawless),
            _ => None,
        }
    }
}

/// The three-tier star for one case: 3 at or above `bars.flawless`, 2 at or above
/// `bars.excellent`, 1 at or above `bars.pass`, else 0 — measured on the **best** deterministic
/// score among this player's exam-mode attempts of that case, because a set gate asks "has this
/// case ever been done this well", not "was the last run good". Practice runs never reach the
/// comparison (same rule as [`stars`]), and a case never attempted scores 0 through the same
/// arithmetic: the best of nothing is 0 bps.
///
/// Server-side only — the chain's `stars` tally is untouched, so nothing about the deployed
/// program changes and no rubric moves: the upper stars are a reading of a score that was
/// already anchored, not a new thing to score. Allocation-free and `no_std` anyway, so the day
/// the program wants it, it compiles in unchanged.
pub fn star_tier(attempts: &[Attempt], case: &[u8; 32], bars: StarBars) -> u32 {
    bars.tier(best_det_bps(attempts, case))
}

/// The best deterministic score this player has ever posted on `case` in exam mode, in basis
/// points. 0 when they have never sat it — the best of nothing.
pub fn best_det_bps(attempts: &[Attempt], case: &[u8; 32]) -> u32 {
    let mut best: u32 = 0;
    for a in attempts {
        if a.exam_mode && a.case == *case {
            let bps = norm_bps(a.det_score as u32, a.det_max as u32);
            if bps > best {
                best = bps;
            }
        }
    }
    best
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
        Attempt { case: case(tag), score: pct, max: 100, det_score: 0, det_max: 0, difficulty, exam_mode: false }
    }

    /// An exam run scoring `det_pct`% on its rubric — the only kind that can earn a star. A star is
    /// measured on the rubric (det), never the outcome, so the outcome score here is deliberately 0.
    fn exam(tag: u8, det_pct: u16) -> Attempt {
        Attempt {
            case: case(tag),
            score: 0,
            max: 100,
            det_score: det_pct,
            det_max: 100,
            difficulty: Difficulty::Student,
            exam_mode: true,
        }
    }

    #[test]
    fn a_star_is_the_det_score_not_the_outcome() {
        // A perfect outcome (100/100) with poor process (det 50/100) earns no star — process gates
        // outcome. This is the whole point of measuring the star on det.
        let a = Attempt {
            case: case(1),
            score: 100,
            max: 100,
            det_score: 50,
            det_max: 100,
            difficulty: Difficulty::Student,
            exam_mode: true,
        };
        assert_eq!(stars(&[a], 7000), 0);
    }

    #[test]
    fn a_cleared_exam_case_is_one_star() {
        assert_eq!(stars(&[exam(1, 80)], 7000), 1);
        assert_eq!(stars(&[exam(1, 65)], 7000), 0); // below the 70% bar
    }

    #[test]
    fn practice_never_earns_a_star() {
        // A perfect practice run (exam_mode = false) earns nothing — stars cannot be farmed.
        assert_eq!(stars(&[attempt(1, 100, Difficulty::Student)], 7000), 0);
    }

    #[test]
    fn a_case_cleared_twice_is_still_one_star() {
        assert_eq!(stars(&[exam(1, 80), exam(1, 95)], 7000), 1);
    }

    #[test]
    fn distinct_cleared_exam_cases_each_score() {
        // Two distinct cleared, one below the bar, one practice: 2 stars.
        let runs = [
            exam(1, 90),
            exam(2, 75),
            exam(3, 40),
            attempt(4, 100, Difficulty::Student),
        ];
        assert_eq!(stars(&runs, 7000), 2);
    }

    #[test]
    fn a_failed_then_passed_exam_case_scores_once() {
        assert_eq!(stars(&[exam(1, 50), exam(1, 88)], 7000), 1);
    }

    /// The three-tier star sits exactly on its published thresholds — 9499 is not flawless,
    /// 8499 is not excellent and 6999 is not a pass, because a bar a student can argue with is
    /// not a bar.
    #[test]
    fn star_tier_exact_boundaries() {
        let c = case(1);
        let t = |det: u16| star_tier(&[exam(1, det)], &c, StarBars::CANON);
        // det here is a percentage (det_max 100), so 84.99% is not expressible — pin the bps
        // boundary on a 10_000-max attempt instead, where one point is one basis point.
        let fine = |det: u16| {
            let a = Attempt { det_max: 10_000, ..exam(1, 0) };
            let a = Attempt { det_score: det, ..a };
            star_tier(&[a], &c, StarBars::CANON)
        };
        assert_eq!(fine(9_499), 2); // one bps under flawless: still excellent
        assert_eq!(fine(9_500), 3); // exactly flawless
        assert_eq!(fine(8_499), 1); // one bps under excellent: still just a pass
        assert_eq!(fine(8_500), 2); // exactly excellent
        assert_eq!(fine(6_999), 0); // one bps under the pass bar: nothing
        assert_eq!(fine(7_000), 1); // exactly the pass bar
        assert_eq!(fine(10_000), 3); // full marks tops out at three, never four
        // and the coarse percentage path agrees where it can express the same points
        assert_eq!(t(94), 2);
        assert_eq!(t(95), 3);
        assert_eq!(t(84), 1);
        assert_eq!(t(85), 2);
        assert_eq!(t(69), 0);
        assert_eq!(t(70), 1);
    }

    /// The same boundaries read straight off the bars, without an attempt in the way — the
    /// arithmetic the page mirrors when it tells a player how far the next star is.
    #[test]
    fn star_bars_tier_and_next_bar_agree_on_the_ladder() {
        let b = StarBars::CANON;
        assert_eq!((b.pass, b.excellent, b.flawless), (7_000, 8_500, 9_500));
        for (bps, want) in [(0, 0), (6_999, 0), (7_000, 1), (8_499, 1), (8_500, 2), (9_499, 2), (9_500, 3), (10_000, 3)] {
            assert_eq!(b.tier(bps), want, "{bps} bps must be {want} star(s)");
        }
        assert_eq!(b.next_bar(0), Some(7_000));
        assert_eq!(b.next_bar(7_000), Some(8_500));
        assert_eq!(b.next_bar(8_500), Some(9_500));
        assert_eq!(b.next_bar(9_500), None, "nothing is above the third star");
        // The server's one supported override moves the pass mark and nothing else.
        assert_eq!(StarBars::with_pass(6_000).tier(6_000), 1);
        assert_eq!(StarBars::with_pass(6_000).flawless, STAR_FLAWLESS_BPS);
    }

    #[test]
    fn star_tier_takes_the_best_attempt_not_the_last() {
        // Failed, then flawless, then merely passed: the best run is what the tier reads.
        let runs = [exam(1, 40), exam(1, 97), exam(1, 71)];
        assert_eq!(star_tier(&runs, &case(1), StarBars::CANON), 3);
        assert_eq!(best_det_bps(&runs, &case(1)), 9_700);
    }

    #[test]
    fn star_tier_ignores_practice_and_other_cases() {
        // A perfect practice run and a stranger case's flawless exam: neither is this case's tier.
        let mut practice = attempt(1, 0, Difficulty::Student);
        practice.det_score = 100; practice.det_max = 100; // even with a det on the tape
        let runs = [practice, exam(2, 96)];
        assert_eq!(star_tier(&runs, &case(1), StarBars::CANON), 0);
        assert_eq!(best_det_bps(&runs, &case(1)), 0);
        // and a case never attempted at all is 0, not an error
        assert_eq!(star_tier(&[], &case(1), StarBars::CANON), 0);
    }

    /// The star a door counts and the star the chain tallies must never disagree about the
    /// bottom rung: anything worth a tier is worth the legacy count, and nothing else is.
    #[test]
    fn the_pass_bar_means_the_same_thing_to_both_tallies() {
        for det in [0u16, 69, 70, 84, 85, 94, 95, 100] {
            let runs = [exam(1, det)];
            let tiered = star_tier(&runs, &case(1), StarBars::CANON) > 0;
            assert_eq!(tiered, stars(&runs, STAR_PASS_BPS) == 1, "{det}% disagrees across tallies");
        }
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
        let a = [Attempt { case: [1; 32], score: u32::MAX, max: 1, det_score: 0, det_max: 0, difficulty: Difficulty::Resident, exam_mode: true }; 8];
        let s = summarize(&a);
        assert_eq!(s.avg_bps, 10_000, "norm is clamped, so the average cannot exceed full marks");
        assert!(s.xp > 0 && s.level >= 1);
    }
}
