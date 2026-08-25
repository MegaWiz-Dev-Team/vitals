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
    vec![
        Node { id: "cc".into(), reveal: Reveal::Volunteered,
               text: "I can't breathe properly".into() },
        Node { id: "allergy".into(), reveal: Reveal::OnAsk,
               text: "I'm allergic to shrimp".into() },
        Node { id: "previous".into(), reveal: Reveal::OnDirectAsk,
               text: "I've reacted before but never like this. A doctor gave me adrenaline".into() },
        Node { id: "meds".into(), reveal: Reveal::OnDirectAsk,
               text: "No medical conditions. I don't take anything regularly".into() },
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
