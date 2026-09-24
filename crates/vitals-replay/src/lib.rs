//! Replay an encounter tape against Embla's physiology automaton, and reduce the run to the
//! handful of facts worth anchoring.
//!
//! Built on `vitals-sce`, this repo's own interpreter — which is held to Embla's reference
//! engine by `conformance/ep1-vectors.json` rather than by a shared build. A verifier nobody
//! can build from a fresh clone is a verifier nobody can audit.
//!
//! **What is anchored, and what is not.** The vital trajectories are `f64` and the engine's own
//! golden tests compare them with a `1e-6` tolerance, so bit-identical replay across machines is
//! not something to promise. Everything below is *discrete* — an outcome, a harm event, an
//! ordered beat list — and a 1e-6 wobble in diastolic pressure cannot flip any of it. The
//! trajectory is simulated; the outcome is proven.

// No unsafe, enforced rather than observed. Nothing in the replay engine needs it, and in a codebase whose
// product is verifiability, "the compiler checked every memory access" should be a property a
// stranger can confirm from one line. (vitals-program cannot carry this: Solana's entrypoint!
// macro expands to the unsafe input deserialisation every program has.)
#![forbid(unsafe_code)]
pub mod bell;
pub use bell::{horizon, ring, rung, Horizon, Rang, BELL_CEILING_SEC, BELL_TICK};

use vitals_sce::{render_beat, Sce, SceState};
use sha2::{Digest, Sha256};

/// One entry on the tape. Mirrors the engine's own golden-test driver, which is where this
/// format came from — the physiology tests were already a replay format, unintentionally.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    /// Advance the sim clock by `dt` seconds.
    Tick(f64),
    /// The player did something. Text, because that is what the matcher consumes.
    Do(String),
    /// An order the learner gave, already resolved to the intervention it names.
    ///
    /// The text is kept because it is the only evidence of how the order was phrased, and a
    /// debrief is half about phrasing. The id is what replay uses. Recognition therefore happens
    /// once, while the run is being played, and never again — which is what lets recognition
    /// improve without invalidating a leaf already anchored, and what lets an order arrive in a
    /// language no keyword list covers.
    Act { text: String, id: String },
    /// The player *asked* something.
    ///
    /// Recorded, never applied. History-taking is part of what a run was, so the question
    /// belongs in the tape — but it must not reach the intervention matcher, or asking "did you
    /// take your adrenaline?" would administer adrenaline. And only the question is kept: the
    /// patient's reply comes from a language model, which is why it is nowhere near the hash.
    Ask(String),
    /// Turn a device that is already on to the number the player dialled.
    ///
    /// Not a `Do`. Device text goes through the intervention matcher, and the matcher keys on the
    /// device's own name — so `"o2 set to 6"` re-runs the *oxygen order* and re-attaches at the
    /// scenario's canonical setting. The number the player actually chose has to reach the state
    /// without passing the matcher, or the tape replays to a different machine than the one the
    /// player was looking at.
    Set(String, f64),
    /// Take a device off.
    ///
    /// Same reason, and worse: `"remove o2"` matches the oxygen intervention's own keyword and
    /// puts the mask back on. A tape that says the opposite of what happened is not a record.
    Off(String),
    /// A shock, at the joules the learner dialled.
    ///
    /// Straight at the physiology, past the matcher, for the reason the whole defibrillator
    /// change exists: whether a rhythm can be shocked is a fact the engine holds, and routing the
    /// button through each case's own `interventions` list made it content — so `ep2` branched on
    /// a *state name* and `ep3`, `ep4` and `ep5` declared nothing at all and swallowed the order
    /// whole. One step, one meaning, on every case on the shelf.
    ///
    /// Not a `Do` carrying `"defibrillate 200 j"` either. That text is what a learner types and
    /// what the kit mints, and it is recognised *once* — while the run is being played, exactly
    /// like [`Step::Act`] — so the tape records the shock rather than a phrase a later build
    /// might read differently.
    Shock(f64),
}

