//! The patient must not hand over what the learner has not earned.
//!
//! Her story marks some facts `on_direct_ask`: she says them only when asked about that exact
//! thing. A language model, left alone, volunteers them within a turn — "I've reacted before, a
//! doctor gave me adrenaline" — and a lazy candidate skips the history-taking that the station
//! exists to assess. The gate reads her reply against what the learner has actually asked and
//! refuses a reply that leaks an unearned reveal, so the model can be told to try again.
//!
//! This is not the embla gate. There is no hidden diagnosis to name here — the diagnosis is the
//! learner's to infer. What is protected is the *timing* of a reveal, which is what makes the
//! examination an examination.

use vitals_sce::reveal_gate::{Gate, Node, Reveal, Violation};

fn nodes() -> Vec<Node> {
    let n = |id: &str, reveal, text: &str, kw: &[&str]| Node {
        id: id.into(),
        reveal,
        text: text.into(),
        keywords: kw.iter().map(|k| k.to_string()).collect(),
    };
    vec![
        n("cc", Reveal::Volunteered, "I can't breathe properly", &["breathe"]),
        n("allergy", Reveal::OnAsk, "I'm allergic to shrimp", &["allerg"]),
        n("previous", Reveal::OnDirectAsk,
          "I've reacted before but never like this. A doctor gave me adrenaline",
          &["before", "previous", "happened"]),
        n("meds", Reveal::OnDirectAsk, "No medical conditions. I don't take anything regularly",
          &["medic", "conditions", ""]),
    ]
}

#[test]
fn volunteering_an_unearned_direct_ask_fact_is_a_violation() {
    let g = Gate::new(&nodes());
    let earned = std::collections::HashSet::new(); // asked nothing
    let v = g.check("Oh, I've reacted before — a doctor gave me adrenaline once.", &earned);
    assert_eq!(v, vec![Violation::UnearnedReveal("previous".into())]);
}

#[test]
fn the_same_fact_once_earned_is_allowed() {
    let g = Gate::new(&nodes());
    let earned: std::collections::HashSet<String> = ["previous".to_string()].into();
    let v = g.check("Yes — I've reacted before, a doctor gave me adrenaline.", &earned);
    assert!(v.is_empty(), "a fact the learner asked about is hers to hear");
}

#[test]
fn volunteered_and_on_ask_facts_are_never_gated() {
    // Only on_direct_ask is timing-protected. She may always state her complaint, and on_ask
    // facts are the normal reward for asking — neither is a leak.
    let g = Gate::new(&nodes());
    let earned = std::collections::HashSet::new();
    assert!(g.check("I can't breathe properly, it came on so fast.", &earned).is_empty());
    assert!(g.check("I'm allergic to shrimp, and I ate some.", &earned).is_empty());
}

#[test]
fn matching_is_canonical_so_a_full_width_leak_still_trips() {
    // The reply passes through the same NFKC canon the tape uses. A model that answers in
    // full-width or with odd spacing must not slip an unearned reveal past a byte comparison —
    // the exact hole the lowercase-only version had.
    let g = Gate::new(&nodes());
    let earned = std::collections::HashSet::new();
    let v = g.check("Ｉ'ｖｅ　ｒｅａｃｔｅｄ　ｂｅｆｏｒｅ, a doctor gave me ａｄｒｅｎａｌｉｎｅ.", &earned);
    assert_eq!(v, vec![Violation::UnearnedReveal("previous".into())]);
}

#[test]
fn a_reply_that_leaks_two_unearned_facts_reports_both() {
    let g = Gate::new(&nodes());
    let earned = std::collections::HashSet::new();
    let mut v = g.check(
        "I've reacted before and a doctor gave me adrenaline. I take nothing regularly, no conditions.",
        &earned,
    );
    v.sort();
    assert_eq!(v, vec![
        Violation::UnearnedReveal("meds".into()),
        Violation::UnearnedReveal("previous".into()),
    ]);
}

#[test]
fn a_clean_reply_that_earns_nothing_and_leaks_nothing_is_fine() {
    let g = Gate::new(&nodes());
    let earned = std::collections::HashSet::new();
    assert!(g.check("It hurts. I'm frightened.", &earned).is_empty());
}

// ── the retry hint: gate-side, so the patient stays pure model-plumbing ──────────────────────

#[test]
fn a_clean_reply_asks_for_no_retry() {
    // No violations, no hint — the caller sends the reply as is.
    assert_eq!(vitals_sce::reveal_gate::retry_hint(&[]), None);
}

#[test]
fn the_hint_names_what_leaked_and_says_not_to_volunteer_it() {
    use vitals_sce::reveal_gate::{retry_hint, Violation};
    let h = retry_hint(&[
        Violation::UnearnedReveal("previous".into()),
        Violation::UnearnedReveal("meds".into()),
    ])
    .expect("a hint for a leak");
    // It carries the ids the system prompt already maps to content, and the discipline to apply.
    assert!(h.contains("previous"), "{h}");
    assert!(h.contains("meds"), "{h}");
    assert!(h.to_lowercase().contains("unless"), "{h}");
}

