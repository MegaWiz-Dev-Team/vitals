//! The SCE interpreter — an independent implementation of the Vitals replay semantics.
//!
//! Runs a [`crate::schema::Sce`] as a hybrid automaton: continuous per-second vital DYNAMICS
//! inside the current STATE, global TRIGGERS, discrete TRANSITIONS, and free-text INTERVENTIONS.
//! Emits [`NarrativeBeat`]s, accumulates harm events, and terminates in an [`Outcome`].
//!
//! **Why this exists separately.** Embla ships the reference implementation of the same
//! semantics. A protocol whose only implementation lives inside one private company's crate is
//! not a protocol, and a verifier that cannot be built from a fresh clone cannot be audited.
//! So the guarantee comes from `conformance/ep1-vectors.json` — frozen from the reference
//! engine, reproduced exactly here — rather than from sharing a binary. Independent
//! implementations that agree are stronger evidence than one implementation everyone trusts.
//!
//! Dependencies are serde and the standard collections. Nothing here needs a database, an HTTP
//! server, or a network, so it compiles to wasm for the public verify page.

use crate::schema::{Cond, Effect, Op, Sce};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

// ── Shared physiology value types ───────────────────────────────────────────
// Moved here from the retired hardcoded engine (`physiology.rs`). These are the
// monitor/Director interface: a vitals snapshot, coarse clinical status, the
// narrative beats the Director keys cutscenes on, and the terminal outcome. The
// Director (`story.rs`) and the Story-Mode HUD (`web.rs`) consume them unchanged.

/// Live vital signs — the standard patient-monitor vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vitals {
    pub hr: f64,
    pub sbp: f64,
    pub dbp: f64,
    pub spo2: f64,
    pub rr: f64,
    pub temp: f64,
    pub gcs: u8,
    /// What the ECG is actually doing. Separate from `hr` because the two can disagree, and the
    /// disagreement is the entire point: in PEA there are complexes marching along at a countable
    /// rate and no cardiac output at all.
    pub rhythm: Rhythm,
}

/// The cardiac rhythm — scenario content, declared per state, never guessed from the heart rate.
///
/// It exists so shockability is a fact the engine holds rather than something a display infers.
/// A monitor that draws VF because the status happens to read "arrest" will happily draw VF for an
/// arrest that is not VF, and then teach the learner to shock it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rhythm {
    /// Sinus (or any perfusing rhythm) — rate carried by `hr`.
    Sinus,
    /// Ventricular fibrillation. Shockable.
    Vf,
    /// Pulseless ventricular tachycardia. Shockable.
    Vt,
    /// Pulseless electrical activity — organised complexes, no output. NOT shockable.
    Pea,
    /// Asystole. NOT shockable.
    Asystole,
}

impl Rhythm {
    /// Whether a defibrillator can do anything for it. VF and pulseless VT only.
    ///
    /// This is the whole clinical reason `rhythm` exists. Shocking PEA or asystole is not merely
    /// useless — it stops compressions and delays the adrenaline that is the actual treatment.
    pub fn shockable(self) -> bool { matches!(self, Rhythm::Vf | Rhythm::Vt) }

    /// Whether the heart is producing a pulse. False for every arrest rhythm, including PEA where
    /// the ECG looks reassuring.
    pub fn perfusing(self) -> bool { matches!(self, Rhythm::Sinus) }

    pub fn as_str(self) -> &'static str {
        match self {
            Rhythm::Sinus => "sinus",
            Rhythm::Vf => "vf",
            Rhythm::Vt => "vt",
            Rhythm::Pea => "pea",
            Rhythm::Asystole => "asystole",
        }
    }

    pub fn parse(s: &str) -> Option<Rhythm> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sinus" | "sr" | "nsr" => Some(Rhythm::Sinus),
            "vf" | "vfib" => Some(Rhythm::Vf),
            "vt" | "pvt" => Some(Rhythm::Vt),
            "pea" => Some(Rhythm::Pea),
            "asystole" | "flatline" => Some(Rhythm::Asystole),
            _ => None,
        }
    }
}

/// What a shock actually did. Returned so the caller can chart the truth rather than "shock given".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShockResult {
    /// Shockable rhythm, shock delivered, rhythm converted.
    Converted,
    /// An arrest rhythm a shock cannot treat (PEA, asystole). Recorded as harm.
    NotShockable,
    /// The patient has a pulse. Shocking a perfusing rhythm is harm.
    Perfusing,
}

/// Coarse clinical status shown on the HUD and keyed for cutscenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatientStatus {
    Stable,
    Deteriorating,
    Critical,
    Arrest,
    Improving,
    Recovered,
    Dead,
}

/// Terminal outcome. NOTE: still the legacy fixed (anaphylaxis-shaped) set; the
/// data-driven engine bridges SCE outcome ids onto it via [`outcome_enum`]. A
/// follow-up generalises this to carry the SCE outcome id + kind directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    WinDischarge,
    WinIcu,
    DeathArrest,
    DeathBiphasic,
}

