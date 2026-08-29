//! The bell — how an encounter ends when nobody's scenario trigger ends it.
//!
//! A station used to end only when the case's own automaton said so. Two of the twelve
//! (`osce-b2`, `osce-c`) declare no ending edge a candidate can reach by standing still, and a
//! third (`osce-c2`) strands in `responding` if the drug goes in and the peak flow is never
//! repeated. Those runs ran forever with the mark sheet sealed behind `outcome.is_none()`.
//!
//! **What ending must not be.** Freezing the clock and scoring the current state is a cheat
//! code: a candidate watching a patient slide toward arrest presses it one second early, dodges
//! [`vitals_osce::death_cap`] and banks a pass on a patient their management was killing. So
//! ending here is not a freeze. It stops taking input and **runs the encounter on** — the
//! patient does not stop existing because the candidate has stopped acting — and scores whatever
//! that produces.
//!
//! The forward run is written on the tape as ordinary [`Step::Tick`]s of [`BELL_TICK`], which is
//! the cadence the live loop already ticks a station at. It is therefore *literally* the same
//! run the candidate would have got by standing at the bedside doing nothing until the end, and
//! a tape that ends this way is indistinguishable — byte for byte, leaf for leaf — from one that
//! was played out. Nothing marks "the candidate pressed finish", because that is not a fact
//! about the patient and putting it on the tape would make an early finish score differently
//! from a late one.
//!
//! **What bounds it.** An unbounded fast-forward on a case with no ending is the same hang one
//! room along, so the loop stops at the first of:
//!
//!   1. a terminal outcome — the natural conclusion, and the usual one;
//!   2. **nothing is going to happen again**: the clock is past every deadline the scenario
//!      itself can still act on ([`Horizon`]), the encounter has run at least as long as the
//!      station advertises, and a whole quiet window has passed in which no vital, axis, status,
//!      event or beat moved. Time is called on a patient who is exactly as she was left;
//!   3. [`BELL_CEILING_SEC`], a backstop that no case in this repository reaches.
//!
//! The horizon is read out of the scenario rather than guessed, which is why no case file is
//! touched: a `.sce.json`'s sha256 is its identity on chain.

use crate::Step;
use vitals_sce::schema::{Cond, Effect, Sce};
use vitals_sce::{render_beat, SceState};

/// The sim-seconds one bell tick advances.
///
/// The same 2 s the page's own loop sends at a station (`hard` is forced off there), so running
/// the encounter on and letting the candidate stand there produce the identical tape. Pinned
/// here and never taken from the caller: a client that could choose the granularity could choose
/// a favourable one, and transitions are evaluated once per tick.
pub const BELL_TICK: f64 = 2.0;

/// The shortest stretch of nothing-at-all that counts as "nothing more will happen".
pub const QUIET_MIN_SEC: f64 = 30.0;

/// The backstop. Sixty sim-minutes is far past every ending any case in this repository has;
/// it exists so a scenario authored later cannot spin this loop, not because it is ever met.
pub const BELL_CEILING_SEC: f64 = 3600.0;

/// The last moment a scenario can still act on its own clock.
///
/// Every `t_elapsed`/`t_in_state` comparison anywhere in the file is a deadline the case has
/// written for itself, and past the largest of them nothing can fire on time alone — only the
/// continuous dynamics can still move anything, and if they have stopped moving they have hit a
/// floor or their guard is off. That pair of facts is what makes "quiet" mean "finished" rather
/// than "quiet for now", and it is why the horizon is read out of the case instead of being a
/// constant somebody has to keep in step with twelve scenario files.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Horizon {
    /// Largest literal compared against `t_elapsed`.
    pub t_elapsed: f64,
    /// Largest literal compared against `t_in_state`.
    pub t_in_state: f64,
    /// How long quiet has to last. Longer than the longest self-clearing flag the case sets, so
    /// a flag that expires and switches a dynamic back on cannot do it *after* we stopped
    /// watching.
    pub quiet: f64,
}

/// Read the horizon off a scenario.
pub fn horizon(sce: &Sce) -> Horizon {
    let mut h = Horizon { t_elapsed: 0.0, t_in_state: 0.0, quiet: QUIET_MIN_SEC };
    let mut for_sec: f64 = 0.0;
    for st in &sce.states {
        for d in &st.dynamics {
            cond(d.when.as_ref(), &mut h);
        }
        for b in &st.bands {
            cond(Some(&b.when), &mut h);
        }
        for t in &st.transitions {
            cond(Some(&t.when), &mut h);
            effects(&t.doo, &mut h, &mut for_sec);
        }
    }
    for t in &sce.triggers {
        cond(Some(&t.when), &mut h);
        effects(&t.doo, &mut h, &mut for_sec);
    }
    for iv in &sce.interventions {
        effects(&iv.effects, &mut h, &mut for_sec);
    }
    h.quiet = h.quiet.max(for_sec + BELL_TICK);
    h
}

