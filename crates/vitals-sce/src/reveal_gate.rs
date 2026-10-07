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

use crate::text::fold;
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
    /// The author's words for asking about this line. A question containing any of them earns
    /// it — see [`earned`].
    pub keywords: Vec<String>,
}

/// How many times a leaking reply is regenerated, with the hint, before the story's fallback
/// line is sent instead. Two is what `bench_p2` measured; the served path and the bench read
/// this one constant so they cannot drift apart.
pub const REGEN_CAP: usize = 2;

/// What the patient says when she cannot answer, if her story does not say.
pub const DEFAULT_FALLBACK: &str = "I can't really talk any more.";

/// A story's dialogue nodes, as the gate needs them. The one reader of a persona's `dialogue`
/// for the gate — the bench and the server both build from here.
pub fn nodes(story: &serde_json::Value) -> Vec<Node> {
    story["dialogue"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|n| Node {
                    id: n["id"].as_str().unwrap_or("").to_string(),
                    reveal: match n["reveal"].as_str().unwrap_or("on_ask") {
                        "volunteered" => Reveal::Volunteered,
                        "on_direct_ask" => Reveal::OnDirectAsk,
                        _ => Reveal::OnAsk,
                    },
                    text: n["patient"].as_str().unwrap_or("").to_string(),
                    keywords: n["keywords"]
                        .as_array()
                        .map(|k| k.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The story's own safe line — what the learner hears when every regeneration still leaked. It
/// is the case author's sentence for "she cannot answer", so by construction it reveals nothing.
pub fn fallback(story: &serde_json::Value) -> &str {
    story["fallback"].as_str().unwrap_or(DEFAULT_FALLBACK)
}

/// The node ids a learner has earned, given every question they have asked this run.
///
/// **The first reader of dialogue `keywords`.** Every persona has carried them since it was
/// written, and nothing read them until the gate needed to know what had been asked. A node is
/// earned once any question contains any of its keywords, compared the way intervention keywords
/// are — both sides through [`fold`] (NFKC, lower-cased), then a substring test — so a question
/// typed through an IME earns what the same question typed on a US keyboard earns.
///
/// `asks` must include **the question being answered right now**. A learner who asks "any
/// medicines?" has earned the medicines line in this very reply; leaving the current question out
/// would make the gate refuse the one answer the learner just did the work for.
///
/// An empty keyword earns nothing: `contains("")` is true of every question, and a stray blank in
/// a case file must not open every gate in it.
pub fn earned<S: AsRef<str>>(nodes: &[Node], asks: &[S]) -> HashSet<String> {
    let asks: Vec<String> = asks.iter().map(|a| fold(a.as_ref())).collect();
    nodes
        .iter()
        .filter(|n| {
            n.keywords.iter().map(|k| fold(k)).filter(|k| !k.trim().is_empty()).any(|k| {
                asks.iter().any(|a| a.contains(k.as_str()))
            })
        })
        .map(|n| n.id.clone())
        .collect()
}

/// What the gate did with one reply. Counts only — never the reply or what it leaked — so it is
/// safe to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Model calls made after the first, each carrying a [`retry_hint`].
    pub regenerations: usize,
    /// The cap was reached, or a regeneration failed, and the fallback line was sent instead.
    pub fell_back: bool,
}

impl Outcome {
    /// `checked` (first reply was clean), `regenerated` (a regeneration was clean) or
    /// `fell_back`.
    pub fn action(&self) -> &'static str {
        match (self.fell_back, self.regenerations) {
            (true, _) => "fell_back",
            (false, 0) => "checked",
            (false, _) => "regenerated",
        }
    }
}

/// One reply, gated: ask, check, regenerate with the hint up to [`REGEN_CAP`], then fall back.
///
/// `say` is the model call, given the hint for a regeneration (`None` on the first attempt). An
/// error on the first attempt is returned as is — the learner got no reply, as before the gate.
/// An error on a regeneration falls back instead: a reply already exists and it leaked, so the
/// safe line is the only thing left to send.
pub fn guard<E>(
    gate: &Gate,
    earned: &HashSet<String>,
    fallback: &str,
    mut say: impl FnMut(Option<&str>) -> Result<String, E>,
) -> Result<(String, Outcome), E> {
    let mut reply = say(None)?;
    let mut out = Outcome { regenerations: 0, fell_back: false };
    loop {
        let v = gate.check(&reply, earned);
        if v.is_empty() {
            return Ok((reply, out));
        }
        if out.regenerations >= REGEN_CAP {
            break;
        }
        out.regenerations += 1;
        match say(retry_hint(&v).as_deref()) {
            Ok(r) => reply = r,
            Err(_) => break,
        }
    }
    out.fell_back = true;
    Ok((fallback.to_string(), out))
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
    fold(s)
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