impl Outcome {
    /// Did the patient die?
    ///
    /// Asked here rather than by matching on variants at the call site, because the variants are
    /// a legacy shape: [`outcome_enum`] folds any outcome a case declares with `kind: "death"`
    /// into `DeathArrest` when it does not recognise the id, so this is the one place that knows
    /// the fold and the only honest way to ask the question.
    pub fn is_death(self) -> bool {
        matches!(self, Outcome::DeathArrest | Outcome::DeathBiphasic)
    }
}

/// Events emitted on tick/apply for the Director to react to.
#[derive(Debug, Clone, PartialEq)]
pub enum NarrativeBeat {
    StatusChanged(PatientStatus),
    Threshold(String),
    Harm(String),
    Terminal(Outcome),
}


/// Live, data-driven physiology state. Drives `tick` on a soft real-time clock and
/// `apply` on trainee actions, exactly like the old `PhysioState`.
/// A piece of equipment currently ON the patient.
///
/// Distinct from `done`, which records that an intervention HAPPENED. The difference matters the
/// moment anything wants to render the patient: "was given oxygen at some point" and "has a mask on
/// right now" are different facts, and only the second one tells the picture what to draw, the
/// monitor whether it has a probe, and the physiology whether the support is still there.
///
/// Removable on purpose. Pulling the oxygen has to let SpO₂ fall again — support that can only ever
/// be added teaches that nothing you attach can be got wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Equipment {
    pub id: String,
    /// The setting where one exists: litres per minute, mL/hr, joules.
    pub setting: Option<f64>,
    /// Scenario clock when it went on — lets a debrief say "oxygen took 4 minutes to arrive".
    pub since_sec: f64,
}

/// One thing that happened, and when.
///
/// Everything before this kept its own list — actions in one, chart lines in another, harms in a
/// third, the chat in a fourth — and none of them carried the clock except `Equipment.since_sec`.
/// You could not ask "what has happened, in order" without merging four half-records and guessing
/// the interleaving, which is precisely the question an agent has to answer before it can react to
/// a situation rather than to the last sentence typed at it.
/// The event kind an order that landed is recorded under. Rubrics match on this string.
pub const ACTION: &str = "action";

/// The event kind an order the case *refused* is recorded under.
///
/// A separate kind rather than a missing record, because "you called the cath lab and it said
/// no" is a thing that happened and a debrief has to be able to say so. It is simply not the
/// thing a rubric asking "was reperfusion achieved" is asking about — and since `vitals-osce`
/// filters strictly on `kind == "action"`, giving the refusal its own name is the whole fix.
pub const ACTION_REFUSED: &str = "action_refused";

/// What one intervention's effects actually did, as opposed to what they said.
///
/// Collected while the effects run and thrown away immediately after; nothing here reaches the
/// tape, the beats, the harm list or the outcome, which is why it cannot move a leaf.
#[derive(Debug, Default, Clone, Copy)]
struct EffectTrace {
    /// A `branch` matched none of its arms and ran its `else`.
    took_else: bool,
    /// Something changed about the patient — a variable, a flag, a harm, a state, a terminal
    /// outcome, a piece of equipment. A beat is not one of these.
    touched: bool,
}

impl EffectTrace {
    /// The case answered with words and changed nothing: the order did not happen.
    fn refused(self) -> bool {
        self.took_else && !self.touched
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Scenario clock, seconds. The ordering key, and what a debrief quotes.
    pub t_sec: f64,
    /// `action` | `action_refused` | `equipment` | `harm` | `status` | `beat` | `outcome`
    pub kind: String,
    pub text: String,
}

pub struct SceState {
    sce: Arc<Sce>,
    pub vitals: Vitals,
    pub status: PatientStatus,
    pub harm_events: Vec<String>,
    vars: BTreeMap<String, f64>,   // custom hidden axes (e.g. airway_patency)
    flags: BTreeMap<String, f64>,  // flag -> expiry (INFINITY = permanent); absent = cleared
    state_idx: usize,
    t_elapsed: f64,
    t_in_state: f64,
    done: BTreeSet<String>,
    fired: BTreeSet<String>,       // triggers already fired (for `once`)
    outcome: Option<Outcome>,
    outcome_id: Option<String>,
    /// What is attached to the patient right now. The single record every renderer reads.
    equipment: Vec<Equipment>,
    /// Everything that happened, in order, stamped with the scenario clock.
    events: Vec<Event>,
    /// `dbp₀/sbp₀`, when the engine is the one deriving the diastolic; `None` when this case
    /// drives `dbp` itself and the engine keeps its hands off. See [`SceState::normalise`].
    dbp_ratio: Option<f64>,
}

impl SceState {
    /// Build a fresh state at t=0 from a scenario.
    pub fn new(sce: Sce) -> SceState {
        let v0 = sce.vitals0;
        let vitals = Vitals { hr: v0.hr, sbp: v0.sbp, dbp: v0.dbp, spo2: v0.spo2, rr: v0.rr, temp: v0.temp, gcs: v0.gcs,
                              rhythm: Rhythm::Sinus };
        let vars = sce.variables.iter().map(|(k, a)| (k.clone(), a.init)).collect();
        let state_idx = sce.states.iter().position(|s| s.id == sce.initial_state).unwrap_or(0);
        let status = sce.states.get(state_idx).and_then(|s| s.status.as_deref()).map(parse_status).unwrap_or(PatientStatus::Stable);
        // Whether the engine derives the diastolic is a property of the authored file, so it is
        // decided once, here, and never re-asked while the run is moving. A `vitals0` the ratio
        // cannot be read off — a zero or a negative systolic, or a diastolic that is already at
        // or above it — is left alone for the clamp below to deal with.
        let dbp_ratio = (!writes_dbp(&sce) && v0.sbp > 0.0 && v0.dbp > 0.0 && v0.dbp < v0.sbp)
            .then(|| v0.dbp / v0.sbp);
        let mut st = SceState {
            sce: Arc::new(sce),
            vitals,
            status,
            harm_events: Vec::new(),
            vars,
            flags: BTreeMap::new(),
            state_idx,
            t_elapsed: 0.0,
            t_in_state: 0.0,
            done: BTreeSet::new(),
            fired: BTreeSet::new(),
            outcome: None,
            outcome_id: None,
            equipment: Vec::new(),
            events: Vec::new(),
            dbp_ratio,
        };
        let sce_rc = Arc::clone(&st.sce);
        st.adopt_state_rhythm(&sce_rc);
        st
    }