/// The reduction of a run: everything discrete, nothing continuous.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    /// Ordered beats, canonically rendered.
    pub beats: Vec<String>,
    pub harm_events: Vec<String>,
    /// `None` means the tape ended before the patient reached a terminal state.
    pub outcome: Option<String>,
    pub steps: usize,
    /// Sim seconds elapsed across the tape.
    pub sim_seconds: f64,
    /// What is still on the patient at the end, and at what number.
    ///
    /// Not hashed into the leaf — it is display state, and the leaf commits to outcomes. It is
    /// here because a reducer that cannot report the kit cannot be checked against the machine
    /// the player was looking at, and that gap is exactly where the tape drifted from the run.
    pub equipment: Vec<(String, Option<f64>)>,
}

/// Run a tape and reduce it, keeping the machine.
///
/// Resuming a saved run and verifying a finished one are the same operation, and they must stay
/// the same operation: the tape drifted from the run once already because device handling existed
/// in two places. There is one step loop, and this is it.
pub fn resume(sce_json: &str, tape: &[Step]) -> Result<(SceState, Replay), String> {
    let sce = Sce::from_json(sce_json).map_err(|e| format!("bad SCE: {e}"))?;
    let mut st = SceState::new(sce);
    let r = step_through(&mut st, tape);
    Ok((st, r))
}

/// Solana's target slot time. Slots are the only clock both the chain and every browser agree on,
/// so the ward's idle time is measured in them rather than in wall time nobody can check.
pub const SLOT_SECONDS: f64 = 0.4;

/// Simulated seconds per real second while nobody is on shift: **one simulated minute per sixty
/// real ones**, set by the founder on 16 ก.ย. ("ผมอยากได้ 1:60").
///
/// Slow on purpose, and slower than the 1:10 it replaces for a reason about the ward rather than
/// about physiology: a patient who drifts slowly is one several strangers can still meet, and beds
/// turn over slowly enough that the queue is not eaten by time simply passing. An hour away costs
/// her a simulated minute.
///
/// Nothing bounds it. A gap is worth exactly what it lasted, so ten real hours is ten simulated
/// minutes and a long weekend is seventy-two — past the arrest of most of the catalogue, which is
/// the ruling and not a side effect of it (see the mortality table in docs/CWF_PLAN.md).
///
/// **A design choice, not a physical constant.** The founder can move it; the number is here, in
/// one place, so moving it is one edit and every browser still derives the same patient.
pub const IDLE_SIM_PER_REAL: f64 = 1.0 / 60.0;

// There is no cap, and there was one until 16 ก.ย. — the history is the design.
//
// It stood at two simulated minutes, set below the fastest untreated arrest in the catalogue
// (`ep5-the-night-the-stars-fell`, 186 s left alone from the start) so that a gap could only ever
// make a patient worse. The argument was about the record: death only inside a shift means every
// death on this ward is attributable to what a key did, or failed to do, while holding her.
//
// The founder removed it the same day, and the reason is about the ward: a place where being
// abandoned is survivable is not one. A patient nobody visits deteriorates as the engine says and
// can arrest and die unattended.
//
// What replaces the cap is not a constant but a duty. The ward's ticker — never a stranger —
// replays idle time for every open bed each minute and, when the engine reaches death, anchors a
// closing shift with an empty tape and the idle span, so the chain says *died, nobody on shift*
// and the next stranger never opens a corpse believing she is alive.
//
// `vitals-web`'s `an_unattended_patient_dies_when_the_engine_says_she_does` walks all sixteen
// cases and holds both ends of the published range.

/// Simulated seconds to advance for a gap of `real` seconds between two shifts.
///
/// Pure, and derived from two numbers the chain records — the block times of the two slots, which
/// are facts about when those blocks were produced rather than a count multiplied by a nominal
/// rate. It was that count: `slots × 0.4 s`, on a devnet producing slots at 0.166 s, which made
/// every gap 2.4× longer than it was and killed unattended patients that much sooner.
///
/// A negative span is a clock disagreeing with itself, never a patient getting younger.
pub fn idle_sim_seconds(real: f64) -> f64 {
    if real <= 0.0 { 0.0 } else { real * IDLE_SIM_PER_REAL }
}