fn cond(c: Option<&Cond>, h: &mut Horizon) {
    let Some(c) = c else { return };
    match c {
        Cond::All { all } => all.iter().for_each(|x| cond(Some(x), h)),
        Cond::Any { any } => any.iter().for_each(|x| cond(Some(x), h)),
        Cond::Not { not } => cond(Some(not), h),
        Cond::Cmp { var, value, .. } => match var.as_str() {
            "t_elapsed" => h.t_elapsed = h.t_elapsed.max(*value),
            "t_in_state" => h.t_in_state = h.t_in_state.max(*value),
            _ => {}
        },
        Cond::Flag { .. } | Cond::InState { .. } | Cond::Done { .. } => {}
    }
}

fn effects(es: &[Effect], h: &mut Horizon, for_sec: &mut f64) {
    for e in es {
        match e {
            Effect::Flag { for_sec: Some(s), .. } => *for_sec = for_sec.max(*s),
            Effect::Branch { branch, els } => {
                for a in branch {
                    cond(Some(&a.iff), h);
                    effects(&a.then, h, for_sec);
                }
                effects(els, h, for_sec);
            }
            _ => {}
        }
    }
}

/// Why the bell stopped ringing. Reported so a caller can say which ending this was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rang {
    /// The scenario reached one of its own terminal outcomes.
    Outcome,
    /// The encounter ran out — she is exactly as she was left, and nothing about her is going
    /// to change.
    TimeCalled,
    /// [`BELL_CEILING_SEC`]. Not reachable by any case in this repository; here so that a case
    /// which one day is cannot hang a server.
    Ceiling,
}

/// Ring the bell on a live machine: run the encounter on to its conclusion.
///
/// `at_least_sec` is the point on the sim clock the encounter must reach before time may be
/// called on it — the station's advertised duration. It is a floor and not a deadline: a case
/// still doing something at that mark goes on until it stops, which is what keeps an arrest that
/// lands after the bell from being dodged by pressing finish before it.
///
/// Returns the ticks to append to the tape and the beats they emitted, in order. The caller owns
/// both: it is appending the ticks that makes this run re-derivable by anyone holding the tape,
/// and there is nothing else to append — no marker says the bell was rung early, because that is
/// a fact about the candidate and not about the patient.
pub fn ring(
    sce_json: &str,
    st: &mut SceState,
    at_least_sec: f64,
) -> Result<(Vec<Step>, Vec<String>, Rang), String> {
    let sce = Sce::from_json(sce_json).map_err(|e| format!("bad SCE: {e}"))?;
    let h = horizon(&sce);
    let mut steps = Vec::new();
    let mut beats = Vec::new();

    loop {
        if st.outcome().is_some() {
            return Ok((steps, beats, Rang::Outcome));
        }
        // Read off the machine rather than counted here, so the answer is about the whole run
        // and not about this call. That is the difference between a finish and a stand-there
        // producing the same tape and producing tapes that differ by one tick.
        if st.t_sec() >= at_least_sec
            && st.t_sec() > h.t_elapsed
            && st.var("t_in_state") > h.t_in_state
            && st.quiet_for() >= h.quiet
        {
            return Ok((steps, beats, Rang::TimeCalled));
        }
        if st.t_sec() >= BELL_CEILING_SEC {
            return Ok((steps, beats, Rang::Ceiling));
        }
        let emitted = st.tick(BELL_TICK);
        steps.push(Step::Tick(BELL_TICK));
        beats.extend(emitted.iter().map(render_beat));
    }
}

/// The same thing from a tape rather than a live machine — the shape a test or a verifier wants.
///
/// Replays `tape` through the one step loop, rings the bell on what it produced, and hands back
/// the whole tape including the ticks the bell added.
pub fn rung(sce_json: &str, tape: &[Step], at_least_sec: f64) -> Result<(Vec<Step>, Rang), String> {
    let (mut st, _) = crate::resume(sce_json, tape)?;
    let (added, _, why) = ring(sce_json, &mut st, at_least_sec)?;
    let mut whole = tape.to_vec();
    whole.extend(added);
    Ok((whole, why))
}