    pub fn outcome(&self) -> Option<Outcome> { self.outcome }
    /// The raw SCE outcome id (more general than the legacy `Outcome` enum).
    pub fn outcome_id(&self) -> Option<&str> { self.outcome_id.as_deref() }
    /// Seconds on the sim clock since the encounter started (was `PhysioState::t_sec`).
    pub fn t_sec(&self) -> f64 { self.t_elapsed }
    /// Read any live variable by name — a vital, a custom hidden axis
    /// (e.g. `"airway_patency"`), or a pseudo-clock. For the HUD/stats.
    pub fn var(&self, name: &str) -> f64 { self.get_var(name) }

    // ── equipment ──────────────────────────────────────────────────────────
    //
    // Attaching is idempotent on id: turning the flowmeter from 6 to 10 LPM updates the setting and
    // keeps `since_sec`, because it is the same mask that went on at the same moment. A second
    // "attach" is a device reporting its state, not a new device.

    /// Put equipment on the patient (or update its setting). Returns true if this is new.
    ///
    /// Turning the flowmeter up is a clinical act and belongs in the chart, so a changed setting
    /// records a line of its own. Re-reporting the same number does not — a device restating its
    /// state should not fill the record with noise.
    pub fn attach(&mut self, id: &str, setting: Option<f64>) -> bool {
        if let Some(e) = self.equipment.iter_mut().find(|e| e.id == id) {
            let before = e.setting;
            e.setting = setting.or(e.setting);
            let after = e.setting;
            if let (Some(v), true) = (after, before != after) {
                self.record("equipment", format!("{id} set to {v:.0}"));
            }
            return false;
        }
        let since = self.t_elapsed;
        self.equipment.push(Equipment { id: id.to_string(), setting, since_sec: since });
        match setting {
            Some(v) => self.record("equipment", format!("{id} on at {v:.0}")),
            None => self.record("equipment", format!("{id} on")),
        }
        true
    }

    /// Take it off. Returns true if something was actually removed.
    pub fn detach(&mut self, id: &str) -> bool {
        let n = self.equipment.len();
        self.equipment.retain(|e| e.id != id);
        let removed = self.equipment.len() != n;
        if removed { self.record("equipment", format!("{id} off")); }
        removed
    }

    /// Append to the timeline. Public so the layer that owns the learner's actions can put them on
    /// the SAME list as the physiology's own events — two lists would need merging, and a merge is
    /// where the interleaving gets guessed.
    pub fn record(&mut self, kind: &str, text: impl Into<String>) {
        let t = self.t_elapsed;
        self.events.push(Event { t_sec: t, kind: kind.to_string(), text: text.into() });
    }

    pub fn events(&self) -> &[Event] { &self.events }

    /// The last `n` events, oldest first — what an agent is given to reason about.
    pub fn recent(&self, n: usize) -> &[Event] {
        let start = self.events.len().saturating_sub(n);
        &self.events[start..]
    }

    pub fn equipment(&self) -> &[Equipment] { &self.equipment }
    pub fn has_equipment(&self, id: &str) -> bool { self.equipment.iter().any(|e| e.id == id) }

    /// Does this string name an intervention this scenario defines?
    ///
    /// Asked because the engine records prose under `action` as well as ids — the defibrillator
    /// writes its own sentence — and a renderer has to be able to tell "this is an id, translate
    /// it" from "this is already a line, print it".
    pub fn is_intervention(&self, id: &str) -> bool {
        self.sce.interventions.iter().any(|iv| iv.id == id)
    }