/// **The most a gap may cost the stranger who finally comes: five simulated minutes.**
///
/// Set by the founder on 23 ก.ย. 2026, the morning after the first real stranger ever to take a
/// shift on this ward opened Nadege Toussaint — twelve real hours alone, so twelve simulated
/// minutes into a PSVT case, already past the point it can be treated from. He asked her nine
/// questions across forty seconds, she arrested, and he left without recording anything. A ward
/// whose first shift is always a death teaches exactly one thing, and it is not to come back.
///
/// His ruling: "รักษาหลักการ 'ไม่มีใครมาก็ตาย' ไว้ แต่ให้คนที่มาถึงได้รักษาจริง ไม่ใช่มาดูตาย" — keep the
/// principle that nobody coming means she dies, but whoever does come gets to treat, not to watch
/// a death. Two rules about one gap, both true at once, and they are two functions here so that
/// neither can be quietly used for the other's work:
///
/// * [`idle_sim_seconds`] stays unbounded. It is what the ward's ticker hands the engine when it
///   asks whether a patient has died alone, and being abandoned here has to stay survivable-by-
///   nobody. She gets worse for every hour nobody comes.
/// * This one stops at the cap, and it is what a chart is brought up to the moment somebody
///   arrived. Whoever comes finds her as she would be five minutes in.
///
/// A ceiling, never a rescaling: under it the two agree exactly, so an hour alone still costs her
/// the simulated minute it always did and only the long gaps flatten.
///
/// **This changes what an already-anchored shift re-derives to.** The chain stores tapes and slots,
/// never states, so every state on this ward is recomputed by this code — and a shift anchored
/// after a gap longer than five real hours now rebuilds from a different patient than it was played
/// on, and its leaf will not match. Visible as unrebuildable rather than silently wrong, which is
/// the only mercy available; checked against devnet before this shipped.
///
/// Not a physical constant. It is a promise about what showing up is worth, in one place, so moving
/// it is one edit and every browser still derives the same patient.
pub const ARRIVAL_IDLE_CAP_SIM_SECONDS: f64 = 300.0;

/// Simulated seconds to advance a chart being brought up to the moment somebody arrived at the bed.
///
/// See [`ARRIVAL_IDLE_CAP_SIM_SECONDS`] for why this is not [`idle_sim_seconds`], and why the
/// ticker's death path must never call it.
pub fn idle_sim_seconds_on_arrival(real: f64) -> f64 {
    idle_sim_seconds(real).min(ARRIVAL_IDLE_CAP_SIM_SECONDS)
}

/// Let `seconds` of simulated time pass with nobody in the room.
///
/// **At the scenario's own grain, never in one jump**, and that is the whole function. The engine
/// takes one state edge per tick and evaluates each trigger once per tick, so time delivered as a
/// single large tick walks past edges the same time in ordinary ticks would have taken: ep1 handed
/// an hour as one `tick(3600.0)` comes back with a systolic of 0, a saturation of 0 and **no
/// outcome at all** — a corpse the chart calls alive — while an hour of one-second ticks arrests
/// her at 518 s. Two physiologies, one for time somebody watched and one for time nobody did, and
/// every promise on the ward rests on there being only one.
///
/// Mechanism, not policy: how much time passes is [`idle_seconds`]. This is public so the grain
/// rule can be tested at its own level and stay pinned to something no constant can move — the
/// ratio and the cap were both changed in a single day, and "there is one physiology" has to
/// survive that.
pub fn pass_idle(st: &mut SceState, seconds: f64) {
    let grain = st.tick_seconds().max(f64::MIN_POSITIVE);
    let mut left = seconds;
    while left > 0.0 {
        let dt = grain.min(left);
        st.tick(dt);
        left -= dt;
    }
}

