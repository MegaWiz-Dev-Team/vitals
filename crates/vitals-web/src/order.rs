//! What a learner's typed order is read as: the scenario's intervention, or the defibrillator.
//!
//! Recognition happens once, live, and its answer goes on the tape (`Step::Act` with an id, or
//! `Step::Shock` with the joules); replay never reads the words again. Lifted out of `main.rs` so
//! the reading can be tested from outside the server, against every case on disk.

use vitals_sce::{recognise, SceState};

use crate::lang;

/// Which intervention an order names — the scenario first, then the language layer.
///
/// Two readers, in a fixed order, and the order is the whole safety argument:
///
///   1. **The scenario's own matcher.** Its answer is final. Every keyword a case author wrote,
///      in whatever language they wrote it in, decides what happens on their own case.
///   2. **Only if that declined**, [`lang::canonical_order`] offers the English order a
///      non-English phrase names — and the same matcher rules on *that*. So a translation can
///      add recognition and can never redirect, shadow or override an order a case already
///      understood, and a station with no such intervention still does nothing, exactly as today.
///
/// Both read as [`vitals_sce::recognise`] does: an abbreviation only as a whole word, no keyword
/// inside a compound that means something else, and no negated order — so "give adrenaline IM" is
/// IM, "ประเมินทางเดินหายใจ" is not standing her up, and "no aspirin" is not aspirin.
///
/// The empty string means nobody understood it, which is a real answer and goes on the tape as
/// one: replay must stay faithful to a run in which nothing happened.
pub fn resolve_order(st: &SceState, act: &str) -> String {
    st.resolve(act).or_else(|| through_language(st, act)).unwrap_or_default()
}

/// The language layer's turn: the English order `act` translates to, as the scenario reads it —
/// unless the learner negated the phrase that was translated ([`SceState::alias_stands`]).
/// Without that the alias table re-opened the negation trap one layer down: "อย่าเพิ่งให้กลับบ้าน"
/// translated to "discharge home", and EP4, which keys its discharge in English only, sent her home.
fn through_language(st: &SceState, act: &str) -> Option<String> {
    let (key, en) = lang::canonical_order_key(act)?;
    st.resolve(en).filter(|id| st.alias_stands(id, act, key))
}

// ── the defibrillator ───────────────────────────────────────────────────────
//
// The one order on the tray that is not an order. Everything else in `kit_phrase` is a sentence
// the *scenario* rules on: the case says what oxygen does to this patient, and a case that never
// mentions oxygen is entitled to ignore it. Shockability is not like that. Whether a rhythm can
// be shocked is physiology — `Rhythm::shockable` — and routing it through each case's own
// `interventions` list is how the shelf ended up where it was: `ep2` branched on the *name of a
// state*, and `ep3`, `ep4` and `ep5` declared no `defibrillate` at all, so pressing the button on
// a five-year-old in cardiac arrest charted nothing, scored nothing and appeared in no debrief.
//
// So the button and the typed order both end at `SceState::defibrillate`, and both write the
// same `Step::Shock` on the tape. What is deliberately *not* changed is who answers first: the
// scenario's matcher still rules on the text, and only when it declines does the engine's own route open.
// A station that defines its own shock intervention keeps it, unchanged, and every rubric in the
// repo scores exactly as it did.

/// What the defibrillator delivers when nobody said. The page's own default preset.
pub const DEFIB_JOULES: f64 = 200.0;

/// Words that mean "deliver a shock", and the ones that mean the *other* kind of shock.
///
/// Shaped like a scenario `Matcher` because that is the shape this repo already argues about:
/// `any_kw` plus `not_kw`. The exclusions are the whole reason it is not a bare substring test —
/// "cardiogenic shock", "septic shock", "she is in shock" and "shock index" are all things a
/// candidate types on a station where nobody wants 200 joules delivered, and on `ep5`, whose
/// patient is exsanguinating, they are things a candidate types *often*.
///
/// The keywords are `ep2`'s own, taken off the intervention this replaced, so a learner who
/// reached the old one reaches this.
const SHOCK_KW: [&str; 5] = ["defib", "shock", "cardiovert", "joule", "200j"];
const NOT_SHOCK_KW: [&str; 11] = [
    "cardiogenic", "septic", "hypovol", "haemorrhagic", "hemorrhagic", "distributive",
    "neurogenic", "spinal shock", "obstructive shock", "shock index", "in shock",
];

/// Does this text name a defibrillator — one the learner did not negate?
///
/// "do not shock" and "no defib" are not shocks. They were: this route opens on every case whose
/// scenario declines the words, so a negated shock delivered 200 J into whatever rhythm was
/// running — the most dangerous misreading on the page. A keyword counts as
/// [`recognise::says`] reads it.
fn names_a_shock(text: &str) -> bool {
    let t = recognise::canon(text);
    SHOCK_KW.iter().any(|k| recognise::says(&t, k, &[])) && !NOT_SHOCK_KW.iter().any(|k| t.contains(k))
}

/// The energy an order asks for, in joules, if it names one a defibrillator could deliver.
///
/// Read off the text the learner actually typed rather than off the language layer's canonical
/// English, so "ช็อกไฟฟ้า 360" keeps its 360 — `lang::canonical_order` answers *whether* a phrase
/// is a shock, and would flatten every one of them to the same headword.
fn joules_in(text: &str) -> Option<f64> {
    let t = vitals_sce::text::canon(text);
    let mut best = None;
    let mut n = String::new();
    for c in t.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            n.push(c);
            continue;
        }
        if !n.is_empty() {
            // A defibrillator's dial: nothing outside this is an energy, so a year, a bed
            // number or a saturation cannot be read as one.
            if let Ok(v) = n.parse::<f64>() {
                if (1.0..=1000.0).contains(&v) && best.is_none() {
                    best = Some(v);
                }
            }
            n.clear();
        }
    }
    best
}

/// The shock an order asks for, if it is one — through the language layer, like every order,
/// and not when the learner negated the phrase it translated ("ห้ามช็อกไฟฟ้า").
pub fn shock_order(act: &str) -> Option<f64> {
    let t = recognise::canon(act);
    let named = names_a_shock(act)
        || lang::canonical_order_key(act)
            .is_some_and(|(key, en)| names_a_shock(en) && recognise::says(&t, &recognise::canon(key), &[]));
    named.then(|| joules_in(act).unwrap_or(DEFIB_JOULES))
}