    /// The human name a case gives one of its own interventions.
    ///
    /// The id is the engine's word and the rubric's needle — `adrenaline_undosed`,
    /// `dx_epiglottitis`, `exam_throat` — and every one of those spells out what the mark sheet
    /// is looking for. This is the case author's word for the same thing, written to be read.
    /// `None` when the id is not an intervention here, or when the case never named it.
    pub fn intervention_label(&self, id: &str) -> Option<&str> {
        self.sce.interventions.iter().find(|iv| iv.id == id).and_then(|iv| iv.label.as_deref())
    }
    /// Setting of an attached item, if it has one (oxygen LPM, pump mL/hr).
    pub fn equipment_setting(&self, id: &str) -> Option<f64> {
        self.equipment.iter().find(|e| e.id == id).and_then(|e| e.setting)
    }

    // ── defibrillation ─────────────────────────────────────────────────────

    /// Deliver a shock. Returns what it actually did.
    ///
    /// Deliberately NOT a scenario intervention. Whether a rhythm can be shocked is physiology, not
    /// content — if every scenario author had to re-declare it, one of them would eventually let a
    /// student shock asystole and be told "good job".
    ///
    /// An inappropriate shock is charted as harm rather than ignored. Ignoring it teaches that
    /// pressing the button is free, when the cost is the ten seconds off the chest and the
    /// adrenaline not given.
    pub fn defibrillate(&mut self, joules: f64) -> ShockResult {
        let r = self.vitals.rhythm;
        if r.perfusing() {
            let h = format!("shock {joules:.0} J delivered to a perfusing patient — dangerous");
            self.harm_events.push(h.clone());
            self.record("harm", h);
            return ShockResult::Perfusing;
        }
        if !r.shockable() {
            let h = format!("shock {joules:.0} J into {} — not shockable, and it cost compressions and adrenaline", r.as_str());
            self.harm_events.push(h.clone());
            self.record("harm", h);
            return ShockResult::NotShockable;
        }
        self.record("action", format!("defibrillate {joules:.0} J — rhythm back to sinus"));
        self.vitals.rhythm = Rhythm::Sinus;
        ShockResult::Converted
    }

    /// Advance the soft real-time clock by `dt` seconds.
    pub fn tick(&mut self, dt: f64) -> Vec<NarrativeBeat> {
        let mut beats = Vec::new();
        if self.outcome.is_some() { return beats; }
        let sce_rc = Arc::clone(&self.sce);
        let sce: &Sce = &sce_rc;

        self.t_elapsed += dt;
        self.t_in_state += dt;
        let now = self.t_elapsed;
        self.flags.retain(|_, exp| *exp > now);   // expire timed flags
        self.apply_dynamics(sce, dt);

        // global triggers. A trigger that *changes state* (e.g. biphasic onset)
        // defers the new state's status by one tick — the old phase machine set the
        // new phase without touching status that tick (only the next tick's dynamics
        // re-derive it). Transitions (below) instead update status immediately, like
        // the old arrest/recovered edges.
        let mut trig_changed = false;
        for tr in &sce.triggers {
            if self.outcome.is_some() { break; }
            if tr.once && self.fired.contains(&tr.id) { continue; }
            if self.eval(sce, &tr.when) {
                self.fired.insert(tr.id.clone());
                if self.run_effects(sce, &tr.doo, &mut beats) { trig_changed = true; }
            }
        }
        // one state edge per tick (mirrors the old phase machine)
        if self.outcome.is_none() && !trig_changed {
            self.take_first_transition(sce, &mut beats);
            self.update_status(sce, &mut beats);
        }
        // A trigger or an edge can move the systolic too, and `apply_dynamics` above ran before
        // either of them. Without this the pair would spend the rest of the tick disagreeing —
        // which is exactly the window a monitor polls in.
        self.normalise();
        beats
    }

    /// Record a trainee action and apply its physiologic effect.
    /// Apply a player action. The interpreter only ever needed the text, so the tape records
    /// text rather than a host-specific action type — one less thing for a second
    /// implementation to get wrong.
    pub fn apply(&mut self, action: &str) -> Vec<NarrativeBeat> {
        let text = crate::text::canon(action).to_lowercase();
        let sce_rc = Arc::clone(&self.sce);
        let i = self.match_intervention(&sce_rc, &text);
        self.fire(i)
    }

    /// Which intervention this text names, if any.
    ///
    /// Recognition, separated from application. The caller runs this once while the learner is
    /// playing, writes the answer onto the tape, and replay never has to ask again — which is what
    /// lets recognition get better without changing what an anchored run did.
    pub fn resolve(&self, text: &str) -> Option<String> {
        let sce_rc = Arc::clone(&self.sce);
        let t = crate::text::canon(text).to_lowercase();
        self.match_intervention(&sce_rc, &t).map(|i| sce_rc.interventions[i].id.clone())
    }

    /// Apply an intervention the caller has already identified.
    ///
    /// The route a resolved tape takes. Recognition happened once, when the run was played; here
    /// there is an id and nothing to interpret, which is what keeps replay reproducible however
    /// clever recognition gets. An id this scenario does not define does nothing — a verifier
    /// replaying somebody else's tape must not panic on it, and half-applying would be worse.
    pub fn apply_id(&mut self, id: &str) -> Vec<NarrativeBeat> {
        let sce_rc = Arc::clone(&self.sce);
        let i = sce_rc.interventions.iter().position(|iv| iv.id == id);
        self.fire(i)
    }

