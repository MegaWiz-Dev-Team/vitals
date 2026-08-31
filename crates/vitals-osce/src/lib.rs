//! Deterministic OSCE rubric scorer — the re-derivable 40.
//!
//! A rubric is a list of checks against a *replayed* run's event log (`SceState::events()`), which
//! `vitals-replay::resume` reproduces byte-for-byte from the tape. Nothing here reads a language
//! model or a keyword matcher over free text the player typed — it reads what the deterministic
//! automaton *did*: the actions it recorded, the harms it fired, the terminal outcome it reached.
//! That is the whole reason this score, and only this score, may drive an on-chain claim
//! (`docs/RISKS.md` §3): a stranger re-runs the engine and gets the same number.
//!
//! The rubric itself is **data**, authored per case (JSON), so the clinical content — which action
//! is critical, what the time window is, which harm must be avoided — lives outside this engine and
//! is reviewed by clinicians, not compiled in. `status` lets a provisional rubric say so.
#![forbid(unsafe_code)]

use serde::Deserialize;
use vitals_replay::{resume, Step};
use vitals_sce::runtime::{Event, Outcome};
use vitals_sce::text::canon;

/// One deterministic check against the event log. Internally tagged by `type` so a rubric reads
/// as flat JSON: `{"label":"...","type":"action","needle":"oxygen","points":10}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Check {
    /// An `action` event whose text contains `needle` was recorded.
    Action { needle: String, points: u16 },
    /// The same, and no later than `by_sec` on the scenario clock — timing is part of competence.
    ActionBy { needle: String, by_sec: f64, points: u16 },
    /// An `action` matching *any* needle happened — for a step with clinically equivalent routes
    /// (reperfusion by PCI or by thrombolysis), so taking the right one is not penalised.
    ActionAny { any_of: Vec<String>, points: u16 },
    /// No `harm` event whose text contains `needle` occurred. Points are earned by *avoiding* it.
    NoHarm { needle: String, points: u16 },
    /// The run reached a terminal `outcome` named in `any_of`.
    Outcome { any_of: Vec<String>, points: u16 },
    /// ── the only check that can take marks away ──────────────────────────────
    ///
    /// **Orders this case had no indication for.** Counts the interventions the candidate
    /// ordered which no other item on this sheet pays for and which `allow` does not clear,
    /// and takes `per_item` marks for each, to a floor of `max_penalty`.
    ///
    /// It exists because ordering is not free. A candidate who presses every button on the
    /// station scores full marks on a rubric made only of "did they do X" items — the audit
    /// measured exactly that, twelve stations, and eleven of them came out at 40/40 for a tape
    /// that ordered everything the case defines. Nothing in a checklist of positives can see
    /// that, because everything the checklist asks for genuinely happened.
    ///
    /// **It never counts a question or an examination.** Asking more than the mark sheet lists
    /// is normal and good practice, and an examiner does not deduct for it; ordering a drug, a
    /// test or a procedure the patient did not need has a cost, a risk and a delay attached,
    /// and a real mark sheet does deduct for it. So the two are treated differently, and the
    /// split is structural rather than per-rubric: an intervention id beginning `ask_` or
    /// `exam_` is history or examination and is never counted, on any station, without a rubric
    /// author having to remember to say so. `every_station_sorts_its_chips_into_the_two_kinds`
    /// is what keeps that convention true of the shipped cases.
    ///
    /// **What is exempt, and why it is derived rather than listed.** Anything this rubric's own
    /// `action` / `action_by` / `action_any` needles match is exempt automatically: a sheet must
    /// never take marks for an order it is simultaneously paying for. Only `no_harm` and
    /// `outcome` needles are left out of that, because they match harm sentences and outcome
    /// ids rather than intervention ids, and folding them in would exempt by accident.
    ///
    /// **`allow` is the clinical judgement, and it is per case.** It carries the orders a
    /// clinician has cleared for *this* patient even though no item pays for them — fluids for
    /// a pressure of 92, a steroid behind the adrenaline, the admission the rubric only marks
    /// through a `no_harm` — and the orders another item on this sheet already prices, so that
    /// a mistake the case flags as harm is charged once rather than twice. Everything not on it
    /// and not exempt is counted, which is the safe direction: a chip nobody classified costs
    /// marks rather than being silently free.
    NoUnindicated {
        /// Intervention ids this check must not count. See above: clinician-cleared, or already
        /// marked by another item on this sheet.
        #[serde(default)]
        allow: Vec<String>,
        /// What one unindicated order costs.
        per_item: u16,
        /// The most this check can take off, however many were ordered.
        max_penalty: u16,
    },
}

impl Check {
    pub fn points(&self) -> u16 {
        match self {
            Check::Action { points, .. }
            | Check::ActionBy { points, .. }
            | Check::ActionAny { points, .. }
            | Check::NoHarm { points, .. }
            | Check::Outcome { points, .. } => *points,
            // A deduction is not worth anything. It adds nothing to the rubric's maximum — the
            // station is still marked out of the same forty — it only takes.
            Check::NoUnindicated { .. } => 0,
        }
    }