/// Continue a stay: one shift's tape, run on the machine the last shift left behind.
///
/// This is how the ward hands a patient from one stranger to the next (CWF_PLAN.md). The state
/// comes from [`resume`] over everything anchored so far, and this runs the new shift on top of
/// it, so shift N+1 starts where shift N stopped rather than where the scenario starts.
///
/// `idle_slots` is the gap since the last anchored shift. Nobody was watching her during it, but
/// her body was still hers: [`idle_seconds`] turns the gap into simulated time and the machine
/// ticks through it before the shift's first step. A patient nobody visits can deteriorate, and
/// with a long enough gap she can die — that is a ward, and it stays verifiable because the gap is
/// on chain and the ratio is a constant in this file.
///
/// **The [`Replay`] returned is the shift's own, not the stay's.** Its beats, its seconds, and —
/// the one that needs saying — its harm: the machine accumulates `harm_events` across the whole
/// stay, so this reports only the ones this tape added. A stranger is scored on what they did, not
/// on what they walked into. `outcome` is the stay's, because an outcome is a fact about the
/// patient and the shift that reaches it is the shift that reached it.
pub fn shift(st: &mut SceState, tape: &[Step], idle_real_seconds: f64) -> Replay {
    // Her body first, then the shift. The idle time is the gap the chain records between the last
    // anchored shift and this one, so it is not ours to choose at play time — and it is applied
    // here, as the first thing, because a stranger's first action must land on the patient they
    // are actually looking at.
    //
    pass_idle(st, idle_sim_seconds(idle_real_seconds));
    step_through(st, tape)
}

/// The one step loop.
///
/// Resuming a saved run, verifying a finished one and continuing a stay are the same operation and
/// they must stay the same operation: the tape drifted from the run once already because device
/// handling existed in two places. Everything above calls this and nothing reimplements it.
fn step_through(st: &mut SceState, tape: &[Step]) -> Replay {
    let harm_before = st.harm_events.len();
    let mut beats = Vec::new();
    let mut sim_seconds = 0.0;

    for step in tape {
        let emitted = match step {
            Step::Tick(dt) => {
                sim_seconds += dt;
                st.tick(*dt)
            }
            Step::Do(text) => st.apply(text),
            Step::Act { id, .. } => st.apply_id(id),
            // Deliberately inert. Asking costs time — which the surrounding Tick steps carry —
            // and reveals information, but it changes nothing about the patient.
            Step::Ask(_) => Vec::new(),
            // Straight at the state, never at the matcher. Neither emits a narrative beat — the
            // equipment timeline records them, and beats come only from orders and from time.
            Step::Set(id, v) => {
                st.attach(id, Some(*v));
                Vec::new()
            }
            Step::Off(id) => {
                st.detach(id);
                Vec::new()
            }
            // The one step that reaches the physiology without asking the scenario anything.
            Step::Shock(joules) => st.defibrillate(*joules).1,
        };
        for b in emitted {
            beats.push(render_beat(&b));
        }
    }

    Replay {
        beats,
        harm_events: st.harm_events[harm_before..].to_vec(),
        outcome: st.outcome().map(|o| format!("{o:?}")),
        steps: tape.len(),
        sim_seconds,
        equipment: st.equipment().iter().map(|e| (e.id.clone(), e.setting)).collect(),
    }
}

/// Run a tape and reduce it. The verifier's view: the machine is scaffolding, the reduction is
/// the answer.
pub fn replay(sce_json: &str, tape: &[Step]) -> Result<Replay, String> {
    resume(sce_json, tape).map(|(_, r)| r)
}

/// sha256 of the scenario definition. Pinning this is what stops a rewritten scenario from
/// silently revaluing every credential ever issued against it.
pub fn sce_hash(sce_json: &str) -> [u8; 32] {
    Sha256::digest(sce_json.as_bytes()).into()
}

/// The leaf. Canonical, newline-delimited, length-prefixed per section so no two different runs
/// can serialise to the same bytes by rearranging fields.
impl Step {
    /// Record an order the learner gave.
    ///
    /// Canonicalises on the way in, so the tape — and therefore the leaf built from it — holds
    /// one form of the text regardless of which keyboard produced it. Constructing `Step::Do`
    /// directly skips this; that is left possible on purpose, because replaying an old tape has
    /// to reproduce exactly the bytes it was anchored with, canonical or not.
    pub fn did(text: &str) -> Step {
        Step::Do(vitals_sce::text::canon(text))
    }

