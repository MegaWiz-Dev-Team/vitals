//! Reading a learner's order against a scenario's keyword lists: the scenario's own rule, minus the
//! ways plain substring containment reads words the learner never wrote.
//!
//! The matcher `apply` uses (`SceState::match_intervention`) is substring containment over
//! canonical lower-case text. Three traps follow from it, all live in the bay before this module:
//!
//! 1. **An abbreviation inside a longer word.** EP1's IV-push harm keys on `iv`, which is inside
//!    "give": "give adrenaline 0.5 mg IM" gave the IV push. The same shape sits in every episode
//!    and station: `asa` (aspirin) in "nasal", `ent` in "patient", `rsi` in "persistent", `ett` in
//!    "better", `ics` in "antibiotics", `lap` in "collapsed", `lay` in "delay". A keyword that is
//!    only 1–3 letters or digits is an abbreviation, and counts only as a whole word — or its
//!    plural ("abgs", "nebs"). A few stations' short keywords are the start of a word instead
//!    ([`STEMS`]: `sat` for "saturations"), and count at the start of one.
//! 2. **A keyword inside a compound that means something else.** EP1's stand harm keys on เดิน
//!    (walk), which is inside ทางเดินหายใจ (airway): "ประเมินทางเดินหายใจ" stood the patient up, and
//!    "ok, I understand" did the same through `stand`. Thai has no spaces to find a word by, so the
//!    compounds are listed ([`PROTECTED`]), and a keyword that falls wholly inside one of them does
//!    not count there.
//! 3. **A negated order.** EP1's stand harm keys on ลุก, which is inside "ห้ามลุก" (no getting up),
//!    and its discharge keys on กลับบ้าน, which is inside "อย่าเพิ่งให้กลับบ้าน" (don't send her home
//!    yet): the learner said not to, and the case did it. "no aspirin" gave the aspirin, and the
//!    rubric credited it. A keyword the learner negated ([`negated`]) does not name an order, and
//!    does not rule an order out through `not_kw` either — "adrenaline IM, not IV" is IM. Two
//!    exceptions, both read off the scenario's own words: "hold" and "stop" also start orders
//!    ("hold pressure", "stop the bleeding"), so only a [`guarded`] order hears them; and an order
//!    whose own keywords hold a negation — "do not disturb" — is said in negations and hears none
//!    ([`said_in_negations`]).
//!
//! Ported from embla-cloud's `story::recognise` (16 Nov 2026), which found and fixed these on the
//! same EP1–EP5 files between 11 and 16 Nov 2026.
//!
//! **Recognition only.** What a phrase resolves to is written on the tape as an intervention id
//! (`Step::Act`), and replay never reads the text again, so a run played before this module
//! replays to the same leaf. A `Step::Do` carries text and replays through `apply`, which keeps the
//! substring rule untouched for exactly that reason. No scenario file changes. Everything else is
//! the engine's rule exactly: canonical (NFKC) lower-case text, `any_kw`, every group of
//! `all_groups`, none of `not_kw`, first intervention in file order wins.

use crate::schema::{Intervention, Matcher};
use crate::Sce;

/// Compounds whose meaning is not the meaning of a shorter keyword inside them. A keyword found
/// only inside one of these is not an order. Longer keywords that *are* the compound still match.
pub const PROTECTED: &[&str] = &[
    "understand",  // holds stand: "ok, I understand" stood her up
    "standard",    // holds stand: "standard monitoring"
    "outstanding", // holds stand
    "ทางเดินหายใจ",   // airway — holds เดิน (walk)
    "ทางเดินอาหาร",   // gut — holds เดิน
    "ทางเดินปัสสาวะ", // urinary tract — holds เดิน
    "เดินทาง",        // travel ("ประวัติการเดินทาง") — holds เดิน
    "ลุกลาม",         // spreading ("ผื่นลุกลาม") — holds ลุก (get up)
    "ลุกไหม้",        // burning — holds ลุก
    "ใส่ท่อปัสสาวะ",  // urinary catheter — holds ใส่ท่อ (intubate)
    "ใส่ท่อให้อาหาร", // feeding tube — holds ใส่ท่อ
];

/// Short keywords that are the start of an English word, not an abbreviation: a station's `sat` is
/// "saturations", its `neb` is "nebulised", its `eat` is "eaten". One counts at the start of a word
/// ("sats", "saturation probe"), never inside one ("unsaturated").
pub const STEMS: &[&str] = &[
    "sat", // osce-b3, osce-c exam_sats: the chip says "check the saturations"
    "neb", // osce-c2 neb_salbutamol: "nebuliser", "nebulised"
    "eat", // osce-d3 ask_exposure: "has she eaten", "eating"
];

