//! The case factory: `embla-cases` in, ward packs out.
//!
//! The ward — Vitals World — does not run the season's content. Its patients come from the
//! embla-cases library, compiled into the engine's scenario format by this crate: a deterministic
//! tool, never a model, so the same case at the same hash compiles to the same pack on any
//! machine. Every pack it writes has been parsed by `vitals-sce`, replayed by `vitals-replay`'s
//! engine untreated to a death and along its own management path to a win, and marked by
//! `vitals-osce` — and is still `provisional: true` until a clinician has read it.
//!
//! A case the archetype library cannot honestly model is **refused with a reason**, never forced.
#![forbid(unsafe_code)]

pub mod acls;
pub mod archetype;
pub mod embla;
pub mod interventions;
pub mod plan;
pub mod report;
pub mod rubric;
pub mod scenario;
pub mod source;
pub mod text;
pub mod validate;

use archetype::{Archetype, Kind};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Where a case came from, pinned by content.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Source {
    pub repo: String,
    /// The git ref (or `worktree`) the case was read at.
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// SHA-256 of the exact `case.json` bytes that were compiled.
    pub sha256: String,
}

impl Source {
    pub fn of(repo: &str, git_ref: &str, case_json: &str) -> Source {
        let h = Sha256::digest(case_json.as_bytes());
        Source { repo: repo.to_string(), git_ref: git_ref.to_string(), sha256: h.iter().map(|b| format!("{b:02x}")).collect() }
    }
}

/// Why a case was not compiled. Always carries the case id so a report can list it.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub case_id: String,
    pub reason: String,
}

/// The compiler's own name and version, stamped on every pack.
#[derive(Debug, Clone, Serialize)]
pub struct Compiler {
    pub name: &'static str,
    pub version: &'static str,
}

/// A time a sentence pinned to a role, as the pack shows the reviewer.
#[derive(Debug, Clone, Serialize)]
pub struct TimedRole {
    /// Seconds the sentence named.
    pub named_sec: f64,
    /// Seconds the rubric holds the order to (inside the shift, at the latest when the patient
    /// turns critical).
    pub by_sec: f64,
    pub sentence: String,
}