    /// Run intervention `i`, then let the scenario move.
    ///
    /// Transitions and status are re-checked even when nothing matched, because time and the
    /// patient do not wait for the learner to say something the machine understands.
    fn fire(&mut self, i: Option<usize>) -> Vec<NarrativeBeat> {
        let mut beats = Vec::new();
        if self.outcome.is_some() { return beats; }
        let sce_rc = Arc::clone(&self.sce);
        let sce: &Sce = &sce_rc;

        if let Some(i) = i {
            let iv = &sce.interventions[i];
            let id = iv.id.clone();
            let eq = iv.equipment.clone();
            let eq_set = iv.equipment_setting;
            // The order itself, on the timeline, by id and stamped with the clock. It used to
            // leave nothing behind but a membership in `done`, so the record could say a mask went
            // on and at what flow and could not say adrenaline was ever given — let alone when.
            // A debrief cannot be written from a record that does not contain the orders, and the
            // time and the ordering are exactly the parts a verifier can recompute.
            //
            // Recorded before the effects run, so the order precedes the harm it caused.
            // Its *kind* is decided afterwards — see the refusal note below — so the index is
            // kept rather than the event.
            let at = self.events.len();
            self.record(ACTION, id.clone());
            let mut trace = EffectTrace::default();
            if let Some(h) = &sce.interventions[i].harm {
                let h = h.clone();
                trace.touched = true;
                self.harm_events.push(h.clone());
                self.record("harm", h.clone());
                beats.push(NarrativeBeat::Harm(h));
            }
            self.run_effects_traced(sce, &sce.interventions[i].effects.clone(), &mut beats, &mut trace);
            // Whatever the intervention leaves on the patient goes onto the bedside too. Typed
            // order and pressed button now converge on one record; attach() is idempotent on id, so
            // a learner who says it and then presses it does not end up with two masks.
            if let Some(e) = eq { trace.touched = true; self.attach(&e, eq_set); }
            // ── an order the case refused is not an order the case did ──────────────
            // `record("action", …)` used to fire unconditionally, before the branch that
            // decides whether the order lands. So the cath lab that answered "the lab wants an
            // ecg before it spins up" still wrote `action cath_lab`, and the rubric — which
            // reads the event log and nothing else — paid the full ten points for reperfusion
            // to a candidate who never got one. Same for heparin before the scan (6), and for
            // calling GI on an unresuscitated bleed (4): refuse the order, keep the marks.
            //
            // A refusal is now a kind of its own, so no `action` needle can match it and every
            // rubric in the repo is fixed without a line being edited in any of them.
            //
            // The test is [`EffectTrace`]: the order fell through to an `else`, and nothing in
            // the whole intervention touched the patient — no variable, no flag, no harm, no
            // state, no terminal, no equipment. Words only. That is what distinguishes the
            // cath lab saying no from osce-d's crystalloid, which *does* go in when the lines
            // are thin, just slower and worth less — and that one is still an action, because
            // a litre went in.
            //
            // What deliberately does NOT move: `done`, the tape, the beats, the harm list and
            // the outcome. `done` is read by the scenarios' own conditions, so demoting it
            // would change what cases do; the other four are what the leaf hashes. The event
            // log is not in the leaf (`vitals_replay::leaf` hashes tape + beats + harms +
            // outcome), so a run anchored before this change replays to the identical leaf
            // after it. Only the rubric's reading of the run moves — which is the point.
            if trace.refused() {
                self.events[at].kind = ACTION_REFUSED.to_string();
            }
            self.done.insert(id);
        }
        // rescue can promote immediately (the old apply() re-checked is_safe)
        self.take_first_transition(sce, &mut beats);
        self.update_status(sce, &mut beats);
        // An order that moves the systolic moves the pair. Fluids going in is the obvious one:
        // the pressure the learner reads back must not be half of the one they produced.
        self.normalise();
        beats
    }

    // ── internals ──────────────────────────────────────────────────────────

    fn apply_dynamics(&mut self, sce: &Sce, dt: f64) {
        let st = &sce.states[self.state_idx];
        for d in &st.dynamics {
            let on = d.when.as_ref().is_none_or(|c| self.eval(sce, c));
            if on {
                let mut nv = self.get_var(&d.var) + d.rate_per_min / 60.0 * dt;
                if let Some(f) = d.floor { nv = nv.max(f); }
                if let Some(c) = d.ceil { nv = nv.min(c); }
                self.set_var(sce, &d.var, nv);
            }
        }
        self.normalise();
    }

