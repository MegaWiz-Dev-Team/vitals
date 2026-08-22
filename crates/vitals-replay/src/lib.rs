//! Replay an encounter tape against Embla's physiology automaton, and reduce the run to the
//! handful of facts worth anchoring.
//!
//! The engine is not reimplemented here — `embla-engine`'s `sce_runtime` is the same code the
//! game runs. That is the point: a verifier that re-derives an outcome with a *different*
//! implementation proves nothing about the game. One implementation, three call sites.
//!
//! **What is anchored, and what is not.** The vital trajectories are `f64` and the engine's own
//! golden tests compare them with a `1e-6` tolerance, so bit-identical replay across machines is
//! not something to promise. Everything below is *discrete* — an outcome, a harm event, an
//! ordered beat list — and a 1e-6 wobble in diastolic pressure cannot flip any of it. The
//! trajectory is simulated; the outcome is proven.

use embla_engine::encounter::Action;
use embla_engine::sce::Sce;
use embla_engine::sce_runtime::{NarrativeBeat, SceState};
use sha2::{Digest, Sha256};

/// One entry on the tape. Mirrors the engine's own golden-test driver, which is where this
/// format came from — the physiology tests were already a replay format, unintentionally.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Advance the sim clock by `dt` seconds.
    Tick(f64),
    /// The player did something. Text, because that is what the matcher consumes.
    Do(String),
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
}

/// Run a tape and reduce it.
pub fn replay(sce_json: &str, tape: &[Step]) -> Result<Replay, String> {
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
            Step::Do(text) => st.apply(&Action::Prescribe(text.clone())),
        };
        for b in emitted {
            beats.push(render(&b));
        }
    }

    Ok(Replay {
        beats,
        harm_events: st.harm_events.clone(),
        outcome: st.outcome().map(|o| format!("{o:?}")),
        steps: tape.len(),
        sim_seconds,
    })
}

/// Canonical string for a beat. Stable across versions is a promise we are making here, so the
/// rendering is explicit rather than derived from `Debug` on the whole enum.
fn render(b: &NarrativeBeat) -> String {
    match b {
        NarrativeBeat::StatusChanged(s) => format!("status:{s:?}"),
        NarrativeBeat::Threshold(t) => format!("threshold:{t}"),
        NarrativeBeat::Harm(h) => format!("harm:{h}"),
        NarrativeBeat::Terminal(o) => format!("terminal:{o:?}"),
    }
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