    /// Record an order together with what recognition resolved it to.
    pub fn acted(text: &str, id: &str) -> Step {
        Step::Act { text: vitals_sce::text::canon(text), id: id.to_string() }
    }

    /// Record a question. Hashed into the leaf like an order, so canonicalised like one.
    pub fn asked(text: &str) -> Step {
        Step::Ask(vitals_sce::text::canon(text))
    }
}

pub fn leaf(sce_hash: &[u8; 32], tape: &[Step], r: &Replay) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"vitals.leaf.v1\n");
    h.update(sce_hash);

    h.update(format!("\ntape:{}\n", tape.len()));
    for s in tape {
        match s {
            // Ticks are quantised to milliseconds: the tape records intent, and a float that
            // round-trips differently must not change the leaf.
            Step::Tick(dt) => h.update(format!("t{}\n", (dt * 1000.0).round() as i64)),
            Step::Do(text) => h.update(format!("d{text}\n")),
            // A distinct prefix, so a tape that resolves nothing hashes byte for byte as it did
            // before resolution existed and every leaf already anchored still verifies. The unit
            // separator cannot occur in learner text, so no order can spell itself into another.
            Step::Act { text, id } => h.update(format!("D{text}\x1f{id}\n")),
            // A tape with no questions hashes exactly as it did before questions existed, so
            // every leaf anchored under the older encoding still verifies.
            Step::Ask(text) => h.update(format!("a{text}\n")),
            // Same bargain the questions struck: a tape that never touches a dial hashes exactly
            // as it did before dials were on the tape, so leaves anchored earlier still verify.
            Step::Set(id, v) => h.update(format!("s{id}={}\n", (v * 1000.0).round() as i64)),
            Step::Off(id) => h.update(format!("x{id}\n")),
            // The fourth prefix added under the same bargain as `D`, `a` and `s` before it: a
            // tape with no shock on it produces exactly the bytes it produced before shocks
            // existed, so every leaf already anchored still verifies. Quantised to millijoules
            // like every other float on the tape, for the same reason — the tape records the
            // number the learner dialled, and a float that round-trips differently must not
            // change the leaf. `tests/shock_tape.rs` is what holds that claim up.
            Step::Shock(joules) => h.update(format!("j{}\n", (joules * 1000.0).round() as i64)),
        }
    }

    h.update(format!("beats:{}\n", r.beats.len()));
    for b in &r.beats {
        h.update(format!("{b}\n"));
    }
    h.update(format!("harm:{}\n", r.harm_events.len()));
    for e in &r.harm_events {
        h.update(format!("{e}\n"));
    }
    h.update(format!("outcome:{}\n", r.outcome.as_deref().unwrap_or("-")));
    h.finalize().into()
}