    /// Keep the vitals arithmetically possible — and keep the blood pressure a blood pressure.
    ///
    /// Systolic and diastolic are separate variables that a scenario drives independently, and
    /// nothing stopped one crossing the other: shock drops systolic faster, and `50/54` reached
    /// the screen. That is not a severe blood pressure, it is not a blood pressure at all.
    /// Narrow pulse pressure in shock is real; inverted pulse pressure is a bug.
    ///
    /// The clamp at the bottom was written as the guard rail for that, and it was being *hit*,
    /// which is the real finding. Almost every case declares dynamics for `sbp` and none at all
    /// for `dbp`, so the systolic marched down while the diastolic stood exactly where `vitals0`
    /// left it: `osce-a`, untreated, ran `88/60`, `84/60`, `80/60`, `76/60`, `62/60` and then
    /// `58/58`. A pulse pressure of zero is not a low blood pressure — it is a number no patient
    /// has ever had, and it is the first thing a clinician stops at.
    ///
    /// So when the case does not drive `dbp` itself, the diastolic follows the systolic in the
    /// ratio this patient arrived with, `dbp₀/sbp₀`. Two reasons for that rule and not another:
    ///
    ///   * It is **neutral**. It says only that this patient's two pressures belong together —
    ///     which the case already said when it wrote `vitals0` — and invents no number of its
    ///     own. Distributive shock drops the diastolic disproportionately and cardiogenic shock
    ///     narrows the pulse pressure early; choosing between those is a clinical decision that
    ///     belongs in the scenario file and to the clinician who reviews it, not in an engine
    ///     that cannot see the diagnosis.
    ///   * Being a ratio rather than a fixed offset, the pulse pressure **narrows as the
    ///     pressure falls**, which is the direction every kind of shock moves, without committing
    ///     to how far. It reaches zero only when the systolic does — and a systolic of zero is a
    ///     patient for whom `reading.rs` reports no cuff reading at all.
    ///
    /// A case that drives its own `dbp` keeps full control; see [`writes_dbp`] for what counts as
    /// driving it. The clamp stays underneath both, as the backstop it was meant to be rather
    /// than the thing producing the reading.
    fn normalise(&mut self) {
        if let Some(k) = self.dbp_ratio {
            self.vitals.dbp = self.vitals.sbp * k;
        }
        if self.vitals.dbp > self.vitals.sbp {
            self.vitals.dbp = self.vitals.sbp;
        }
    }

    /// Returns true if an effect changed state or terminated.
    fn run_effects(&mut self, sce: &Sce, es: &[Effect], beats: &mut Vec<NarrativeBeat>) -> bool {
        let mut ignored = EffectTrace::default();
        self.run_effects_traced(sce, es, beats, &mut ignored)
    }