/// One case, compiled: everything the ward's door needs to admit the patient.
#[derive(Debug, Clone, Serialize)]
pub struct Pack {
    pub case_id: String,
    pub source: Source,
    pub title: String,
    /// ISO 3166-1 alpha-3, when the case carries one.
    pub country: Option<String>,
    /// `student` | `intern` | `resident` — the ward's three levels, as the case declares.
    pub difficulty: String,
    pub clinical_tier: Option<u8>,
    pub specialty: Option<String>,
    pub care_setting: Option<String>,
    pub language: Option<String>,
    pub tags: Vec<String>,
    /// True when the case is tagged `endemic` — drawn from a country's list, never the common draw.
    pub endemic: bool,
    /// Always true from this compiler. Cleared by a clinician, never by code.
    pub provisional: bool,
    /// The case's own version string.
    pub version: String,
    /// Which archetype compiled it.
    pub archetype: String,
    pub archetype_label: String,
    /// The patient at presentation, without the name: the ward assigns its own persona.
    pub patient: PackPatient,
    /// The chief complaint, the history and the room, verbatim from the case — the opening the
    /// ward's voice speaks from. Scanned for the name like everything else.
    pub presentation: embla::Presentation,
    /// The full scenario, as `vitals-sce` reads it.
    pub sce: serde_json::Value,
    /// The mark sheet, as `vitals-osce` reads it.
    pub rubric: serde_json::Value,
    /// The patient's words, keyed by the `ask_` intervention that draws them out.
    pub voice: BTreeMap<String, interventions::VoiceLine>,
    /// The plan, step by step, with what each compiled into.
    pub management: Vec<plan::PlanStep>,
    /// The roles a sentence pinned to the clock.
    pub timed: BTreeMap<String, TimedRole>,
    /// Vital signs the case did not give, filled with resting defaults.
    pub vitals_assumed: Vec<String>,
    /// What the replays proved.
    pub replay: validate::Proof,
    pub compiler: Compiler,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackPatient {
    pub age: Option<u32>,
    pub sex: Option<String>,
}

pub const COMPILER: Compiler = Compiler { name: "vitals-casefactory", version: env!("CARGO_PKG_VERSION") };

fn refuse(id: &str, reason: impl Into<String>) -> Refusal {
    Refusal { case_id: id.to_string(), reason: reason.into() }
}

/// Compile one case. `Ok` is a pack that passed every gate; `Err` says why it did not.
pub fn compile(case_json: &str, source: Source) -> Result<Pack, Refusal> {
    let case = embla::parse_case(case_json).map_err(|e| refuse("?", e))?;
    let id = case.meta.id.clone();

    let difficulty = match case.meta.difficulty.as_deref().map(str::to_lowercase).as_deref() {
        Some("student") => "student",
        Some("intern") => "intern",
        Some("resident") => "resident",
        other => return Err(refuse(&id, format!("difficulty {other:?} is not one the ward serves (student|intern|resident)"))),
    };
    if case.hidden.management_plan.is_empty() {
        return Err(refuse(&id, "no management_plan — nothing to compile into interventions"));
    }

    // Words first, then vitals: a clinic case with no blood pressure is refused as the stable
    // presentation it is, not for the number it never needed.
    if let Some(why) = Archetype::not_yet(&case) {
        return Err(refuse(&id, why));
    }
    if Archetype::candidates(&case).is_empty() {
        return Err(refuse(&id, Archetype::none_fits(&case)));
    }
    // A patient without a pulse has no vitals to read; the arrest archetype starts from zeros.
    let v0 = match case.vitals0() {
        Ok(v) => v,
        Err(e) => {
            let arrest_words = Archetype::candidates(&case).first().is_some_and(|(_, a)| *a == Archetype::AclsCardiacArrest);
            if arrest_words { embla::Vitals0::arrest() } else { return Err(refuse(&id, e)) }
        }
    };
    let a = Archetype::detect(&case, &v0).map_err(|e| refuse(&id, e))?;
    let mapped = plan::map(&case, a);
    // An arrest is turned by the algorithm's own tools, which every arrest case carries.
    if mapped.critical().is_empty() && a != Archetype::AclsCardiacArrest {
        return Err(refuse(&id, format!("the plan names no therapy the {} archetype can act on — nothing turns the trajectory", a.id())));
    }
    // Oxygen alone does not turn respiratory failure; the plan has to name what does.
    if a.oxygen_is_critical() && mapped.critical().iter().all(|p| p.role.id == "oxygen") {
        return Err(refuse(&id, format!("the plan names no therapy the {} archetype can act on beyond oxygen — nothing turns the trajectory", a.id())));
    }

    let built = interventions::build(&case, &mapped, a);
    let sim = scenario::build(&case, a, &v0, &mapped, &built);
    let rubric = rubric::derive(&case, a, &mapped, &built, &sim, &source.sha256);

    // the management path: gates, then every critical order in the plan's order, twenty
    // seconds apart; then everything else the rubric pays for — the supportive orders, the
    // history, the examination, the workup, the diagnosis — spread across the recovery window,
    // so the whole path lands before the win is declared and the golden score is the sheet's
    // full reading of it
    let mut path: Vec<validate::PathStep> = Vec::new();
    let mut t = 20.0;
    let last_critical = match acls::golden_prefix(&case, a, &v0, &mapped) {
        Some(prefix) => {
            let last = prefix.last().map(|(t, _)| *t).unwrap_or(20.0);
            path.extend(prefix.into_iter().map(|(t_sec, id)| validate::PathStep { t_sec, id }));
            last
        }
        None => {
            for kind in [Kind::Gate, Kind::Critical] {
                for p in mapped.present.iter().filter(|p| p.role.kind == kind) {
                    path.push(validate::PathStep { t_sec: t, id: p.tx_id() });
                    t += 20.0;
                }
            }
            t - 20.0
        }
    };
    let on_path: Vec<String> = path.iter().map(|p| p.id.clone()).collect();
    let mut rest: Vec<String> = mapped.present.iter().filter(|p| p.role.kind == Kind::Supportive).map(plan::Present::tx_id).filter(|id| !on_path.contains(id)).collect();
    let paid: Vec<String> = rubric["items"]
        .as_array()
        .map(|items| items.iter().filter_map(|it| it.get("needle").and_then(|n| n.as_str()).map(str::to_string)).collect())
        .unwrap_or_default();
    for pre in ["ask_", "exam_", "ix_", "dx_"] {
        for n in paid.iter().filter(|n| n.starts_with(pre)) {
            rest.push(n.clone());
        }
    }
    // an examination the rubric pays for as "any of" still has to happen on the path
    if let Some(items) = rubric["items"].as_array() {
        for it in items.iter().filter(|it| it["type"] == "action_any") {
            if let Some(first) = it["any_of"].as_array().and_then(|a| a.first()).and_then(|v| v.as_str()) {
                rest.push(first.to_string());
            }
        }
    }
    let window = a.recovery_sec() - 40.0;
    let spacing = if rest.is_empty() { 0.0 } else { (window / rest.len() as f64).clamp(1.0, 20.0).floor() };
    let mut t = last_critical + 10.0;
    for id in rest {
        path.push(validate::PathStep { t_sec: t, id });
        t += spacing;
    }

    let timed: BTreeMap<String, TimedRole> = mapped
        .timed
        .iter()
        .map(|(k, v)| (k.clone(), TimedRole { named_sec: v.named_sec, by_sec: scenario::by_sec(a, v.named_sec), sentence: v.sentence.clone() }))
        .collect();

    let endemic = case.meta.search_tags.iter().any(|t| t.eq_ignore_ascii_case("endemic"));

    // a first pack without the proof, so the scans see every string the final one will carry
    let proof_placeholder = validate::Proof {
        untreated_death_sec: 0.0,
        win_path: path.clone(),
        win_sec: 0.0,
        win_outcome: sim.win.to_string(),
        golden_score: validate::GoldenScore { earned: 0, max: 0, pass_bps: 0 },
    };
    let mut pack = Pack {
        case_id: id.clone(),
        source,
        title: case.meta.title.clone(),
        country: case.meta.country.clone().filter(|c| c.len() == 3),
        difficulty: difficulty.to_string(),
        clinical_tier: case.meta.clinical_tier,
        specialty: case.meta.specialty.clone(),
        care_setting: case.meta.care_setting.clone(),
        language: case.meta.language.clone(),
        tags: case.meta.search_tags.clone(),
        endemic,
        provisional: true,
        version: case.meta.version.clone().unwrap_or_else(|| "0.0.0".into()),
        archetype: a.id().to_string(),
        archetype_label: a.label().to_string(),
        patient: PackPatient { age: case.patient.age, sex: case.sex() },
        presentation: embla::Presentation {
            chief_complaint: text::scrub(&case.presentation.chief_complaint, &case.patient.name),
            hpi: text::scrub(&case.presentation.hpi, &case.patient.name),
            setting: case.presentation.setting.as_deref().map(|s| text::scrub(s, &case.patient.name)),
        },
        sce: sim.sce.clone(),
        rubric: rubric.clone(),
        voice: built.voice.clone(),
        management: mapped.steps.clone(),
        timed,
        vitals_assumed: v0.assumed.clone(),
        replay: proof_placeholder,
        compiler: COMPILER,
    };

    let as_json = serde_json::to_value(&pack).map_err(|e| refuse(&id, format!("pack does not serialise: {e}")))?;
    let proof = validate::validate(&as_json, &sim.sce.to_string(), &rubric.to_string(), a, &case.patient.name, &path)
        .map_err(|e| refuse(&id, e))?;
    pack.replay = proof;
    Ok(pack)
}