pub fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Reduce a replay to the record that gets anchored.
///
/// `case` is the scenario hash by default: three runs of one patient are one case, and the
/// competency model is entitled to know that. A host that authors many cases against one
/// scenario passes its own case id instead.
// Nine arguments, and clippy is right that it is a lot. They are nine independent facts about
// one attempt — who, which engine, which case, how hard, which mode, what happened, and what was
// declared beforehand — none optional, none derivable from another here. A params struct would
// spread the same nine names over nine lines and add a type whose only job is to be built once,
// immediately, by both callers. If a tenth fact appears, that is the moment to bundle.
#[allow(clippy::too_many_arguments)]
pub fn record_for(
    player: [u8; 32],
    sce_hash: [u8; 32],
    case: [u8; 32],
    difficulty: vitals_progress::Difficulty,
    exam_mode: bool,
    tape: &[Step],
    r: &Replay,
    commitment: [u8; 32],
    committed_slot: u64,
) -> Result<vitals_progress::record::AttemptRecord, String> {
    let outcome = match r.outcome.as_deref() {
        None => vitals_progress::record::Outcome::NoTerminal,
        Some(s) => vitals_progress::record::Outcome::parse(s)
            .ok_or_else(|| format!("unknown outcome {s:?} — this build cannot score it"))?,
    };
    let mut rec = vitals_progress::record::AttemptRecord {
        player,
        sce_hash,
        case,
        run_hash: leaf(&sce_hash, tape, r),
        difficulty,
        exam_mode,
        outcome,
        harm_count: r.harm_events.len() as u16,

        // Passed in rather than computed here: this crate replays a tape and has no idea what was
        // committed before the run started. The caller that made the commitment supplies it, and
        // the program checks it against the commitment account rather than believing anyone.
        commitment,
        committed_slot,

        // A story-mode run is deterministic end to end. Its whole score is re-derivable by anyone
        // who replays this tape against the pinned engine, so it goes in `det_*` and the judged
        // half is zero — not missing, but the record stating that no part of it rested on a
        // witness. There is no rubric either: the outcome comes from the physiology.
        rubric_hash: [0u8; 32],
        det_score: 0,
        det_max: 0,
        judged_score: 0,
        judged_max: 0,
    };
    // Filled after construction because the score is derived from the outcome and the harm count
    // that were just set. Saturating rather than truncating: a score that wrapped to a small
    // number in a record meant to be trusted later is the worst possible failure of this field.
    rec.det_score = rec.score().min(u16::MAX as u32) as u16;
    rec.det_max = rec.max_score().min(u16::MAX as u32) as u16;
    Ok(rec)
}

// ── the debrief ─────────────────────────────────────────────────────────────
//
// Vitals could grade but not teach. A finished case gave an outcome, a score and a hash, and said
// nothing about *why* — not that adrenaline came four minutes late, not which order caused the
// harm, not how long she spent in arrest. The score is the verdict; this is the reasoning.
//
// Every line below is a time or an ordering derived from the tape, so a verifier holding the same
// two inputs re-derives the same debrief. Nothing here is an opinion and nothing needs a model.
// The targets it measures against are clinical judgement and live in the scenario file.

/// One thing the scenario expected, and what actually happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Expectation {
    pub id: String,
    pub label: String,
    pub why: String,
    /// The target, in seconds from the start of the case. `None` means it matters that it
    /// happened, not when.
    pub within: Option<f64>,
    /// When it was first done. `None` means never.
    pub done_at: Option<f64>,
    /// Done, but past the target. Never late when it was never done — that is a different failure
    /// and it reads differently.
    pub late: bool,
    pub late_by: Option<f64>,
}

/// Something that hurt her, and the order it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct HarmAt {
    pub text: String,
    pub at: f64,
    /// The order recorded immediately before it. `None` when the harm came from the passage of
    /// time rather than from anything the player did.
    pub caused_by: Option<String>,
}

/// How long she spent in one clinical state.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub status: String,
    pub from: f64,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Debrief {
    pub outcome: Option<String>,
    pub sim_seconds: f64,
    pub expected: Vec<Expectation>,
    /// Orders the scenario says not to give, that were given anyway.
    pub avoided: Vec<Expectation>,
    pub harms: Vec<HarmAt>,
    pub statuses: Vec<Span>,
}