    /// The same walk, with a note of what it did — see [`EffectTrace`]. Only [`SceState::fire`]
    /// reads the note; every other caller takes the plain wrapper above.
    fn run_effects_traced(
        &mut self,
        sce: &Sce,
        es: &[Effect],
        beats: &mut Vec<NarrativeBeat>,
        trace: &mut EffectTrace,
    ) -> bool {
        let mut changed = false;
        for e in es {
            if self.outcome.is_some() { break; }
            match e {
                Effect::Delta { delta, cap, floor } => {
                    trace.touched = true;
                    for (k, v) in delta {
                        let mut nv = self.get_var(k) + v;
                        if let Some(c) = cap { nv = nv.min(*c); }
                        if let Some(f) = floor { nv = nv.max(*f); }
                        self.set_var(sce, k, nv);
                    }
                }
                Effect::Set { set } => {
                    trace.touched = true;
                    for (k, v) in set { self.set_var(sce, k, *v); }
                }
                Effect::Flag { flag, value, for_sec } => {
                    trace.touched = true;
                    if *value {
                        let exp = for_sec.map_or(f64::INFINITY, |s| self.t_elapsed + s);
                        self.flags.insert(flag.clone(), exp);
                    } else {
                        self.flags.remove(flag);
                    }
                }
                // Deliberately not `touched`: a beat is the case talking, not the case
                // changing. It is the whole difference between "GI, on the phone: not on an
                // empty tank" and a scope that actually went in.
                Effect::Beat { beat } => beats.push(NarrativeBeat::Threshold(beat.clone())),
                Effect::Harm { harm } => {
                    trace.touched = true;
                    self.harm_events.push(harm.clone());
                    self.record("harm", harm.clone());
                    beats.push(NarrativeBeat::Harm(harm.clone()));
                }
                Effect::ToState { to_state } => { trace.touched = true; self.change_state(sce, to_state); changed = true; }
                Effect::Outcome { outcome } => { trace.touched = true; self.terminate(sce, outcome, beats); changed = true; }
                Effect::Branch { branch, els } => {
                    let mut matched = false;
                    for a in branch {
                        if self.eval(sce, &a.iff) {
                            if self.run_effects_traced(sce, &a.then, beats, trace) { changed = true; }
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        trace.took_else = true;
                        if self.run_effects_traced(sce, els, beats, trace) { changed = true; }
                    }
                }
            }
        }
        changed
    }

    fn take_first_transition(&mut self, sce: &Sce, beats: &mut Vec<NarrativeBeat>) -> bool {
        let st = &sce.states[self.state_idx];
        for t in &st.transitions {
            if self.eval(sce, &t.when) {
                self.run_effects(sce, &t.doo, beats);
                if self.outcome.is_some() { return true; }
                if let Some(to) = &t.to_state { self.change_state(sce, to); return true; }
                if let Some(oc) = &t.outcome { self.terminate(sce, oc, beats); return true; }
                return true;
            }
        }
        false
    }

    fn change_state(&mut self, sce: &Sce, id: &str) {
        if let Some(i) = sce.states.iter().position(|s| s.id == id) {
            self.state_idx = i;
            self.t_in_state = 0.0;
            self.adopt_state_rhythm(sce);
        }
    }

    /// A state that names a rhythm imposes it on entry. A state that says nothing leaves it alone,
    /// so a shock that converted VF is not silently undone by the next transition.
    fn adopt_state_rhythm(&mut self, sce: &Sce) {
        if let Some(r) = sce.states[self.state_idx].rhythm.as_deref().and_then(Rhythm::parse) {
            self.vitals.rhythm = r;
        }
    }

    fn terminate(&mut self, sce: &Sce, id: &str, beats: &mut Vec<NarrativeBeat>) {
        let kind = sce.outcomes.iter().find(|o| o.id == id).map(|o| o.kind.as_str()).unwrap_or("death");
        let o = outcome_enum(id, kind);
        self.outcome = Some(o);
        self.outcome_id = Some(id.to_string());
        self.record("outcome", id.to_string());
        self.status = match kind { "win" => PatientStatus::Recovered, "death" => PatientStatus::Dead, _ => self.status };
        // A body that has stopped has stopped. Terminating used to set a status and leave every
        // vital frozen at whatever it held the instant before, so the screen showed a patient
        // marked Dead with a pulse of 128 and a respiratory rate of 28 — the monitor sweeping,
        // the chest still rising. Death is a fact about the patient, not a display state.
        if kind == "death" {
            self.vitals.hr = 0.0;
            self.vitals.sbp = 0.0;
            self.vitals.dbp = 0.0;
            self.vitals.rr = 0.0;
            // No pulse for an oximeter to read. A saturation is a measurement of flowing blood.
            self.vitals.spo2 = 0.0;
            self.vitals.gcs = 3;
            self.vitals.rhythm = Rhythm::Asystole;
        }
        beats.push(NarrativeBeat::Terminal(o));
    }

    fn update_status(&mut self, sce: &Sce, beats: &mut Vec<NarrativeBeat>) {
        if self.outcome.is_some() { return; }
        let st = &sce.states[self.state_idx];
        let mut chosen = st.status.clone();
        for b in &st.bands {
            if self.eval(sce, &b.when) { chosen = Some(b.status.clone()); break; }
        }
        if let Some(name) = chosen {
            let ns = parse_status(&name);
            if ns != self.status {
                self.status = ns;
                self.record("status", format!("{ns:?}"));
                beats.push(NarrativeBeat::StatusChanged(ns));
            }
        }
    }

    fn match_intervention(&self, sce: &Sce, text: &str) -> Option<usize> {
        // Both sides canonicalised: the learner may type through an IME, and a case authored in
        // Japanese may carry full-width keywords for the same reason. Comparing raw would make
        // matching depend on which keyboard wrote the case file.
        let t = crate::text::canon(text).to_lowercase();
        let has = |k: &str| t.contains(&crate::text::canon(k).to_lowercase());
        for (i, iv) in sce.interventions.iter().enumerate() {
            let m = &iv.matcher;
            let has_positive = !m.any_kw.is_empty() || !m.all_groups.is_empty();
            let any_ok = m.any_kw.is_empty() || m.any_kw.iter().any(|k| has(k));
            let groups_ok = m.all_groups.iter().all(|g| g.iter().any(|k| has(k)));
            let not_ok = !m.not_kw.iter().any(|k| has(k));
            if has_positive && any_ok && groups_ok && not_ok {
                return Some(i);
            }
        }
        None
    }

    fn eval(&self, sce: &Sce, c: &Cond) -> bool {
        match c {
            Cond::All { all } => all.iter().all(|x| self.eval(sce, x)),
            Cond::Any { any } => any.iter().any(|x| self.eval(sce, x)),
            Cond::Not { not } => !self.eval(sce, not),
            Cond::Cmp { var, op, value } => {
                let a = self.get_var(var);
                match op {
                    Op::Lt => a < *value, Op::Le => a <= *value,
                    Op::Gt => a > *value, Op::Ge => a >= *value,
                    Op::Eq => a == *value, Op::Ne => a != *value,
                }
            }
            Cond::Flag { flag, is } => self.flag_active(flag) == *is,
            Cond::InState { in_state } => sce.states[self.state_idx].id == *in_state,
            Cond::Done { done } => self.done.contains(done),
        }
    }

    fn flag_active(&self, f: &str) -> bool {
        self.flags.get(f).is_some_and(|&exp| exp > self.t_elapsed)
    }

    fn get_var(&self, name: &str) -> f64 {
        match name {
            "hr" => self.vitals.hr, "sbp" => self.vitals.sbp, "dbp" => self.vitals.dbp,
            "spo2" => self.vitals.spo2, "rr" => self.vitals.rr, "temp" => self.vitals.temp,
            "gcs" => self.vitals.gcs as f64,
            "t_in_state" => self.t_in_state, "t_elapsed" => self.t_elapsed,
            _ => *self.vars.get(name).unwrap_or(&0.0),
        }
    }

    fn set_var(&mut self, sce: &Sce, name: &str, val: f64) {
        match name {
            "hr" => self.vitals.hr = val, "sbp" => self.vitals.sbp = val, "dbp" => self.vitals.dbp = val,
            "spo2" => self.vitals.spo2 = val, "rr" => self.vitals.rr = val, "temp" => self.vitals.temp = val,
            "gcs" => self.vitals.gcs = val.round().clamp(0.0, 15.0) as u8,
            "t_in_state" | "t_elapsed" => {}
            _ => {
                let mut v = val;
                if let Some(ax) = sce.variables.get(name) {
                    if let Some(mn) = ax.min { v = v.max(mn); }
                    if let Some(mx) = ax.max { v = v.min(mx); }
                }
                self.vars.insert(name.to_string(), v);
            }
        }
    }
}

/// The one variable a case has to write to take its own diastolic back off the engine.
const DBP: &str = "dbp";

/// Does this scenario say anything about its own diastolic?
///
/// Not only `dynamics`. A `set` or a `delta` on `dbp` — in an intervention, a trigger or a
/// transition, including inside the arms of a `branch` — is a case stating what the diastolic
/// does just as plainly as a dynamic is, and an engine that overwrote it on the next tick would
/// be silently discarding an authored number. So any authored write at all hands the diastolic
/// back to the case, for that scenario, permanently.
///
/// As of this commit no case in the repo writes `dbp` anywhere: across the twelve stations, the
/// four season scenarios and the conformance copy of EP1, the only occurrence of the name is the
/// `vitals0` line. Which is the whole reason the derivation cannot move an outcome — nothing
/// reads it either.
fn writes_dbp(sce: &Sce) -> bool {
    fn in_effects(es: &[Effect]) -> bool {
        es.iter().any(|e| match e {
            Effect::Delta { delta, .. } => delta.contains_key(DBP),
            Effect::Set { set } => set.contains_key(DBP),
            Effect::Branch { branch, els } => {
                branch.iter().any(|a| in_effects(&a.then)) || in_effects(els)
            }
            _ => false,
        })
    }
    sce.states.iter().any(|s| {
        s.dynamics.iter().any(|d| d.var == DBP)
            || s.transitions.iter().any(|t| in_effects(&t.doo))
    }) || sce.triggers.iter().any(|t| in_effects(&t.doo))
        || sce.interventions.iter().any(|i| in_effects(&i.effects))
}

fn parse_status(s: &str) -> PatientStatus {
    match s.to_lowercase().as_str() {
        "stable" => PatientStatus::Stable,
        "deteriorating" => PatientStatus::Deteriorating,
        "critical" => PatientStatus::Critical,
        "arrest" => PatientStatus::Arrest,
        "improving" => PatientStatus::Improving,
        "recovered" => PatientStatus::Recovered,
        "dead" => PatientStatus::Dead,
        _ => PatientStatus::Deteriorating,
    }
}

/// Every outcome id this bridge actually knows a name for.
///
/// One list, checked by [`crate::schema::Sce::validate`], so an author cannot invent a
/// `death_at_home` that quietly plays as `death_arrest` — which EP4 did, taking the "dies at
/// home" line out of the game and leaving it as dead code in the page.
pub const OUTCOME_IDS: [&str; 4] = ["win_discharge", "win_icu", "death_arrest", "death_biphasic"];

/// Bridge SCE outcome ids → the legacy `Outcome` enum (kept while the Director +
/// Story-Mode HUD still speak `Outcome`). Unknown ids fall back by `kind`.
fn outcome_enum(id: &str, kind: &str) -> Outcome {
    match id {
        "win_discharge" => Outcome::WinDischarge,
        "win_icu" => Outcome::WinIcu,
        "death_arrest" => Outcome::DeathArrest,
        "death_biphasic" => Outcome::DeathBiphasic,
        _ => if kind == "win" { Outcome::WinDischarge } else { Outcome::DeathArrest },
    }
}

/// Canonical string form of a beat.
///
/// This is part of the specification, not a debug convenience: the anchored leaf hashes these
/// strings, so any implementation that renders them differently produces different leaves for
/// the same run. Explicit `match` rather than `Debug` on the enum, so adding a variant is a
/// deliberate act and not a silent hash change.
pub fn render_beat(b: &NarrativeBeat) -> String {
    match b {
        NarrativeBeat::StatusChanged(s) => format!("status:{s:?}"),
        NarrativeBeat::Threshold(t) => format!("threshold:{t}"),
        NarrativeBeat::Harm(h) => format!("harm:{h}"),
        NarrativeBeat::Terminal(o) => format!("terminal:{o:?}"),
    }
}