// ── what the learner has earned: the first reader of dialogue `keywords` ─────────────────────

use vitals_sce::reveal_gate::{earned, guard, Outcome, REGEN_CAP};

#[test]
fn a_question_containing_a_keyword_earns_that_node() {
    let e = earned(&nodes(), &["Has this happened before?"]);
    assert!(e.contains("previous"));
    assert!(!e.contains("meds"));
}

#[test]
fn keywords_match_through_the_same_fold_as_intervention_keywords() {
    // Full-width and upper case, as an IME or a shift key would type it.
    let e = earned(&nodes(), &["ＡＮＹ　ＭＥＤＩＣＩＮＥＳ?"]);
    assert!(e.contains("meds"));
}

#[test]
fn a_blank_keyword_earns_nothing() {
    // `meds` carries a stray "" in its keywords; it must not match every question.
    let e = earned(&nodes(), &["how are you"]);
    assert!(!e.contains("meds"));
    assert!(earned(&nodes(), &[] as &[&str]).is_empty());
}

#[test]
fn every_question_on_the_tape_counts_not_only_the_last() {
    let e = earned(&nodes(), &["any medicines?", "how are you"]);
    assert!(e.contains("meds"));
}

// ── the gated reply: check, regenerate with the hint, fall back ───────────────────────────────

const LEAK: &str = "Oh, I've reacted before — a doctor gave me adrenaline once.";
const CLEAN: &str = "It hurts. I'm frightened.";

/// What `guard` returned, and the hint each model call was given.
type Run = (Result<(String, Outcome), ()>, Vec<Option<String>>);

fn run(replies: &[Result<&str, ()>]) -> Run {
    let g = Gate::new(&nodes());
    let mut hints = Vec::new();
    let mut i = 0;
    let r = guard(&g, &Default::default(), "FALLBACK", |h| {
        hints.push(h.map(String::from));
        let out = replies[i.min(replies.len() - 1)].map(String::from);
        i += 1;
        out
    });
    (r, hints)
}

#[test]
fn a_clean_reply_is_one_call_and_untouched() {
    let (r, hints) = run(&[Ok(CLEAN)]);
    assert_eq!(r, Ok((CLEAN.to_string(), Outcome { regenerations: 0, fell_back: false })));
    assert_eq!(hints, vec![None]);
}

#[test]
fn a_leak_is_regenerated_with_the_hint() {
    let (r, hints) = run(&[Ok(LEAK), Ok(CLEAN)]);
    assert_eq!(r, Ok((CLEAN.to_string(), Outcome { regenerations: 1, fell_back: false })));
    assert!(hints[0].is_none());
    assert!(hints[1].as_deref().is_some_and(|h| h.contains("previous")));
}

#[test]
fn a_persistent_leak_falls_back_after_the_cap() {
    let (r, hints) = run(&[Ok(LEAK)]);
    assert_eq!(r, Ok(("FALLBACK".to_string(), Outcome { regenerations: REGEN_CAP, fell_back: true })));
    assert_eq!(hints.len(), 1 + REGEN_CAP);
}

#[test]
fn a_failed_regeneration_falls_back_rather_than_sending_the_leak() {
    let (r, _) = run(&[Ok(LEAK), Err(())]);
    assert_eq!(r, Ok(("FALLBACK".to_string(), Outcome { regenerations: 1, fell_back: true })));
}

#[test]
fn a_failed_first_call_is_an_error_as_before() {
    let (r, hints) = run(&[Err(())]);
    assert_eq!(r, Err(()));
    assert_eq!(hints.len(), 1);
}

#[test]
fn the_outcome_names_its_action() {
    assert_eq!(Outcome { regenerations: 0, fell_back: false }.action(), "checked");
    assert_eq!(Outcome { regenerations: 1, fell_back: false }.action(), "regenerated");
    assert_eq!(Outcome { regenerations: 2, fell_back: true }.action(), "fell_back");
}

/// Every persona's own fallback line passes its own gate — the line sent after the cap must not
/// itself be a leak.
#[test]
fn every_story_fallback_is_clean_under_its_own_gate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![root.join("demo/ep1-en.json")];
    if let Ok(d) = std::fs::read_dir(root.join("demo/personas")) {
        files.extend(d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")));
    }
    assert!(files.len() > 1, "no personas found");
    for f in files {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let ns = vitals_sce::reveal_gate::nodes(&v);
        let g = Gate::new(&ns);
        let fb = vitals_sce::reveal_gate::fallback(&v);
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        assert!(g.check(fb, &Default::default()).is_empty(), "{name}: the fallback line leaks");
        // And every held-back node is reachable: something a learner can type earns it.
        for n in ns.iter().filter(|n| n.reveal == Reveal::OnDirectAsk) {
            assert!(n.keywords.iter().any(|k| !k.trim().is_empty()),
                "{name}/{}: a held-back node no question can earn", n.id);
        }
    }
}