/// The engine's canonical form, lower-cased: what the matcher compares.
///
/// Every word list here goes through it before it is compared with a learner's text. NFKC splits
/// Thai sara am (ำ → ํ + า), so a list typed as written never matches canonical text that holds ำ.
pub fn canon(s: &str) -> String {
    crate::text::canon(s).to_lowercase()
}

/// A keyword of 1–3 ASCII letters or digits: an abbreviation, matched as a whole word only.
fn is_abbreviation(k: &str) -> bool {
    (1..=3).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Where canonical keyword `k` occurs in canonical text `t` as something the learner actually
/// wrote, as byte ranges: an abbreviation only as a whole word or its plural (a [`STEMS`] entry at
/// the start of a word), never wholly inside a [`PROTECTED`] compound.
fn occurrences(t: &str, k: &str) -> Vec<(usize, usize)> {
    let mut shields: Vec<(usize, usize)> = Vec::new();
    for c in protected() {
        if c.len() > k.len() && c.contains(k) {
            shields.extend(t.match_indices(c.as_str()).map(|(j, m)| (j, j + m.len())));
        }
    }
    let abbrev = is_abbreviation(k);
    let stem = abbrev && STEMS.contains(&k);
    let word = |c: char| c.is_ascii_alphanumeric();
    t.match_indices(k)
        .map(|(i, m)| (i, i + m.len()))
        .filter(|&(i, end)| {
            if abbrev {
                let before = t[..i].chars().next_back().is_some_and(word);
                // A plural is the word: "abgs", "nebs", "rubs".
                let rest = &t[end..];
                let rest = rest.strip_prefix('s').filter(|r| !r.starts_with(word)).unwrap_or(rest);
                let after = !stem && rest.starts_with(word);
                if before || after {
                    return false;
                }
            }
            !shields.iter().any(|&(j, e)| j <= i && end <= e)
        })
        .collect()
}

/// [`PROTECTED`], canonical.
fn protected() -> &'static [String] {
    static P: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    P.get_or_init(|| PROTECTED.iter().map(|c| canon(c)).collect())
}

/// Does canonical text `t` contain canonical keyword `k` as something the learner actually wrote?
pub fn contains(t: &str, k: &str) -> bool {
    // The engine's rule: every text contains the empty string (a scenario may rely on it).
    k.is_empty() || !occurrences(t, k).is_empty()
}

/// [`contains`], counting only an occurrence the learner did not negate ([`negated`]), every
/// negation heard ([`Hears::All`]). `own` are the order's own keywords, which a negation reaches
/// across.
pub fn says(t: &str, k: &str, own: &[String]) -> bool {
    says_as(t, k, own, Hears::All)
}

fn says_as(t: &str, k: &str, own: &[String], hears: Hears) -> bool {
    k.is_empty() || occurrences(t, k).iter().any(|&(i, _)| !negated_as(t, i, own, hears))
}

/// Which negations an order hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hears {
    /// None: the order is itself said in negations ([`said_in_negations`]).
    Nothing,
    /// The plain ones ([`NEGATIONS_TH`], [`NEGATIONS_EN`]) — every other order.
    Plain,
    /// Those and "hold", "stop", "หยุด" ([`HALTS_TH`], [`HALTS_EN`]) — a [`guarded`] order.
    All,
}

/// Words that put what follows out of the order. Thai is matched anywhere — it has no spaces — and
/// [`negated`] then asks that only filler stand between the negation and the order.
const NEGATIONS_TH: &[&str] = &["ห้าม", "อย่า", "ไม่", "งด", "เลี่ยง", "ยกเลิก"];
/// English negations, matched as whole words; any word ending in "n't" is one too.
const NEGATIONS_EN: &[&str] = &["no", "not", "never", "nor", "dont", "cannot", "avoid", "without", "withhold", "cancel"];
/// Words that stop an order but also start one — "hold pressure", "stop the bleeding",
/// "หยุดเลือด". Heard only by an order that does harm or sends the patient home: "hold the
/// discharge" holds it, and "hold pressure on the wound" still presses on it.
const HALTS_TH: &[&str] = &["หยุด"];
const HALTS_EN: &[&str] = &["stop", "hold"];