/// Replay a tape and say what it did, against what the scenario asked for.
pub fn debrief(sce_json: &str, tape: &[Step]) -> Result<Debrief, String> {
    let sce = Sce::from_json(sce_json).map_err(|e| format!("bad SCE: {e}"))?;
    let spec = sce.debrief.clone();
    let (st, r) = resume(sce_json, tape)?;
    let events = st.events();

    // First time each intervention was ordered. The automaton records orders by id, so this is
    // an exact match rather than a guess at what the learner meant.
    let first_at = |id: &str| -> Option<f64> {
        events.iter().find(|e| e.kind == "action" && e.text == id).map(|e| e.t_sec)
    };

    let build = |e: &vitals_sce::Expect| {
        let done_at = first_at(&e.id);
        let (late, late_by) = match (e.within_sec, done_at) {
            (Some(target), Some(t)) if t > target => (true, Some(t - target)),
            _ => (false, None),
        };
        Expectation {
            id: e.id.clone(),
            label: e.label.clone().unwrap_or_else(|| e.id.clone()),
            why: e.why.clone().unwrap_or_default(),
            within: e.within_sec,
            done_at,
            late,
            late_by,
        }
    };

    let (expected, avoided) = match &spec {
        Some(s) => (
            s.expect.iter().map(&build).collect(),
            s.avoid.iter().map(&build).collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };

    // Harm, blamed on the order recorded immediately before it. The automaton records the order
    // first and then the harm it caused, so "immediately before" is the cause and not a guess.
    //
    // A shock counts as an order here even though it is not an `action` — `vitals_sce::SHOCK`
    // has a kind of its own so that no rubric silently starts marking it, and that is a fact
    // about scoring, not about causation. Left out, a wrong shock's harm would be blamed on
    // whatever the candidate happened to order in the same second, which is a debrief naming
    // the wrong mistake.
    let cause = |k: &str| k == "action" || k == vitals_sce::runtime::SHOCK;
    let mut harms = Vec::new();
    for (i, e) in events.iter().enumerate() {
        if e.kind != "harm" {
            continue;
        }
        let caused_by = events[..i]
            .iter()
            .rev()
            .find(|p| cause(&p.kind))
            .filter(|p| (e.t_sec - p.t_sec).abs() < 1e-6)
            .map(|p| p.text.clone());
        harms.push(HarmAt { text: e.text.clone(), at: e.t_sec, caused_by });
    }

    // How long each state lasted. The last one runs to the end of the tape.
    let marks: Vec<(&str, f64)> = events
        .iter()
        .filter(|e| e.kind == "status")
        .map(|e| (e.text.as_str(), e.t_sec))
        .collect();
    let mut statuses = Vec::new();
    for (i, (name, from)) in marks.iter().enumerate() {
        let until = marks.get(i + 1).map(|(_, t)| *t).unwrap_or(r.sim_seconds);
        statuses.push(Span { status: (*name).to_string(), from: *from, seconds: (until - from).max(0.0) });
    }

    Ok(Debrief {
        outcome: r.outcome.clone(),
        sim_seconds: r.sim_seconds,
        expected,
        avoided,
        harms,
        statuses,
    })
}

/// **How much longer this patient has, if nobody comes.**
///
/// Simulated seconds from her state now until the engine reaches an ending with nobody treating
/// her — which is the question the ward's own ticker answers every minute, asked one step earlier.
/// `None` when she reaches no ending inside `limit`: a patient the case does not kill untreated,
/// or one so far out that a countdown would be a fiction.
///
/// **It does not advance her.** The state is cloned, because the board asks this about every open
/// bed on every build and a question that changed its subject would be the ward killing patients by
/// looking at them.
///
/// Walked at the scenario's own grain through [`pass_idle`] rather than in one jump, for the reason
/// that function gives: an hour delivered as a single tick walks past the edges an hour of ordinary
/// ticks would have crossed, and produces a corpse the chart calls alive. The answer has to come
/// from the same physiology the ticker uses or it is a different patient's clock.
///
/// Real time is the caller's to compute and the conversion is [`IDLE_SIM_PER_REAL`]: at one
/// simulated minute per sixty real ones, a simulated second is a real minute, so simulated seconds
/// divided by sixty is hours.
pub fn sim_seconds_until_untreated_ending(st: &SceState, limit: f64) -> Option<f64> {
    if st.outcome().is_some() {
        return Some(0.0);
    }
    let mut ahead = st.clone();
    // A minute of simulated time per step: fine enough that the answer is never more than a minute
    // out — under a real hour at 1:60 — and coarse enough that forty beds cost forty thousand ticks
    // rather than two and a half million.
    const STEP: f64 = 60.0;
    let mut spent = 0.0;
    while spent < limit {
        pass_idle(&mut ahead, STEP);
        spent += STEP;
        if ahead.outcome().is_some() {
            return Some(spent);
        }
    }
    None
}
