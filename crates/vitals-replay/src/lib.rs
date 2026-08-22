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
pub fn record_for(
    player: [u8; 32],
    sce_hash: [u8; 32],
    case: [u8; 32],
    difficulty: vitals_progress::Difficulty,
    exam_mode: bool,
    tape: &[Step],
    r: &Replay,
) -> Result<vitals_progress::record::AttemptRecord, String> {
    let outcome = match r.outcome.as_deref() {
        None => vitals_progress::record::Outcome::NoTerminal,
        Some(s) => vitals_progress::record::Outcome::parse(s)
            .ok_or_else(|| format!("unknown outcome {s:?} — this build cannot score it"))?,
    };
    Ok(vitals_progress::record::AttemptRecord {
        player,
        sce_hash,
        case,
        run_hash: leaf(&sce_hash, tape, r),
        difficulty,
        exam_mode,
        outcome,
        harm_count: r.harm_events.len() as u16,
    })
}