/// What may stand between a negation and the order it negates: who it is for, may/should/must,
/// give/let, yet, the dose. Anything else — a reason, a finding, a comma — ends its reach, so
/// "ไม่มีหวีด ให้กลับบ้าน" (no wheeze, send her home) is still a discharge.
const FILLER_TH: &[&str] = &[
    "ให้", "ควร", "จะ", "ต้อง", "จำเป็น", "เพิ่ง", "พึ่ง", "รีบ", "ได้", "เลย", "ไป", "อีก", "นะ", "เด็ดขาด",
    "ปล่อย", "อนุญาต", "ลอง", "ทำ", "การ", "สั่ง", "ฉีด", "ใช้", "ยา", "และ", "หรือ",
    "ผู้ป่วย", "คนไข้", "คนป่วย", "น้อง", "เขา", "เธอ", "เค้า", "คุณ",
    "มก", "มก.", "มล", "มล.", "มิลลิกรัม", "ซีซี", "ลิตร",
];
const FILLER_EN: &[&str] = &[
    "let", "lets", "allow", "allowed", "permit", "make", "give", "given", "use", "inject", "administer", "send",
    "do", "be", "been", "being", "is", "are", "get", "go", "have", "need", "needs", "should", "must", "try",
    "her", "him", "them", "it", "the", "a", "an", "this", "that", "patient", "pt", "she", "he", "they",
    "to", "for", "up", "out", "of", "bed", "and", "or", "yet", "ever", "any", "just", "please", "ready",
    "safe", "fit", "mg", "mcg", "ml", "cc", "l", "g",
];

/// Where a negation an order `hears` sits in canonical text `t`, as byte ranges.
fn negations(t: &str, hears: Hears) -> Vec<(usize, usize)> {
    let (th, en): (&[&str], Vec<&str>) = match hears {
        Hears::Nothing => return Vec::new(),
        Hears::Plain => (&[], NEGATIONS_EN.to_vec()),
        Hears::All => (HALTS_TH, [NEGATIONS_EN, HALTS_EN].concat()),
    };
    // Thai negations go through canon like every other list compared with canonical text.
    static NEG_TH: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let neg_th = NEG_TH.get_or_init(|| NEGATIONS_TH.iter().map(|n| canon(n)).collect());
    let mut out: Vec<(usize, usize)> = neg_th
        .iter()
        .cloned()
        .chain(th.iter().map(|n| canon(n)))
        .flat_map(|n| t.match_indices(n.as_str()).map(|(i, m)| (i, i + m.len())).collect::<Vec<_>>())
        .collect();
    let word_char = |c: char| c.is_ascii_alphanumeric() || c == '\'' || c == '’';
    let mut i = 0;
    for w in t.split(|c: char| !word_char(c)) {
        let start = i;
        i += w.len();
        // The separator after this piece: one char, of whatever width.
        i += t[i..].chars().next().map_or(0, char::len_utf8);
        if en.contains(&w) || w.ends_with("n't") || w.ends_with("n’t") {
            out.push((start, start + w.len()));
        }
    }
    out
}

/// Is the word at byte `at` of canonical text `t` under a negation the learner wrote before it —
/// "ห้ามลุก", "อย่าเพิ่งให้กลับบ้าน", "do not let her stand", "ห้ามให้อะดรีนาลีนเข้าเส้น"? Every
/// negation counts here, "hold" and "stop" too ([`Hears::All`]).
///
/// It is when nothing but filler stands between the two: [`FILLER_TH`], [`FILLER_EN`], a dose, or
/// one of `own`, the order's own keywords — so the ห้าม in "ห้ามลุกเดิน" reaches เดิน across ลุก.
pub fn negated(t: &str, at: usize, own: &[String]) -> bool {
    negated_as(t, at, own, Hears::All)
}

fn negated_as(t: &str, at: usize, own: &[String], hears: Hears) -> bool {
    negations(t, hears).into_iter().any(|(_, end)| end <= at && filler(&t[end..at], own))
}

fn filler(gap: &str, own: &[String]) -> bool {
    gap.split_whitespace().all(|w| filler_word(w, own))
}

