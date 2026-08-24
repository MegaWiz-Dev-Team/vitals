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
        };
        for b in emitted {
            beats.push(render_beat(&b));
        }
    }

    let r = Replay {
        beats,
        harm_events: st.harm_events.clone(),
        outcome: st.outcome().map(|o| format!("{o:?}")),
        steps: tape.len(),
        sim_seconds,
        equipment: st.equipment().iter().map(|e| (e.id.clone(), e.setting)).collect(),
    };
    Ok((st, r))
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
    let mut harms = Vec::new();
    for (i, e) in events.iter().enumerate() {
        if e.kind != "harm" {
            continue;
        }
        let caused_by = events[..i]
            .iter()
            .rev()
            .find(|p| p.kind == "action")
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