    /// The check's own name, as the rubric JSON spells it. Carried onto the mark sheet so the
    /// page can say "window was 5:00" for a timed item without re-deriving what kind it was.
    pub fn kind(&self) -> &'static str {
        match self {
            Check::Action { .. } => "action",
            Check::ActionBy { .. } => "action_by",
            Check::ActionAny { .. } => "action_any",
            Check::NoHarm { .. } => "no_harm",
            Check::Outcome { .. } => "outcome",
            Check::NoUnindicated { .. } => "no_unindicated",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub label: String,
    #[serde(flatten)]
    pub check: Check,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rubric {
    /// Which case this rubric marks. Bound into the record's `rubric_hash` upstream so a re-mark
    /// and a re-word are distinguishable.
    pub case: String,
    /// Basis points of the deterministic score that earn a star. 7000 = 70%.
    #[serde(default = "default_pass")]
    pub pass_bps: u32,
    /// Free-text provenance / review state. A provisional rubric announces itself here.
    #[serde(default)]
    pub status: String,
    pub items: Vec<Item>,
}
fn default_pass() -> u32 {
    7000
}

/// How one item came out, for a reader rather than for the arithmetic.
///
/// The arithmetic is binary — a check is earned or it is not, and that is what makes the score
/// re-derivable. But "you never gave it" and "you gave it four minutes late" are the same zero
/// and completely different lessons, and a mark sheet whose whole job is to say what to fix
/// first must not flatten them. [`Mark::Partial`] is that distinction and nothing more: it never
/// awards a point, it only says the deed happened and the qualifier did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Earned, in full.
    Hit,
    /// The action happened, outside the window it had to happen in. Worth nothing.
    Partial,
    /// Never done — or, for an avoidance check, done.
    Miss,
}

impl Mark {
    pub fn as_str(self) -> &'static str {
        match self {
            Mark::Hit => "hit",
            Mark::Partial => "partial",
            Mark::Miss => "miss",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemResult {
    pub label: String,
    pub points: u16,
    pub earned: bool,
    /// Which check this was, for the sentence the debrief writes under it.
    pub kind: &'static str,
    pub mark: Mark,
    /// The scenario clock this item's evidence sits at, in seconds: when the action was first
    /// ordered, when the avoided harm fired, when the outcome was reached. `None` means it never
    /// happened — which is the earned answer for a `no_harm` and the failing one for the rest.
    pub at: Option<f64>,
    /// The window the item had to land inside. `Some` only for a timed check.
    pub within: Option<f64>,
    /// What this item *took*, rather than failed to give. Zero for every check but
    /// [`Check::NoUnindicated`], which is the only one that can deduct.
    pub penalty: u16,
    /// The orders this item charged for, by intervention id, in the order they were given.
    /// Empty for every other check. The mark sheet names them — a deduction a candidate cannot
    /// see the reason for teaches nothing, and an examiner who cannot see it cannot review it.
    pub charged: Vec<String>,
}

impl ItemResult {
    /// What this item actually put on the board. The one place a point is turned into a number,
    /// so the sheet and the total cannot be added up two different ways.
    pub fn earned_points(&self) -> u16 {
        if self.earned {
            self.points
        } else {
            0
        }
    }
    /// What it cost. The mark sheet is sorted on this — the biggest hole is the thing to fix.
    /// A deduction costs what it took; every other item costs the points it did not earn.
    pub fn lost(&self) -> u16 {
        (self.points - self.earned_points()).saturating_add(self.penalty)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetResult {
    /// The score of record — what the chain carries, what the star is read off, what the sheet
    /// prints at the top. Normally the items' own total; below it when [`DetResult::capped_from`]
    /// says the death cap took the rest away.
    pub earned: u16,
    pub max: u16,
    pub items: Vec<ItemResult>,
    /// What the items added up to before the death cap, or `None` when no cap applied — which
    /// is every run the patient survived. See [`death_cap`].
    pub capped_from: Option<u16>,
    /// What [`Check::NoUnindicated`] took off the items' own total, before the death cap. Zero
    /// on a run that ordered nothing the case did not need — which is every competent tape in
    /// this file. Kept separate from `earned` so the sheet can show the subtraction rather than
    /// a total that does not add up.
    pub penalty: u16,
}

/// ── the rule that outranks the arithmetic ────────────────────────────────────
///
/// **If the patient died, the run scores below the pass bar. Always. Whatever else it did.**
///
/// The audit found osce-c2 and osce-d4 handing out stars over a body: 29/40 and 32/40, both
/// above 70%, both with the patient dead on the trolley. Nothing was broken in the marking —
/// every one of those points was genuinely earned. The terminal outcome was simply *an item*,
/// worth 3 of 40 on one station and 5 on the other, so a candidate could buy a pass out of the
/// history, the bloods and the imaging and lose the patient on the way.
///
/// That is a defensible way to weight a written paper and an indefensible way to weight this.
/// A star on this shelf is a claim, anchored, that the holder **knew enough to help her** — not
/// that they got a lot of questions right in a room she happened to die in. There is no
/// weighting of a rubric that expresses "and she lived" correctly, because it is not a weight:
/// it is a floor under everything else, and a floor has to be written as one.
///
/// So it is a rule in code, above the items, and it reads the terminal the automaton actually
/// reached rather than any item a rubric author remembered to include. A rubric that forgets to
/// pay for survival is now still safe; a rubric that pays 5 points for it is not thereby saying
/// survival is worth 5 points.
///
/// The cap is the highest score that still fails: the largest `e` for which
/// `e * 10_000 / max < pass_bps`. Not zero — the mark sheet is the lesson, and a candidate who
/// ran a good resuscitation and lost the patient anyway has earned the right to read what they
/// did get. They have simply not earned a star.
fn death_cap(pass_bps: u32, max: u16) -> u16 {
    ((pass_bps as u64 * max as u64).saturating_sub(1) / 10_000) as u16
}

impl DetResult {
    /// The deterministic score in basis points, the shape `vitals-progress::dreyfus` consumes.
    /// Zero max scores as zero — an empty rubric proves nothing, it does not prove perfection.
    pub fn bps(&self) -> u32 {
        if self.max == 0 {
            0
        } else {
            (self.earned as u32 * 10_000) / self.max as u32
        }
    }
    /// Whether this run clears the rubric's bar — earns a star.
    pub fn cleared(&self, rubric: &Rubric) -> bool {
        self.max > 0 && self.bps() >= rubric.pass_bps
    }

    /// The items, read back as one number — always the items' own total, never the capped one
    /// and never the penalised one.
    /// `earned` is accumulated as the checks are walked and this re-adds the sheet the player is
    /// shown; they are two paths to the same total, which is exactly the property
    /// [`sheet_for_run`] refuses to publish a mark sheet without. When the death cap has bitten,
    /// this is what [`DetResult::capped_from`] holds, and the sheet is checked against *that*:
    /// the arithmetic still has to reconcile, it is just no longer the score.
    pub fn items_total(&self) -> u16 {
        self.items.iter().map(ItemResult::earned_points).fold(0u16, u16::saturating_add)
    }

    /// The mark sheet in the order it is meant to be read: the costliest miss first, because the
    /// top of the list is what to fix before sitting the station again. Ties keep the rubric's
    /// own order, so two runs of the same station read the same way.
    pub fn by_loss(&self) -> Vec<&ItemResult> {
        let mut v: Vec<&ItemResult> = self.items.iter().collect();
        v.sort_by_key(|i| std::cmp::Reverse(i.lost()));
        v
    }
}

/// The scenario clock of the first `kind` event whose text contains `needle`, canonicalised on
/// both sides so a full-width or NFC/NFD variant cannot slip a match. `None` if it never happened.
///
/// `kind` is matched exactly, which is what makes `action_refused` invisible to every `action`
/// check in every rubric without one of them being edited: the cath lab that answered "the lab
/// wants an ecg before it spins up" is on the event log, and it is not an action.
fn hit(events: &[Event], kind: &str, needle: &str) -> Option<f64> {
    let n = canon(needle).to_lowercase();
    events
        .iter()
        .filter(|e| e.kind == kind)
        .find(|e| canon(&e.text).to_lowercase().contains(&n))
        .map(|e| e.t_sec)
}

/// The earliest scenario clock at which *any* of `needles` matched, so a check with several
/// clinically equivalent routes reports the one that actually happened rather than the first
/// one the rubric happened to list.
fn first_of(events: &[Event], kind: &str, needles: &[String]) -> Option<f64> {
    needles.iter().filter_map(|n| hit(events, kind, n)).fold(None, |acc: Option<f64>, t| {
        Some(acc.map_or(t, |a| a.min(t)))
    })
}

/// Does `needle` appear in `hay`, by the same rule [`hit`] uses on an event's text?
///
/// Factored out because [`Check::NoUnindicated`] has to ask it of an intervention *id* rather
/// than of an event, and a second spelling of "matches" is a second answer to "did this rubric
/// pay for this order" — which is exactly the disagreement that would let a sheet deduct for
/// something it was also crediting.
fn contains(hay: &str, needle: &str) -> bool {
    canon(hay).to_lowercase().contains(&canon(needle).to_lowercase())
}

/// Whether an intervention id is history-taking or physical examination.
///
/// The convention is the case set's own — `ask_allergy`, `exam_chest` — and it is read here
/// rather than declared per rubric because "asking more is not a fault" is a fact about
/// clinical examinations in general, not about any one patient. A rubric author cannot forget
/// it, and cannot get it wrong for one station.
fn is_assessment(id: &str) -> bool {
    let id = canon(id).to_lowercase();
    id.starts_with("ask_") || id.starts_with("exam_")
}

/// Every order the run gave, by intervention id, first time each, in the order given.
///
/// `action_refused` counts. An order the case turned down still shows the judgement that made
/// it — the needle aimed at a sac with nothing in it, the mist held out for a child who is
/// settled — and the case refusing it is not the candidate deciding against it. The gated
/// orders that are *right but early* (the cath lab before the ECG, heparin before the scan,
/// GI before the resuscitation) are all paid for by an item on their own sheet, so the
/// exemption below clears them without any of this needing to know they are special.
fn ordered(events: &[Event]) -> Vec<(String, f64)> {
    let mut seen: Vec<(String, f64)> = Vec::new();
    for e in events.iter().filter(|e| e.kind == "action" || e.kind == "action_refused") {
        if !seen.iter().any(|(id, _)| id == &e.text) {
            seen.push((e.text.clone(), e.t_sec));
        }
    }
    seen
}

/// The orders `rubric` pays for somewhere, as needles to test an id against.
///
/// Deliberately only the three action checks. A `no_harm` needle is a sentence out of a harm
/// event ("the itch was never the emergency") and an `outcome` needle is a terminal id, and
/// letting either exempt an intervention would clear orders by coincidence of wording.
fn credited_needles(rubric: &Rubric) -> Vec<&str> {
    let mut v = Vec::new();
    for it in &rubric.items {
        match &it.check {
            Check::Action { needle, .. } | Check::ActionBy { needle, .. } => v.push(needle.as_str()),
            Check::ActionAny { any_of, .. } => v.extend(any_of.iter().map(String::as_str)),
            Check::NoHarm { .. } | Check::Outcome { .. } | Check::NoUnindicated { .. } => {}
        }
    }
    v
}

/// The orders this run gave that `rubric` neither pays for nor clears — what
/// [`Check::NoUnindicated`] charges for. Public because it is also the review surface: a
/// clinician reading a station wants the list, not the arithmetic.
pub fn unindicated(events: &[Event], rubric: &Rubric, allow: &[String]) -> Vec<String> {
    let credited = credited_needles(rubric);
    ordered(events)
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| !is_assessment(id))
        .filter(|id| !allow.iter().any(|a| canon(a).to_lowercase() == canon(id).to_lowercase()))
        .filter(|id| !credited.iter().any(|n| contains(id, n)))
        .collect()
}

/// Score a replayed run against a rubric. Pure and total: same events + same rubric + same
/// terminal → same result, which is the property the claim path depends on.
///
/// One walk produces both halves of the answer — the total the chain carries, and the per-item
/// mark sheet the debrief shows. There is deliberately no second function that re-reads the
/// events to explain the score: a sheet computed on its own path is a sheet that can disagree
/// with the number a verifier re-derives, and then neither of them is evidence.
///
/// `ended` is the terminal the automaton reached, and it is a parameter rather than something
/// read back out of the event log on purpose: the log carries the outcome's *id*, and only the
/// scenario knows whether an id it declared is a win or a death. Every caller therefore has to
/// answer the question, which is how [`death_cap`] stays impossible to skip.
pub fn score(events: &[Event], rubric: &Rubric, ended: Option<Outcome>) -> DetResult {
    let mut earned = 0u16;
    let mut max = 0u16;
    let mut items = Vec::with_capacity(rubric.items.len());
    for it in &rubric.items {
        let p = it.check.points();
        max = max.saturating_add(p);
        // `at` is the evidence, `ok` is the verdict, and both come off the same lookup — the
        // mark sheet quotes the very seconds the check was decided on.
        let mut penalty = 0u16;
        let mut charged: Vec<String> = Vec::new();
        let (ok, at, within) = match &it.check {
            Check::Action { needle, .. } => {
                let at = hit(events, "action", needle);
                (at.is_some(), at, None)
            }
            Check::ActionBy { needle, by_sec, .. } => {
                let at = hit(events, "action", needle);
                (at.is_some_and(|t| t <= *by_sec), at, Some(*by_sec))
            }
            Check::ActionAny { any_of, .. } => {
                let at = first_of(events, "action", any_of);
                (at.is_some(), at, None)
            }
            // The clock here is when the harm fired, not when it was avoided — an avoidance
            // check that was kept has nothing to point at, which is the whole idea.
            Check::NoHarm { needle, .. } => {
                let at = hit(events, "harm", needle);
                (at.is_none(), at, None)
            }
            Check::Outcome { any_of, .. } => {
                let at = first_of(events, "outcome", any_of);
                (at.is_some(), at, None)
            }
            // The deduction. `at` is the second the first charged order was given, so the sheet
            // can say when the over-ordering started; there is no window to quote.
            Check::NoUnindicated { allow, per_item, max_penalty } => {
                charged = unindicated(events, rubric, allow);
                penalty = per_item
                    .saturating_mul(charged.len().min(u16::MAX as usize) as u16)
                    .min(*max_penalty);
                let at = charged.first().and_then(|id| hit(events, "action", id));
                (penalty == 0, at, None)
            }
        };
        if ok {
            earned = earned.saturating_add(p);
        }
        let mark = match (ok, &it.check, at) {
            (true, _, _) => Mark::Hit,
            // The only partial the check set can produce: the drug was given, the window shut.
            (false, Check::ActionBy { .. }, Some(_)) => Mark::Partial,
            _ => Mark::Miss,
        };
        items.push(ItemResult {
            label: it.label.clone(),
            points: p,
            earned: ok,
            kind: it.check.kind(),
            mark,
            at,
            within,
            penalty,
            charged,
        });
    }
    // ── the deduction, taken off the items' own total ────────────────────────
    // Before the death cap and after everything else, because it is arithmetic about the run's
    // marks and the cap is a rule about the run's ending. `max` never moves: over-ordering does
    // not make a station worth more, it makes this run worth less, and the forty a verifier
    // re-derives has to be the same forty however the run went.
    // A deduction cannot take a mark that was never earned: the floor is zero, and clamping is
    // done on the sheet's own rows rather than only on the total, so what the candidate reads
    // ("−6") and what came off the score are the same number. Without this a run that earned 4
    // and over-ordered 6 would show a −6 against a −4 subtraction, and the sheet would stop
    // adding up — which `sheet_for_run` refuses to publish.
    let mut left = earned;
    for it in items.iter_mut().filter(|i| i.penalty > 0) {
        it.penalty = it.penalty.min(left);
        left -= it.penalty;
    }
    let penalty = items.iter().map(|i| i.penalty).fold(0u16, u16::saturating_add);
    earned -= penalty;
    // The floor, applied last and to the total rather than to any item — see [`death_cap`].
    let capped_from = ended.filter(|o| o.is_death()).and_then(|_| {
        let cap = death_cap(rubric.pass_bps, max);
        (earned > cap).then(|| {
            let was = earned;
            earned = cap;
            was
        })
    });
    let det = DetResult { earned, max, items, capped_from, penalty };
    debug_assert_eq!(
        det.capped_from.unwrap_or(det.earned) + det.penalty,
        det.items_total(),
        "the mark sheet does not add up to the score"
    );
    det
}

/// Score a finished run for anchoring, in one call: replay its tape, mark it against `rubric_json`,
/// and return `(det_score, det_max, rubric_hash)` ready to stamp into the leaf.
///
/// This is the exact path a verifier re-runs — `vitals_replay::resume` reproduces the events from
/// the tape, `score` marks them, `rubric_hash` pins which rubric did the marking — so `det_score`
/// is re-derivable by anyone with the tape and the pinned rubric, which is what lets it, and only
/// it, back an on-chain claim. The rubric is passed as its authored bytes so the hash a verifier
/// recomputes is byte-identical.
pub fn det_for_run(
    sce_json: &str,
    tape: &[Step],
    rubric_json: &str,
) -> Result<(u16, u16, [u8; 32]), String> {
    let (_, det) = sheet_for_run(sce_json, tape, rubric_json)?;
    Ok((det.earned, det.max, vitals_progress::record::rubric_hash(rubric_json.as_bytes())))
}

/// Mark a finished run and hand back the whole sheet — the rubric it was marked against and every
/// item, with the seconds each was decided on.
///
/// This is [`det_for_run`]'s own body: the anchor path calls it and throws the sheet away, the
/// debrief calls it and keeps it. That is the point. A player who is told "adrenaline, 0 of 10,
/// given at 6:12, window was 5:00" is being shown the arithmetic that produced the number on
/// chain, not a second opinion about it — so the sheet is checked against the total before it is
/// returned, and a sheet that does not add up is an error rather than a page.
pub fn sheet_for_run(
    sce_json: &str,
    tape: &[Step],
    rubric_json: &str,
) -> Result<(Rubric, DetResult), String> {
    let rubric: Rubric = serde_json::from_str(rubric_json).map_err(|e| e.to_string())?;
    let (state, _replay) = resume(sce_json, tape)?;
    // The terminal is read off the replayed automaton, never off a caller — the death cap is
    // only worth anything if it reads the same fact a verifier re-derives.
    let det = score(state.events(), &rubric, state.outcome());
    // Not a `debug_assert`: this runs inside the server that serves the debrief, and the one
    // failure worth catching in release is the one where the sheet and the star disagree. It
    // returns rather than panics for the same reason — a bay must not die of a bad rubric.
    // The sheet reconciles against what the items earned, which is the score unless the death
    // cap took the rest — and then it reconciles against the pre-cap total, because the cap is
    // a rule about the run and not an arithmetic error in the sheet. A `no_unindicated`
    // deduction is on the sheet as its own row, so it is part of the sum rather than an
    // exception to it.
    // The deduction is part of the subtraction the sheet has to show: items earned, minus what
    // over-ordering took, is the score — unless the death cap then took the rest.
    if det.capped_from.unwrap_or(det.earned).saturating_add(det.penalty) != det.items_total() {
        return Err(format!(
            "mark sheet does not add up: items total {} against a det score of {} and a penalty of {}",
            det.items_total(),
            det.capped_from.unwrap_or(det.earned),
            det.penalty
        ));
    }
    Ok((rubric, det))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, text: &str, t: f64) -> Event {
        Event { t_sec: t, kind: kind.into(), text: text.into() }
    }
    fn rubric() -> Rubric {
        serde_json::from_str(
            r#"{ "case":"test", "pass_bps":7000, "items":[
                {"label":"oxygen","type":"action","needle":"oxygen","points":10},
                {"label":"early abx","type":"action_by","needle":"antibiotic","by_sec":600,"points":10},
                {"label":"no wrong shock","type":"no_harm","needle":"shock","points":10},
                {"label":"survives","type":"outcome","any_of":["WinDischarge","WinIcu"],"points":10}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn perfect_run_scores_full_and_clears() {
        let evs = [
            ev("action", "gave Oxygen face mask", 30.0),
            ev("action", "antibiotic ceftriaxone", 300.0),
            ev("outcome", "WinDischarge", 900.0),
        ];
        let r = score(&evs, &rubric(), Some(Outcome::WinDischarge));
        assert_eq!((r.earned, r.max), (40, 40));
        assert_eq!(r.bps(), 10_000);
        assert!(r.cleared(&rubric()));
    }

    #[test]
    fn a_late_critical_action_loses_only_its_points() {
        let evs = [
            ev("action", "oxygen", 10.0),
            ev("action", "antibiotic", 900.0), // past the 600s window
            ev("outcome", "WinDischarge", 1000.0),
        ];
        let r = score(&evs, &rubric(), Some(Outcome::WinDischarge));
        assert_eq!(r.earned, 30); // oxygen + no-harm(shock absent) + outcome; abx too late
    }

    #[test]
    fn a_harm_event_fails_the_avoidance_check() {
        let evs = [
            ev("harm", "shock on a perfusing rhythm", 50.0),
            ev("action", "oxygen", 10.0),
        ];
        let r = score(&evs, &rubric(), None);
        assert_eq!(r.earned, 10); // only oxygen; shock harm fired, abx & outcome absent
        assert!(!r.cleared(&rubric()));
    }

    #[test]
    fn canon_defeats_a_fullwidth_dodge() {
        // NFKC folds full-width ＯＸＹＧＥＮ to ASCII, so a cosmetic variant still matches.
        let evs = [ev("action", "ＯＸＹＧＥＮ mask", 5.0)];
        assert!(hit(&evs, "action", "oxygen").is_some());
    }

    #[test]
    fn action_any_credits_either_equivalent_route() {
        let r: Rubric = serde_json::from_str(
            r#"{"case":"t","items":[
                {"label":"reperfusion","type":"action_any","any_of":["cath","thrombolys"],"points":12}
            ]}"#,
        )
        .unwrap();
        // PCI alone earns it; thrombolysis alone earns it; neither earns nothing.
        assert_eq!(score(&[ev("action", "cath_lab", 400.0)], &r, None).earned, 12);
        assert_eq!(score(&[ev("action", "thrombolysis", 400.0)], &r, None).earned, 12);
        assert_eq!(score(&[ev("action", "aspirin", 60.0)], &r, None).earned, 0);
    }

    #[test]
    fn empty_rubric_proves_nothing() {
        let r = DetResult { earned: 0, max: 0, items: vec![], capped_from: None, penalty: 0 };
        assert_eq!(r.bps(), 0);
        assert!(!r.cleared(&rubric()));
    }

    #[test]
    fn det_for_run_marks_a_real_replay_and_pins_the_rubric() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}scenarios/ep2-stemi.json")).unwrap();
        let rubric = std::fs::read_to_string(format!("{root}rubrics/ep2-stemi.json")).unwrap();
        // A competent STEMI: ECG, aspirin, oxygen, reperfuse, then time to salvage muscle.
        let tape = vec![
            Step::Do("12-lead ecg".into()),
            Step::Tick(20.0),
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(15.0),
            Step::Do("oxygen".into()),
            Step::Tick(15.0),
            Step::Do("activate the cath lab for pci".into()),
            Step::Tick(200.0),
            Step::Tick(60.0),
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric).unwrap();
        assert_eq!((s, m), (40, 40)); // full marks — the re-derivable 40
        assert_ne!(rhash, [0u8; 32]);
        // The pin is stable and equals the standalone hash of the same bytes.
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric.as_bytes()));
    }

    /// Station A (osce-a, from embla-cases ddx-anaphylaxis-1), marked by the exact anchor path.
    /// The competent tape speaks in the chip texts the UI actually sends — asks included, because
    /// at a station an ask IS a do — and earns the full 40; the antihistamine-and-wait tape lets
    /// the adrenaline window close and lands under the bar, which is what makes the star a claim.
    #[test]
    fn station_a_competent_run_clears_and_hesitation_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-a.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-a.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-a: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("any allergies?".into()),
            Step::Tick(15.0),
            Step::Do("what did you eat before this?".into()),
            Step::Tick(15.0),
            Step::Do("adrenaline im".into()), // t=30 — inside the five-minute window
            Step::Tick(10.0),
            Step::Do("oxygen mask".into()),
            Step::Tick(10.0),
            Step::Do("serum tryptase".into()),
            Step::Tick(5.0),
            Step::Do("12-lead ecg".into()),
            Step::Tick(5.0),
            Step::Do("anaphylaxis".into()),
            Step::Tick(160.0), // the observation window runs out to discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Antihistamine alone, then waiting: the delay harm fires at five minutes, the patient
        // arrests, and the run must not clear — retry with feedback is the mastery model.
        let hesitation = vec![
            Step::Do("chlorpheniramine".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &hesitation, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "a run without adrenaline must not clear: {s}/{m}");
    }

    /// Station B (osce-b, from embla-cases ddx-possible-nstemi-stemi-2): chest pain → ECG inside
    /// ten minutes → reperfusion decision. The cath lab refuses a hunch — activating it without
    /// an ECG earns the action but never the outcome, and stays under the bar.
    #[test]
    fn station_b_competent_run_clears_and_a_hunch_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-b.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-b: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("where is the pain?".into()),
            Step::Tick(20.0),
            Step::Do("any risk factors — smoking, sugar, pressure?".into()),
            Step::Tick(20.0),
            Step::Do("12-lead ecg".into()), // t=40 — door-to-ECG well inside ten minutes
            Step::Tick(10.0),
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(10.0),
            Step::Do("troponin".into()),
            Step::Tick(10.0),
            Step::Do("acute stemi".into()),
            Step::Tick(5.0),
            Step::Do("activate the cath lab".into()),
            Step::Tick(190.0), // door-to-balloon runs; muscle mostly intact → win_discharge
        ];
        let (s, m, _) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        // Cath lab on a hunch, no ECG ever. The lab refuses — "the lab wants an ecg before it
        // spins up" — and the refusal is now recorded as `action_refused`, so the ten-point
        // reperfusion item is a miss rather than a hit. It used to be a hit: `record("action")`
        // fired before the branch that turned the order down, and the candidate was paid in
        // full for a reperfusion that never happened.
        let hunch = vec![
            Step::Do("activate the cath lab".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (_, sheet) = sheet_for_run(&sce, &hunch, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        assert!(!sheet.cleared(&r), "reperfusion credit without the ECG must not clear: {}/{}", sheet.earned, sheet.max);
        let reperfusion = sheet
            .items
            .iter()
            .find(|i| i.label.starts_with("Reperfusion"))
            .expect("the reperfusion item");
        assert_eq!(
            (reperfusion.earned, reperfusion.mark),
            (false, Mark::Miss),
            "a cath lab that said no still paid for reperfusion"
        );
        assert_eq!(sheet.earned, 3, "a refused order left points behind: {:?}", sheet.items);
    }

    /// **The refused order, on the three stations the audit named.** Each of these is a gated
    /// intervention: the case hears the order, checks a precondition, and turns it down in
    /// words — no flag, no variable, no harm, nothing on the patient. `record("action", …)` used
    /// to fire before that branch was even evaluated, so every one of them was worth full marks
    /// to a candidate who never earned the thing being marked.
    ///
    /// This is the property, and it is stated per station in points because points are what the
    /// star is made of.
    #[test]
    fn an_order_the_case_refused_earns_nothing() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        // (station, the order, the rubric label it must no longer pay for, what it was worth)
        for (ep, order, label, worth) in [
            ("osce-b", "activate the cath lab", "Reperfusion decision", 10u16),
            ("osce-d2", "heparin", "Anticoagulation", 6),
            ("osce-d", "call gi — urgent endoscopy", "Called GI", 4),
        ] {
            let sce = std::fs::read_to_string(format!("{root}stations/{ep}.sce.json")).unwrap();
            let rubric_json = std::fs::read_to_string(format!("{root}rubrics/{ep}.json")).unwrap();
            let tape = vec![Step::Do(order.into()), Step::Tick(200.0), Step::Tick(200.0)];

            // The order is on the record — the debrief has to be able to say the lab said no —
            // and it is on it under a kind no rubric asks about.
            let (state, _) = resume(&sce, &tape).unwrap();
            let kinds: Vec<&str> = state.events().iter().map(|e| e.kind.as_str()).collect();
            assert!(
                kinds.contains(&vitals_sce::runtime::ACTION_REFUSED),
                "{ep}: the refusal left no record at all: {kinds:?}"
            );
            assert!(
                !kinds.contains(&vitals_sce::runtime::ACTION),
                "{ep}: a refused order is still filed as an action: {kinds:?}"
            );

            let (_, sheet) = sheet_for_run(&sce, &tape, &rubric_json).unwrap();
            let item = sheet
                .items
                .iter()
                .find(|i| i.label.starts_with(label))
                .unwrap_or_else(|| panic!("{ep}: no item named {label:?}"));
            assert_eq!(item.points, worth, "{ep}: {label} is not worth what the audit measured");
            assert!(!item.earned, "{ep}: {label} still pays {worth} for an order that was refused");
            assert!(item.at.is_none(), "{ep}: {label} still quotes a second for something that did not happen");
        }
    }

    /// The other half of the same change, and the one that would be expensive to get wrong: a
    /// refusal is a fact about the *marking*, not about the run. The tape, the beats, the harm
    /// list and the outcome are what `vitals_replay::leaf` hashes; the event log is not in it at
    /// all. So every run anchored before this change replays to the identical leaf after it.
    #[test]
    fn refusing_an_order_moves_the_mark_and_never_the_leaf() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b.sce.json")).unwrap();
        let tape = vec![
            Step::Do("activate the cath lab".into()),
            Step::Tick(200.0), Step::Tick(200.0), Step::Tick(200.0), Step::Tick(200.0),
        ];
        let (state, replay) = resume(&sce, &tape).unwrap();

        // Pinned by value, not by "it did not change": these four are the leaf's whole input.
        assert_eq!(
            replay.beats,
            vec![
                "threshold:the lab wants an ecg before it spins up".to_string(),
                "harm:ecg delayed beyond ten minutes — the infarct ran unseen".to_string(),
                "threshold:he shifts against the trolley — the arm pain is back, heavier".to_string(),
                "status:Critical".to_string(),
                "terminal:DeathArrest".to_string(),
            ],
            "the refusal changed what the case said"
        );
        assert_eq!(replay.harm_events.len(), 1, "the refusal changed the harm list");
        assert_eq!(replay.outcome.as_deref(), Some("DeathArrest"), "the refusal changed the outcome");
        // Those four and the tape are the leaf's entire input — `vitals_replay::leaf` takes a
        // `&Replay`, which has no event log in it — so pinning them pins the leaf without
        // hard-coding a hash that an author legitimately rewriting the case would have to come
        // and edit. Replaying twice is the determinism half of the same statement.
        let leaf_of = |t: &[Step]| {
            let r = vitals_replay::replay(&sce, t).unwrap();
            vitals_replay::leaf(&vitals_replay::sce_hash(&sce), t, &r)
        };
        assert_eq!(leaf_of(&tape), leaf_of(&tape), "the leaf is not a function of the tape");

        // And `done` is untouched, because the scenarios' own conditions read it: a refused
        // order that fell out of `done` would change what cases *do*, and that would change a
        // leaf. The proof is the beat: `done` is what the refusal branch is testing against.
        assert!(
            state.events().iter().any(|e| e.kind == vitals_sce::runtime::ACTION_REFUSED && e.text == "cath_lab"),
            "the refusal is not on the record under its own name"
        );
    }

    /// Station C (osce-c, from embla-cases ddx-croup-2): a drooling child who looks like EP3's
    /// boy but is loud, afebrile, vaccinated and barking — croup, not epiglottitis. The competent
    /// tape scores the severity from the doorway, gives the steroid, and touches nothing that
    /// does not need touching. The tongue-depressor tape does everything else right — including
    /// the nebulised-adrenaline rescue — and still lands under the bar, because the one
    /// invasive exam is the station's whole lesson.
    #[test]
    fn station_c_competent_run_clears_and_the_tongue_depressor_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-c.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-c.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-c: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("has she had this before?".into()),
            Step::Tick(10.0),
            Step::Do("any fever?".into()),
            Step::Tick(10.0),
            Step::Do("are her shots up to date?".into()),
            Step::Tick(10.0),
            Step::Do("score her from the doorway".into()),
            Step::Tick(10.0),
            Step::Do("check the saturations".into()),
            Step::Tick(10.0),
            Step::Do("neck and chest films".into()),
            Step::Tick(10.0),
            Step::Do("dexamethasone syrup".into()), // t=60 — the whole visit, given early
            Step::Tick(10.0),
            Step::Do("keep her on mum's lap".into()),
            Step::Tick(10.0),
            Step::Do("watch her for two hours".into()),
            Step::Tick(10.0),
            Step::Do("croup".into()),
            Step::Tick(160.0), // the observation runs out to a discharge before midnight
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // The depressor goes in early; the child cries herself into stridor; the rescue and
        // every other mark are earned — and the run must still fail on the harm it caused.
        let depressor = vec![
            Step::Do("any fever?".into()),
            Step::Tick(5.0),
            Step::Do("score her from the doorway".into()),
            Step::Tick(5.0),
            Step::Do("look in the throat".into()), // the one forbidden exam
            Step::Tick(5.0),
            Step::Do("nebulised adrenaline".into()), // competent rescue, too late to matter
            Step::Tick(5.0),
            Step::Do("has she had this before?".into()),
            Step::Tick(5.0),
            Step::Do("are her shots up to date?".into()),
            Step::Tick(5.0),
            Step::Do("check the saturations".into()),
            Step::Tick(5.0),
            Step::Do("neck and chest films".into()),
            Step::Tick(5.0),
            Step::Do("dexamethasone syrup".into()),
            Step::Tick(5.0),
            Step::Do("watch her for two hours".into()),
            Step::Tick(5.0),
            Step::Do("croup".into()),
            Step::Tick(130.0), // she settles — into an admission, not a discharge
        ];
        let (s, m, _) = det_for_run(&sce, &depressor, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the tongue depressor must not clear the bar: {s}/{m}");
    }

    /// Station D (osce-d, from embla-cases embla-upper-gastrointestinal-bleeding-intern): call
    /// the shock while he compensates, two big lines, crystalloid AND blood, then the scope.
    /// GI is gated like B's cath lab — no resuscitation, no endoscopy. The failing tape is the
    /// chest-pain reflex: aspirin for a CAD history, one bag of fluid through a thin line, hope.
    #[test]
    fn station_d_competent_run_clears_and_the_chest_pain_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-d.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-d.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-d: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("what pills do you take every day?".into()),
            Step::Tick(10.0),
            Step::Do("how much blood — what colour?".into()),
            Step::Tick(10.0),
            Step::Do("feel his hands, look at his eyes".into()),
            Step::Tick(10.0),
            Step::Do("two large-bore lines".into()), // t=30 — access before the fourth minute
            Step::Tick(10.0),
            Step::Do("group and crossmatch four units".into()),
            Step::Tick(10.0),
            Step::Do("warmed crystalloid, wide open".into()),
            Step::Tick(10.0),
            Step::Do("transfuse packed cells".into()),
            Step::Tick(10.0),
            Step::Do("pantoprazole bolus and infusion".into()),
            Step::Tick(10.0),
            Step::Do("hold the aspirin and clopidogrel".into()),
            Step::Tick(10.0),
            Step::Do("rectal exam".into()),
            Step::Tick(10.0),
            Step::Do("upper gi bleed".into()),
            Step::Tick(10.0),
            Step::Do("call gi — urgent endoscopy".into()), // tank full — GI takes the call
            Step::Tick(160.0), // clipped, dry, and up to the unit
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Aspirin for the stent, one bag through a thin line, and waiting: the access harm
        // fires at four minutes, the re-bleed at five finds an empty tank, and he exsanguinates.
        let reflex = vec![
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(20.0),
            Step::Do("warmed crystalloid, wide open".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &reflex, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the chest-pain reflex must not clear the bar: {s}/{m}");
    }

    /// Station A2 (osce-a2, from embla-cases ddx-anaphylaxis-2): anaphylaxis in a gut disguise —
    /// cramps, diarrhoea, a blackout she calls sitting down, and a pressure already at 86. The
    /// competent tape digs the collapse out of an evasive history and treats the shock, not the
    /// itch. The failing tape is the antihistamine-first reflex: chlorpheniramine, then waiting,
    /// while both the reflex harm and the five-minute window fire and the pressure runs out.
    #[test]
    fn station_a2_competent_run_clears_and_the_antihistamine_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-a2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-a2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-a2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("any allergies?".into()),
            Step::Tick(10.0),
            Step::Do("what did you eat today?".into()),
            Step::Tick(10.0),
            Step::Do("did you faint — even for a moment?".into()),
            Step::Tick(10.0),
            Step::Do("adrenaline im".into()), // t=30 — well inside the five-minute window
            Step::Tick(10.0),
            Step::Do("oxygen mask".into()),
            Step::Tick(5.0),
            Step::Do("normal saline bolus".into()),
            Step::Tick(5.0),
            Step::Do("serum tryptase".into()),
            Step::Tick(5.0),
            Step::Do("anaphylaxis".into()),
            Step::Tick(160.0), // the observation window runs out to discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Chlorpheniramine for the itch, then waiting: the antihistamine-first harm fires at two
        // minutes, the window harm at five, and she arrests — nowhere near the bar.
        let reflex = vec![
            Step::Do("chlorpheniramine".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &reflex, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the antihistamine reflex must not clear the bar: {s}/{m}");
    }

    /// Station B2 (osce-b2, from embla-cases ddx-pericarditis-1): ST elevation is a finding, not
    /// a diagnosis. A febrile fourteen-year-old with a rub and a favourite chair — the competent
    /// tape reads the shape off twelve leads and treats with tablets. The failing tape is the
    /// STEMI reflex from station B run on the wrong chest: aspirin, cath lab, lytics — and the
    /// sac fills with blood.
    #[test]
    fn station_b2_competent_run_clears_and_the_stemi_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-b2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-b2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("where is the pain — what makes it better?".into()),
            Step::Tick(10.0),
            Step::Do("does breathing change it?".into()),
            Step::Tick(10.0),
            Step::Do("any fever or a cold lately?".into()),
            Step::Tick(10.0),
            Step::Do("listen to the heart — sit him forward".into()),
            Step::Tick(10.0),
            Step::Do("12-lead ecg".into()), // t=40 — inside ten minutes, and read for its shape
            Step::Tick(10.0),
            Step::Do("troponin".into()),
            Step::Tick(10.0),
            Step::Do("echocardiogram".into()),
            Step::Tick(10.0),
            Step::Do("pericarditis".into()),
            Step::Tick(5.0),
            Step::Do("ibuprofen with food".into()),
            Step::Tick(160.0), // rest on tablets runs out to a discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // The reflex: call it a STEMI, load aspirin, spin the lab, push the lytic. The sac
        // fills, the pressure falls, and the run lands nowhere near the bar.
        let reflex = vec![
            Step::Do("12-lead ecg".into()),
            Step::Tick(10.0),
            Step::Do("aspirin 300 chewed".into()),
            Step::Tick(10.0),
            Step::Do("call it a stemi".into()),
            Step::Tick(10.0),
            Step::Do("activate the cath lab".into()),
            Step::Tick(10.0),
            Step::Do("thrombolysis".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &reflex, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the stemi reflex must not clear the bar: {s}/{m}");
    }

    /// Station B3 (osce-b3, from embla-cases ddx-croup-1): the mild rung of the croup ladder —
    /// grade her from the doorway, one syrup of dexamethasone, an hour of watching, the speech,
    /// home. The failing tape treats a virus with antibiotics and reaches for the door with
    /// nothing behind it; the steroid never comes, and the airway stops waiting.
    #[test]
    fn station_b3_competent_run_clears_and_antibiotics_for_a_virus_fail() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-b3.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-b3.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-b3: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("when did the bark start?".into()),
            Step::Tick(10.0),
            Step::Do("any fever?".into()),
            Step::Tick(10.0),
            Step::Do("is she drinking?".into()),
            Step::Tick(10.0),
            Step::Do("score her from the doorway".into()),
            Step::Tick(10.0),
            Step::Do("check the saturations".into()),
            Step::Tick(10.0),
            Step::Do("neck and chest films".into()),
            Step::Tick(10.0),
            Step::Do("dexamethasone syrup".into()), // t=60 — the whole visit, given early
            Step::Tick(10.0),
            Step::Do("watch her for an hour".into()),
            Step::Tick(10.0),
            Step::Do("give the safety-net advice".into()),
            Step::Tick(10.0),
            Step::Do("croup".into()),
            Step::Tick(220.0), // the watched hour runs out to a discharge
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Amoxicillin for a bark and straight for the door: the discharge harm fires, the
        // steroid never comes, the seventh minute tips her into the night she didn't have.
        let bottle = vec![
            Step::Do("any fever?".into()),
            Step::Tick(10.0),
            Step::Do("amoxicillin for the throat".into()),
            Step::Tick(10.0),
            Step::Do("send her home".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &bottle, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "antibiotics-and-home must not clear the bar: {s}/{m}");
    }

    /// Station C2 (osce-c2, from embla-cases ddx-bronchospasm-acute-asthma-exacerbation-2): the
    /// bronchodilator ladder with its feet on a number — peak flow, salbutamol, ipratropium,
    /// peak flow again, and a systemic steroid behind the neb. The failing tape reads her fear
    /// as panic and sedates the only drive that was holding her.
    #[test]
    fn station_c2_competent_run_clears_and_the_sedative_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-c2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-c2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-c2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("how often does this happen?".into()),
            Step::Tick(10.0),
            Step::Do("can you finish a sentence?".into()),
            Step::Tick(10.0),
            Step::Do("listen to the chest".into()),
            Step::Tick(10.0),
            Step::Do("peak flow — measure it".into()), // the baseline number
            Step::Tick(10.0),
            Step::Do("oxygen".into()),
            Step::Tick(10.0),
            Step::Do("salbutamol neb".into()), // t=50 — the first rung
            Step::Tick(10.0),
            Step::Do("prednisolone 40 mg".into()), // t=60 — the steroid behind the neb, early
            Step::Tick(10.0),
            Step::Do("ipratropium".into()), // the second rung
            Step::Tick(10.0),
            Step::Do("peak flow again".into()), // reassess — the ladder answers in numbers
            Step::Tick(10.0),
            Step::Do("inhaled steroid + action plan".into()),
            Step::Tick(5.0),
            Step::Do("acute asthma exacerbation".into()),
            Step::Tick(250.0), // settled, rechecked, steroid on board → home with a plan
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // One neb, then diazepam for the "panic", twice more when she keeps fighting it: the
        // respiratory drive goes, the saturation follows, and the run must not clear.
        let sedated = vec![
            Step::Do("salbutamol neb".into()),
            Step::Tick(30.0),
            Step::Do("diazepam for the panic".into()),
            Step::Tick(30.0),
            Step::Do("more diazepam".into()),
            Step::Tick(30.0),
            Step::Do("more diazepam".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &sedated, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the sedative must not clear the bar: {s}/{m}");
    }

    /// Station C3 (osce-c3, from embla-cases ddx-pneumonia-2): the diagnosis is the easy half —
    /// the station is disposition. CURB-65 of zero on a woman saturating 93%: score her, dose
    /// her inside the hour, and let the oximeter outvote the arithmetic. The failing tape trusts
    /// the score and sends the lobe home untreated.
    #[test]
    fn station_c3_competent_run_clears_and_the_score_says_home_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-c3.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-c3.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-c3: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("tell me about the cough".into()),
            Step::Tick(10.0),
            Step::Do("any illnesses? do you smoke?".into()),
            Step::Tick(10.0),
            Step::Do("listen to the chest".into()),
            Step::Tick(10.0),
            Step::Do("count the breathing".into()),
            Step::Tick(10.0),
            Step::Do("chest x-ray".into()),
            Step::Tick(10.0),
            Step::Do("full blood count".into()),
            Step::Tick(10.0),
            Step::Do("sputum and blood cultures".into()),
            Step::Tick(10.0),
            Step::Do("curb-65 — score her".into()),
            Step::Tick(10.0),
            Step::Do("co-amoxiclav plus macrolide — first dose now".into()), // t=80, inside the hour
            Step::Tick(10.0),
            Step::Do("oxygen".into()),
            Step::Tick(10.0),
            Step::Do("pneumonia".into()),
            Step::Tick(5.0),
            Step::Do("admit to a short-stay bed".into()),
            Step::Tick(160.0), // the ward takes her, first dose already running
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // "She scored zero": the score is right, the disposition is wrong. Home untreated, the
        // hour slides past, the lobe keeps growing — the harm and the missing dose sink it.
        let arithmetic = vec![
            Step::Do("curb-65 — score her".into()),
            Step::Tick(10.0),
            Step::Do("pneumonia".into()),
            Step::Tick(10.0),
            Step::Do("home with tablets".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &arithmetic, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "score-says-home must not clear the bar: {s}/{m}");
    }

    /// Station D2 (osce-d2, from embla-cases ddx-pulmonary-embolism-2): pretest thinking —
    /// Wells out loud, then the CTPA the score demands, then the anticoagulation the scan has
    /// earned (gated like B's cath lab). The failing tape stalls on a d-dimer that could never
    /// say no and pushes heparin on a hunch the gate refuses; the clot finishes the argument.
    #[test]
    fn station_d2_competent_run_clears_and_the_dimer_stall_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-d2.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-d2.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-d2: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("what were you doing when it started?".into()),
            Step::Tick(10.0),
            Step::Do("any illnesses — any tablets?".into()),
            Step::Tick(10.0),
            Step::Do("how are your legs?".into()),
            Step::Tick(10.0),
            Step::Do("examine the calves".into()),
            Step::Tick(10.0),
            Step::Do("wells score".into()), // high before a single test
            Step::Tick(10.0),
            Step::Do("ct pulmonary angiogram".into()), // the scan the score demanded
            Step::Tick(10.0),
            Step::Do("low-molecular-weight heparin".into()), // t=60 — a clot with a name, starved
            Step::Tick(10.0),
            Step::Do("oxygen".into()),
            Step::Tick(5.0),
            Step::Do("pulmonary embolism".into()),
            Step::Tick(5.0),
            Step::Do("admit to the unit".into()),
            Step::Tick(160.0), // the infusion runs; the unit takes her
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // The stall: a dimer at high probability, heparin on a hunch the gate refuses, and
        // waiting. The clot extends at six minutes and she greys out — under the bar, even
        // with the ungated action credit.
        let stall = vec![
            Step::Do("d-dimer".into()),
            Step::Tick(10.0),
            Step::Do("heparin".into()), // refused — the scan first
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &stall, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the dimer stall must not clear the bar: {s}/{m}");
    }

    /// Station D3 (osce-d3, from embla-cases ddx-p-anaphylaxis-1, re-aged paediatric per the set
    /// design): the season's first disease, child-sized — same drug, different dose. The failing
    /// tape does everything station A taught, including the adult half-milligram, and the child
    /// survives it; the run still lands under the bar, because the dose was the exam.
    #[test]
    fn station_d3_competent_run_clears_and_the_adult_dose_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-d3.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-d3.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-d3: {:?}", parsed.validate());
        let competent = |adrenaline: &str| {
            vec![
                Step::Do("how much does she weigh?".into()),
                Step::Tick(10.0),
                Step::Do("any known allergies?".into()),
                Step::Tick(10.0),
                Step::Do("what did she eat?".into()),
                Step::Tick(10.0),
                Step::Do(adrenaline.into()), // t=30 — inside five minutes either way
                Step::Tick(10.0),
                Step::Do("oxygen".into()),
                Step::Tick(5.0),
                Step::Do("saline 20 ml/kg".into()),
                Step::Tick(5.0),
                Step::Do("anaphylaxis".into()),
                Step::Tick(5.0),
                Step::Do("admit and watch for the second wave".into()),
                Step::Tick(160.0), // the biphasic watch runs out to a discharge
            ]
        };
        let tape = competent("adrenaline 0.2 mg im — 0.01 per kilo");
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // The same competent run with one number wrong: 0.5 mg into twenty kilos. She lives —
        // and the station is failed, because the harm and both dose marks are gone.
        let adult = competent("adrenaline 0.5 mg im — the adult dose");
        let (s, m, _) = det_for_run(&sce, &adult, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the adult dose must not clear the bar: {s}/{m}");
        assert!(s >= 20, "the adult dose still treats — the run fails on marks, not on death: {s}/{m}");
    }

    /// Station D4 (osce-d4, from embla-cases embla-septic-shock-with-multi-organ-failure-resident):
    /// the sepsis six inside the golden hour, in order — cultures before antibiotics, volume
    /// before noradrenaline, and a door for the pus before the unit. The failing tape is the
    /// reflex done backwards: antibiotics with no cultures, pressors into an empty tank, and
    /// five minutes of shock on dry lines.
    #[test]
    fn station_d4_competent_run_clears_and_the_backwards_reflex_fails() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-d4.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-d4.json")).unwrap();
        let parsed = vitals_sce::Sce::from_json(&sce).unwrap();
        assert!(parsed.validate().is_empty(), "osce-d4: {:?}", parsed.validate());
        let tape = vec![
            Step::Do("ask the niece what happened".into()),
            Step::Tick(10.0),
            Step::Do("feel the skin — perfusion".into()),
            Step::Tick(10.0),
            Step::Do("press the right loin".into()),
            Step::Tick(10.0),
            Step::Do("lactate".into()),
            Step::Tick(10.0),
            Step::Do("two sets of blood cultures".into()),
            Step::Tick(10.0),
            Step::Do("urinalysis".into()),
            Step::Tick(10.0),
            Step::Do("two large-bore lines".into()),
            Step::Tick(10.0),
            Step::Do("warmed crystalloid 30 ml/kg".into()), // through real bore
            Step::Tick(10.0),
            Step::Do("broad-spectrum antibiotics now".into()), // t=80 — cultures already flown
            Step::Tick(10.0),
            Step::Do("noradrenaline".into()), // the tank is full; the pressure holds
            Step::Tick(10.0),
            Step::Do("oxygen".into()),
            Step::Tick(10.0),
            Step::Do("urinary catheter — hourly output".into()),
            Step::Tick(10.0),
            Step::Do("call urology — unblock the kidney".into()),
            Step::Tick(10.0),
            Step::Do("icu bed".into()),
            Step::Tick(5.0),
            Step::Do("septic shock — urosepsis".into()),
            Step::Tick(200.0), // stabilised, sourced, booked — the unit takes her warm
        ];
        let (s, m, rhash) = det_for_run(&sce, &tape, &rubric_json).unwrap();
        assert_eq!((s, m), (40, 40));
        assert_eq!(rhash, vitals_progress::record::rubric_hash(rubric_json.as_bytes()));
        // Backwards: meropenem before any culture (harm), noradrenaline into an empty tank
        // (refused), and waiting. Dry lines at five minutes; the pressure walks down to arrest.
        let backwards = vec![
            Step::Do("meropenem now".into()),
            Step::Tick(10.0),
            Step::Do("noradrenaline".into()),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
            Step::Tick(200.0),
        ];
        let (s, m, _) = det_for_run(&sce, &backwards, &rubric_json).unwrap();
        let r: Rubric = serde_json::from_str(&rubric_json).unwrap();
        let det = DetResult { earned: s, max: m, items: vec![], capped_from: None, penalty: 0 };
        assert!(!det.cleared(&r), "the backwards reflex must not clear the bar: {s}/{m}");
    }

    /// **The two stations that gave a star over a body.** Both of these are real runs, both were
    /// above 70%, and in both of them the patient is dead at the end — because the terminal
    /// outcome was just another item, worth 3 of 40 on one and 5 on the other. See [`death_cap`]
    /// for why that is not a weighting problem.
    ///
    /// The tapes are deliberately *good* runs. That is the finding: neither of them is a
    /// candidate flailing. They take the history, send the bloods, name the diagnosis, and lose
    /// her on one step apiece.
    #[test]
    fn a_run_that_ends_with_the_patient_dead_cannot_reach_the_bar() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let step = |orders: &[&str], tail: usize| {
            let mut t = Vec::new();
            for o in orders {
                t.push(Step::Do((*o).into()));
                t.push(Step::Tick(10.0));
            }
            t.extend(std::iter::repeat_n(Step::Tick(30.0), tail));
            t
        };

        // C2, the asthma ladder: everything right, then diazepam for the "panic" on a tiring
        // chest. She stops breathing. Uncapped this was 32/40 — 80%, two stars.
        let c2 = step(
            &[
                "how often does this happen?", "can you finish a sentence?", "listen to the chest",
                "peak flow — measure it", "oxygen", "salbutamol neb", "prednisolone 40 mg",
                "ipratropium", "peak flow again", "inhaled steroid + action plan",
                "acute asthma exacerbation", "diazepam for the panic", "diazepam for the panic",
                "diazepam for the panic", "diazepam for the panic",
            ],
            15,
        );
        // D4, the sepsis six: cultures, fluids, antibiotics inside the hour, source named, unit
        // booked — and no pressor when the full tank stopped answering. Uncapped this was the
        // audit's 29/40, 72.5%, a star.
        let d4 = step(
            &[
                "ask the niece what happened", "feel the skin — perfusion", "press the right loin",
                "lactate", "two sets of blood cultures", "urinalysis", "two large-bore lines",
                "warmed crystalloid 30 ml/kg", "broad-spectrum antibiotics now", "oxygen",
                "urinary catheter — hourly output", "call urology — unblock the kidney", "icu bed",
                "septic shock — urosepsis",
            ],
            30,
        );

        for (ep, tape, uncapped) in [("osce-c2", c2, 32u16), ("osce-d4", d4, 29u16)] {
            let sce = std::fs::read_to_string(format!("{root}stations/{ep}.sce.json")).unwrap();
            let rubric_json = std::fs::read_to_string(format!("{root}rubrics/{ep}.json")).unwrap();
            let (state, _) = resume(&sce, &tape).unwrap();
            assert_eq!(
                state.outcome().map(Outcome::is_death),
                Some(true),
                "{ep}: this tape is supposed to be the one where she dies"
            );

            let (rubric, det) = sheet_for_run(&sce, &tape, &rubric_json).unwrap();
            assert_eq!(
                det.capped_from,
                Some(uncapped),
                "{ep}: the items no longer add to the number the audit measured"
            );
            assert!(
                !det.cleared(&rubric),
                "{ep}: a dead patient still cleared the bar at {}/{}",
                det.earned,
                det.max
            );
            assert!(
                det.bps() < rubric.pass_bps,
                "{ep}: {}bps is not under a pass bar of {}bps",
                det.bps(),
                rubric.pass_bps
            );
            // Not zeroed: the sheet is the lesson, and everything that was genuinely earned is
            // still on it, item by item, for the candidate to read.
            assert_eq!(det.items_total(), uncapped, "{ep}: the cap ate the mark sheet as well");
            assert!(det.earned > 0, "{ep}: the cap zeroed a run that did most of it right");

            // And the chain carries the capped number, not the raw one — det_for_run is the
            // anchor path and this is the only reason any of it matters.
            let (anchored, _, _) = det_for_run(&sce, &tape, &rubric_json).unwrap();
            assert_eq!(anchored, det.earned, "{ep}: the chain would carry the uncapped score");
        }
    }

    /// The cap is a floor under the pass bar and nothing else: it never touches a run the
    /// patient survived, and it is exactly one point below failing rather than a wipe.
    #[test]
    fn the_death_cap_leaves_a_survivor_alone() {
        // The bar is 70% of 40, so 28 is the first passing score and 27 the highest failing one.
        assert_eq!(death_cap(7_000, 40), 27);
        assert_eq!(death_cap(7_000, 100), 69);
        assert_eq!(death_cap(8_000, 40), 31);
        assert_eq!(death_cap(7_000, 0), 0, "an empty rubric proves nothing either way");

        let r = rubric();
        let full = [
            ev("action", "gave Oxygen face mask", 30.0),
            ev("action", "antibiotic ceftriaxone", 300.0),
            ev("outcome", "WinDischarge", 900.0),
        ];
        for won in [Outcome::WinDischarge, Outcome::WinIcu] {
            let d = score(&full, &r, Some(won));
            assert_eq!(d.earned, 40, "{won:?} was capped");
            assert!(d.capped_from.is_none(), "{won:?} reported a cap it did not take");
        }
        // A run still in progress is not a death either — nothing has happened yet.
        assert!(score(&full, &r, None).capped_from.is_none());
        // And the same events with a body attached fail, having earned every point on the sheet.
        let dead = score(&full, &r, Some(Outcome::DeathArrest));
        assert_eq!((dead.earned, dead.capped_from), (27, Some(40)));
        assert_eq!(dead.items_total(), 40, "the sheet stopped showing what was earned");
        assert!(!dead.cleared(&r));
    }

    /// The mark sheet and the star are the same arithmetic or the debrief is a lie.
    ///
    /// Every published station, marked by the exact anchor path, on two tapes each — the empty
    /// one and one that does a little of everything — and on all of them the sheet the debrief
    /// shows must re-add to the number `det_for_run` hands the chain, item by item. This is the
    /// regression guard for the failure the whole endpoint is one refactor away from: a second
    /// walk over the events that explains a score it does not actually reproduce.
    #[test]
    fn the_sheet_always_re_adds_to_the_det_score() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let mut checked = 0;
        for entry in std::fs::read_dir(format!("{root}rubrics")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let case = path.file_stem().unwrap().to_str().unwrap().to_string();
            let sce_path = if case.starts_with("osce-") {
                format!("{root}stations/{case}.sce.json")
            } else {
                format!("{root}scenarios/{case}.json")
            };
            let sce = std::fs::read_to_string(&sce_path).unwrap();
            let rubric_json = std::fs::read_to_string(&path).unwrap();
            // Nothing at all, and then a scatter of the orders these cases share — enough to
            // earn some items and miss others, which is the only interesting shape of sheet.
            for tape in [
                vec![],
                vec![
                    Step::Do("oxygen".into()),
                    Step::Tick(400.0),
                    Step::Do("adrenaline im".into()),
                    Step::Tick(400.0),
                    Step::Do("12-lead ecg".into()),
                    Step::Tick(400.0),
                ],
            ] {
                let (_, det) = sheet_for_run(&sce, &tape, &rubric_json).unwrap();
                let (s, m, _) = det_for_run(&sce, &tape, &rubric_json).unwrap();
                assert_eq!((det.earned, det.max), (s, m), "{case}: sheet and det disagree");
                // Items earned, minus what the deduction took, is the score. On a rubric with
                // no `no_unindicated` item — and on every run that ordered nothing the case did
                // not need — the penalty is zero and this is the identity it always was.
                assert_eq!(
                    det.items_total(),
                    s.saturating_add(det.penalty),
                    "{case}: the items do not add up to the score"
                );
                assert_eq!(
                    det.penalty,
                    det.items.iter().map(|i| i.penalty).sum::<u16>(),
                    "{case}: the sheet's deductions do not add up to the one that was taken"
                );
                assert_eq!(
                    det.items.iter().map(|i| i.points).fold(0u16, u16::saturating_add),
                    m,
                    "{case}: the item maxima do not add up to the rubric max"
                );
                // Sorting for the reader must not invent or drop a point.
                let sorted = det.by_loss();
                assert_eq!(sorted.len(), det.items.len(), "{case}: by_loss lost an item");
                assert!(
                    sorted.windows(2).all(|w| w[0].lost() >= w[1].lost()),
                    "{case}: the sheet is not ordered by what it cost"
                );
                // Every zero says which kind of zero it was, and every point says it was earned.
                for it in &det.items {
                    assert_eq!(it.earned, it.mark == Mark::Hit, "{case}/{}: mark and points disagree", it.label);
                }
                checked += 1;
            }
        }
        assert!(checked >= 20, "expected every published rubric on both tapes, ran {checked}");
    }

    /// Late is not the same zero as never, and the sheet has to say so — with the numbers.
    ///
    /// Station A pays ten points for adrenaline inside five minutes. Given at 6:00 it earns
    /// nothing, exactly as before; what changes is that the sheet now carries `at` and `within`,
    /// so the debrief can write "given at 6:00 — the window was 5:00" instead of a bare cross,
    /// and a run that never gave it at all still reads as a different mistake.
    #[test]
    fn a_late_drug_reads_as_late_and_a_missing_one_as_missing() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");
        let sce = std::fs::read_to_string(format!("{root}stations/osce-a.sce.json")).unwrap();
        let rubric_json = std::fs::read_to_string(format!("{root}rubrics/osce-a.json")).unwrap();
        let find = |det: &DetResult, needle: &str| -> ItemResult {
            det.items.iter().find(|i| i.label.contains(needle)).expect(needle).clone()
        };

        let late = vec![Step::Tick(360.0), Step::Do("adrenaline im".into()), Step::Tick(200.0)];
        let (_, det) = sheet_for_run(&sce, &late, &rubric_json).unwrap();
        let adr = find(&det, "Adrenaline IM");
        assert_eq!(adr.mark, Mark::Partial, "a drug given late is not a drug never given");
        assert_eq!(adr.earned_points(), 0, "late still earns nothing — the star is unchanged");
        assert_eq!(adr.lost(), 10);
        assert_eq!(adr.within, Some(300.0), "the window the sheet quotes is the rubric's own");
        assert_eq!(adr.at, Some(360.0), "the sheet must quote when it actually happened");
        assert_eq!(adr.kind, "action_by");

        let never = vec![Step::Do("chlorpheniramine".into()), Step::Tick(600.0)];
        let (_, det) = sheet_for_run(&sce, &never, &rubric_json).unwrap();
        let adr = find(&det, "Adrenaline IM");
        assert_eq!(adr.mark, Mark::Miss);
        assert_eq!(adr.at, None);
        // The avoidance check points at the moment the harm fired, so the debrief can time it.
        let window = find(&det, "Never let the adrenaline window close");
        assert_eq!(window.mark, Mark::Miss);
        assert!(window.at.is_some(), "a harm that fired must be timed on the sheet");

        // And the costliest miss is what the reader sees first.
        let (_, det) = sheet_for_run(&sce, &never, &rubric_json).unwrap();
        assert_eq!(det.by_loss()[0].lost(), 10, "the ten-point hole is not at the top of the sheet");
    }

    #[test]
    fn every_rubric_uses_the_canonical_star_bar() {
        // Enforce-equal: a rubric whose pass_bps drifts from the one global bar is a failing test,
        // not a silent bug — so the star a verifier re-derives from the pinned rubric and the star
        // the tally counts against the global cannot disagree.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/rubrics");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let r: Rubric = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(
                r.pass_bps,
                vitals_progress::STAR_PASS_BPS,
                "{:?} pass_bps drifted from the canonical star bar",
                path.file_name().unwrap()
            );
            checked += 1;
        }
        assert!(checked >= 3, "expected the three seeded rubrics at least, found {checked}");
    }
}