/// Can `w` be read, left to right, as filler pieces only? Thai runs are cut against the Thai filler
/// and the order's own Thai keywords; each run of other characters must be a filler word, a dose,
/// or carry one of the order's own keywords ("adrenaline" carries `adren`).
fn filler_word(w: &str, own: &[String]) -> bool {
    // The text is canonical, so the list must be too: NFKC splits ำ into ํ + า, and "ทำ" or
    // "จำเป็น" as typed in this file never match — "ไม่จำเป็นต้องให้กลับบ้าน" would send her home.
    static FILLER_TH_CANON: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let filler_th = FILLER_TH_CANON.get_or_init(|| FILLER_TH.iter().map(|w| canon(w)).collect());
    let thai = |c: char| ('\u{0E00}'..='\u{0E7F}').contains(&c);
    let mut reach = vec![false; w.len() + 1];
    reach[0] = true;
    for p in 0..w.len() {
        if !reach[p] || !w.is_char_boundary(p) {
            continue;
        }
        let rest = &w[p..];
        if rest.starts_with(thai) {
            for piece in filler_th.iter().chain(own.iter()).map(String::as_str) {
                if !piece.is_empty() && rest.starts_with(piece) {
                    reach[p + piece.len()] = true;
                }
            }
        } else {
            let run = rest.find(thai).map_or(rest, |q| &rest[..q]);
            let ok = FILLER_EN.contains(&run)
                || is_dose(run)
                || own.iter().any(|k| {
                    k == run
                        || (k.len() >= 4
                            && k.is_ascii()
                            && run.contains(k.as_str())
                            && run.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                });
            if ok {
                reach[p + run.len()] = true;
            }
        }
    }
    reach[w.len()]
}

/// "0.5", "1:1000", "0.5mg", "10x": a number, with a unit stuck to it or not.
fn is_dose(run: &str) -> bool {
    let n = ["mg", "mcg", "ml", "cc"].iter().find_map(|u| run.strip_suffix(u)).unwrap_or(run);
    n.chars().any(|c| c.is_ascii_digit()) && n.chars().all(|c| c.is_ascii_digit() || ".:/%x×".contains(c))
}

/// An order whose wrong reading hurts the patient or sends her home: one the scenario marks with a
/// harm, or the discharge. It hears "hold" and "stop" as negations too ([`Hears::All`]).
pub fn guarded(iv: &Intervention) -> bool {
    iv.harm.is_some() || iv.id == "discharge"
}

/// An order's own keywords — every positive one — canonical.
pub fn own_keywords(m: &Matcher) -> Vec<String> {
    m.any_kw.iter().chain(m.all_groups.iter().flatten()).map(|k| canon(k)).filter(|k| !k.is_empty()).collect()
}

/// Is this order said in negations — does one of its own keywords hold a negation it would hear?
/// EP3's "do not disturb" is the order, not against it.
pub fn said_in_negations(iv: &Intervention) -> bool {
    let heard = if guarded(iv) { Hears::All } else { Hears::Plain };
    own_keywords(&iv.matcher).iter().any(|k| !negations(k, heard).is_empty())
}

/// Which negations `iv` hears: none if it is [`said_in_negations`], "hold" and "stop" too if it is
/// [`guarded`], else the plain ones.
pub fn hears(iv: &Intervention) -> Hears {
    if said_in_negations(iv) {
        Hears::Nothing
    } else if guarded(iv) {
        Hears::All
    } else {
        Hears::Plain
    }
}

/// Does `text` name `iv`? The engine's rule with [`contains`] in place of plain containment, and
/// every keyword read as the learner said it ([`hears`]):
/// - a negated keyword does not name the order — "ห้ามลุก" is not standing her up, "no aspirin" is
///   not aspirin;
/// - a negated keyword does not rule an order out (`not_kw`) — "adrenaline 0.5 mg IM, not IV" is
///   IM. "ห้ามให้อะดรีนาลีนเข้าเส้น" names no adrenaline at all, so it is no order for either route.
pub fn names(iv: &Intervention, text: &str) -> bool {
    let m = &iv.matcher;
    let t = canon(text);
    let own = own_keywords(m);
    let heard = hears(iv);
    let said = |k: &str| says_as(&t, &canon(k), &own, heard);
    (!m.any_kw.is_empty() || !m.all_groups.is_empty())
        && (m.any_kw.is_empty() || m.any_kw.iter().any(|k| said(k)))
        && m.all_groups.iter().all(|g| g.iter().any(|k| said(k)))
        && !m.not_kw.iter().any(|k| said(k))
}

/// The first intervention, in file order, that `text` names.
pub fn first(sce: &Sce, text: &str) -> Option<String> {
    sce.interventions.iter().find(|iv| names(iv, text)).map(|iv| iv.id.clone())
}

/// May a language layer's reading stand? It translated `key`, a phrase in `text`, into an order
/// the scenario read as `id`; not if the learner negated that phrase as the order hears negations
/// ([`hears`]) — "อย่าเพิ่งให้กลับบ้าน" holds ให้กลับบ้าน, which translates to "discharge home", and
/// "ห้ามให้อะดรีนาลีน" holds อะดรีนาลีน.
pub fn alias_stands(sce: &Sce, id: &str, text: &str, key: &str) -> bool {
    match sce.interventions.iter().find(|iv| iv.id == id) {
        Some(iv) => says_as(&canon(text), &canon(key), &own_keywords(&iv.matcher), hears(iv)),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_abbreviation_counts_only_as_a_whole_word() {
        assert!(!contains("give adrenaline 0.5 mg im", "iv"));
        assert!(contains("adrenaline iv push", "iv"));
        assert!(contains("ให้adrenaline ทางiv", "iv"), "ทางiv: a Thai letter is a word boundary for an English abbreviation");
        assert!(contains("ให้ iv", "iv"));
        assert!(!contains("nasal cannula", "asa"));
        assert!(contains("asa 300 mg chewed", "asa"));
        assert!(!contains("oxygen for comfort", "or"));
        assert!(!contains("order a chest x-ray", "or"));
        assert!(contains("take him to or now", "or"));
        assert!(!contains("spo2 91%", "o2"));
        assert!(contains("o2 10 l", "o2"));
        assert!(!contains("she is better", "ett"));
        assert!(!contains("broad-spectrum antibiotics", "ics"));
        assert!(!contains("she collapsed", "lap"));
        assert!(!contains("intubate without delay", "lay"));
        assert!(!contains("iv fluids", "flu"));
    }

    #[test]
    fn an_abbreviation_takes_its_plural() {
        assert!(contains("two abgs", "abg"));
        assert!(contains("back-to-back nebs", "neb"));
        assert!(contains("friction rubs", "rub"));
        assert!(!contains("patients", "ent"), "a plural still starts a word");
        assert!(!contains("ivsomething", "iv"), "an s that runs on is not a plural");
    }

    #[test]
    fn a_stem_counts_at_the_start_of_a_word() {
        assert!(contains("check the saturations", "sat"));
        assert!(contains("sats 94%", "sat"));
        assert!(contains("nebulised salbutamol", "neb"));
        assert!(contains("has she eaten anything new", "eat"));
        assert!(!contains("unsaturated", "sat"));
        assert!(!contains("a great feat", "eat"));
        // Not a stem: an abbreviation does not start a longer word.
        assert!(!contains("enter the room", "ent"));
    }

    #[test]
    fn a_thai_keyword_inside_a_compound_is_not_that_keyword() {
        assert!(!contains(&canon("ประเมินทางเดินหายใจ"), &canon("เดิน")));
        assert!(contains(&canon("ให้ผู้ป่วยลุกเดินไปห้องน้ำ"), &canon("เดิน")));
        assert!(!contains(&canon("ผื่นลุกลามทั้งตัว"), &canon("ลุก")));
        assert!(contains(&canon("ประเมินทางเดินหายใจ"), &canon("ทางเดินหายใจ")), "the compound itself still matches");
        // Both at once: the compound shields its own occurrence, not every occurrence in the text.
        assert!(contains(&canon("ดูทางเดินหายใจแล้วให้เดิน"), &canon("เดิน")));
        assert!(!contains(&canon("ใส่ท่อปัสสาวะ"), &canon("ใส่ท่อ")), "a urinary catheter is not an intubation");
    }

    fn neg(text: &str, word: &str) -> bool {
        let t = canon(text);
        let at = t.find(&canon(word)).unwrap_or_else(|| panic!("{word} in {text}"));
        negated(&t, at, &[])
    }

    #[test]
    fn a_negation_reaches_the_order_across_filler() {
        for (text, word) in [
            ("ห้ามลุก", "ลุก"),
            ("ห้าม ลุก", "ลุก"),
            ("อย่าเพิ่งให้กลับบ้าน", "กลับบ้าน"),
            ("อย่าเพิ่ง ให้กลับบ้าน", "กลับบ้าน"),
            ("ยังไม่ควรให้ผู้ป่วยกลับบ้าน", "กลับบ้าน"),
            ("ไม่ต้องให้กลับบ้าน", "กลับบ้าน"),
            ("งดลุกจากเตียง", "ลุก"),
            ("do not let her stand up", "stand"),
            ("don't discharge her yet", "discharge"),
            ("she shouldn't be discharged", "discharge"),
            ("she is not ready for discharge", "discharge"),
            ("no walking", "walk"),
            ("avoid standing", "stand"),
            ("do not shock", "shock"),
            // ำ: NFKC writes it as ํ + า, so the filler must be canonical too.
            ("ไม่ต้องทำ ecg", "ecg"),
            ("ไม่จำเป็นต้องให้กลับบ้าน", "กลับบ้าน"),
        ] {
            assert!(neg(text, word), "{text}: {word} is negated");
        }
    }

    #[test]
    fn hold_and_stop_are_heard_only_by_a_guarded_order() {
        let t = canon("hold pressure on the wound");
        let at = t.find("pressure").unwrap();
        assert!(negated_as(&t, at, &[], Hears::All));
        assert!(!negated_as(&t, at, &[], Hears::Plain));
        assert!(!negated_as(&t, at, &[], Hears::Nothing));
        let t = canon("no pressure");
        assert!(negated_as(&t, t.find("pressure").unwrap(), &[], Hears::Plain));
        let t = canon("หยุดให้ยา");
        assert!(negated_as(&t, t.find(&canon("ยา")).unwrap(), &[], Hears::All));
        assert!(!negated_as(&t, t.find(&canon("ยา")).unwrap(), &[], Hears::Plain));
    }

    #[test]
    fn a_reason_or_a_finding_ends_the_negation() {
        for (text, word) in [
            ("ไม่มีหวีด ให้กลับบ้าน", "กลับบ้าน"),
            ("ไม่ต้องรอ ให้กลับบ้านได้", "กลับบ้าน"),
            ("ความดันไม่ต่ำแล้ว ให้กลับบ้าน", "กลับบ้าน"),
            ("no wheeze, discharge home", "discharge"),
            ("no wheeze so discharge home", "discharge"),
            ("not improving after im, give adrenaline iv", " iv"),
            ("don't forget to let her walk", "walk"),
            ("อย่าลืมให้ลุกเดิน", "ลุก"),
            ("อย่างช้าๆ ให้ลุกนั่ง", "ลุก"), // อย่าง (-ly) is not อย่า
            ("ห้ามเลือดก่อน แล้วค่อยให้ลุก", "ลุก"),
            ("let her stand up, no rush", "stand"), // the negation comes after
        ] {
            assert!(!neg(text, word), "{text}: {word} is not negated");
        }
    }

    #[test]
    fn a_negation_reaches_across_the_orders_own_words_and_its_dose() {
        let own = [canon("อะดรีนาลีน"), canon("adren")];
        let t = canon("ห้ามให้อะดรีนาลีนเข้าเส้น");
        assert!(negated(&t, t.find(&canon("เข้าเส้น")).unwrap(), &own));
        let t = canon("do not give adrenaline 0.5 mg iv");
        assert!(negated(&t, t.find(" iv").unwrap() + 1, &own));
        let t = canon("ห้ามลุกเดิน");
        let at = t.find(&canon("เดิน")).unwrap();
        assert!(negated(&t, at, &[canon("ลุก"), canon("เดิน")]), "ห้าม reaches เดิน across ลุก");
        assert!(!negated(&t, at, &[]), "…only because ลุก is the order's own word");
    }

    #[test]
    fn an_english_compound_shields_stand() {
        assert!(!contains("ok, i understand", "stand"));
        assert!(!contains("standard monitoring", "stand"));
        assert!(contains("let her stand up", "stand"));
    }

    #[test]
    fn longer_keywords_keep_substring_containment() {
        assert!(contains("isosorbide dinitrate", "nitrate"), "a dinitrate is a nitrate");
        assert!(contains("give adrenaline", "adren"));
        assert!(contains("anything", ""), "the engine's empty keyword");
    }

    #[test]
    fn canon_splits_sara_am_which_is_why_every_list_goes_through_it() {
        // NFKC writes ำ as ํ + า, so "ทำ" as typed in this file is not in canonical text. Pinned,
        // so the day `text::canon` stops splitting it this test says the extra pass can go.
        assert_ne!(canon("ทำ"), "ทำ");
        assert!(!canon("ไม่ต้องทำ ecg").contains("ทำ"));
        assert!(filler_word(&canon("ทำ"), &[]) && filler_word(&canon("จำเป็น"), &[]));
    }
}
