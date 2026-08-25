//! Refusing a reply that hands over an unearned reveal.
//!
//! A patient's story marks some facts `on_direct_ask`: she states them only when asked about that
//! exact thing. A language model volunteers them anyway — it is trying to be helpful — and a
//! candidate who never asked gets the history for free, which is the part of the station being
//! assessed. This gate reads a proposed reply against what the learner has actually earned and
//! flags any unearned reveal, so the caller can make the model try again.
//!
//! It matches on contiguous character windows of the scripted line rather than on words, for the
//! same reason the tape quantises and canonicalises: the markets this is built for — Thai,
//! Japanese, Korean, Chinese — do not put spaces between words, so a word-based match would miss
//! a leak in exactly the languages that matter most. Everything is normalised through the tape's
//! own NFKC `canon` first, so a full-width or oddly spaced reply cannot slip a leak past a byte
//! comparison.
//!
//! Deliberately narrow: it protects the *timing* of a reveal, nothing else. There is no hidden
//! diagnosis to name here — inferring it is the learner's job — so this is not the embla gate,
//! which guards a hidden answer. It shares only the shape.

use crate::text::canon;
use std::collections::HashSet;

/// When the patient will say a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reveal {
    /// Said unprompted — the chief complaint.
    Volunteered,
    /// Said when asked at all. The normal reward for taking a history.
    OnAsk,
    /// Said only when asked about this exact thing. The one the gate protects.
    OnDirectAsk,
}

/// One line of her story.
pub struct Node {
    pub id: String,
    pub reveal: Reveal,
    pub text: String,
}

/// A reply gave away something it should not have.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Violation {
    /// The reply revealed this `on_direct_ask` fact the learner had not earned.
    UnearnedReveal(String),
}

/// Windows shorter than this are common enough across unrelated English that they would false-
/// positive; longer and a natural paraphrase slips through. Twelve characters is a distinctive
/// phrase in every target language without being a whole sentence.
const WINDOW: usize = 12;

/// NFKC (through the tape's canon) → case-folded → punctuation and spacing dropped. The last step
/// is what makes a character window meaningful across languages that do not delimit words.
fn normalise(s: &str) -> String {
    canon(s)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn windows(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    if cs.len() <= WINDOW {
        return if cs.is_empty() { Vec::new() } else { vec![cs.iter().collect()] };
    }
    (0..=cs.len() - WINDOW).map(|i| cs[i..i + WINDOW].iter().collect()).collect()
}

pub struct Gate {
    /// `(id, windows)` for the `on_direct_ask` nodes only — the rest are never gated.
    guarded: Vec<(String, Vec<String>)>,
}

impl Gate {
    pub fn new(nodes: &[Node]) -> Gate {
        let guarded = nodes
            .iter()
            .filter(|n| n.reveal == Reveal::OnDirectAsk)
            .map(|n| (n.id.clone(), windows(&normalise(&n.text))))
            .collect();
        Gate { guarded }
    }

    /// Which unearned reveals, if any, this reply gives away.
    ///
    /// `earned` is the set of node ids the learner has legitimately unlocked — in Vitals it comes
    /// from what they asked, which the run already records. An empty result means the reply is
    /// clear to send.
    pub fn check(&self, reply: &str, earned: &HashSet<String>) -> Vec<Violation> {
        let r = normalise(reply);
        let mut out = Vec::new();
        for (id, wins) in &self.guarded {
            if earned.contains(id) {
                continue;
            }
            if wins.iter().any(|w| r.contains(w.as_str())) {
                out.push(Violation::UnearnedReveal(id.clone()));
            }
        }
        out
    }
}

/// The constraint to add to a regeneration, given what the last reply leaked.
///
/// Lives here, not in the patient: which node leaked and what that means is the gate's knowledge,
/// so the patient stays pure model-plumbing that takes an opaque hint and appends it to the
/// system prompt. `None` when nothing leaked — the caller sends the reply unchanged.
///
/// It names the node ids, which the system prompt already maps to their scripted lines, so this
/// reveals nothing new; it only re-imposes the reveal discipline the model just broke. A blind
/// re-roll gets the same tendency back — this is what makes a regeneration actually change the
/// answer.
pub fn retry_hint(violations: &[Violation]) -> Option<String> {
    if violations.is_empty() {
        return None;
    }
    let ids: Vec<&str> = violations
        .iter()
        .map(|Violation::UnearnedReveal(id)| id.as_str())
        .collect();
    Some(format!(
        "You just volunteered something the patient reveals only when asked about it directly. \
         Do not mention {} unless the doctor asks about that specifically. Answer again, in \
         character, without it.",
        ids.join(" or "),
    ))
}
