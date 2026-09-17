//! Small text tools: ids out of prose, keywords out of display names, negation in a sentence.

use std::collections::BTreeSet;

/// An ASCII identifier from a display name: `Thick and thin blood film` → `thick_and_thin_blood_film`.
/// Non-ASCII (a Thai display) yields an empty slug and the caller falls back to a numbered id.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut last_us = true;
    for ch in s.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_us = false;
        } else if !last_us {
            out.push('_');
            last_us = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    out.trim_matches('_').to_string()
}

/// Words a learner would not type to mean a specific order — kept out of matcher keywords so
/// `chest` does not hand a chest X-ray to whoever says "listen to the chest".
const STOP: &[&str] = &[
    "the", "and", "with", "for", "from", "that", "this", "than", "then", "into", "over", "under", "after",
    "before", "since", "about", "without", "within", "test", "tests", "level", "levels", "count", "profile",
    "examination", "assessment", "general", "chest", "blood", "urine", "bedside", "point", "care", "serial",
    "repeat", "including", "only", "when", "where", "which", "while", "still", "very", "more", "most",
    "less", "left", "right", "both", "sides", "first", "second", "days", "day", "hours", "hour", "week",
    "weeks", "month", "months", "year", "years", "morning", "night", "today", "yesterday", "patient",
    "history", "symptoms", "signs", "status", "present", "absent", "known", "unknown", "recent", "normal",
    "abnormal", "positive", "negative", "other", "some", "none", "not", "no", "yes", "one", "two", "three",
    "four", "five", "six", "seven", "eight", "nine", "ten", "any", "all", "per", "via", "to", "of", "in",
    "on", "at", "by", "or", "if", "is", "as", "an", "a", "x", "ml", "mg", "kg", "min", "sec", "regional",
    "exclusion", "loinc", "value", "result", "immediately", "hourly", "daily", "appearance", "findings",
    "finding", "output", "sample", "bloods", "area", "areas",
];

/// Matcher keywords for a display name: the whole phrase, then each distinctive word.
///
/// Distinctive means four letters or more, not a stop word, not a number. The phrase comes first
/// so a learner who types the order as written matches it whole; the words let a shorter order
/// land. Everything is lower-case because the engine lower-cases both sides.
pub fn keywords(display: &str) -> Vec<String> {
    let phrase = display.trim().to_lowercase();
    let mut out: Vec<String> = Vec::new();
    if phrase.len() >= 4 {
        out.push(phrase.clone());
    }
    for w in phrase.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '₂').map(str::trim) {
        let w = w.trim_matches('-');
        if w.len() < 4 || STOP.contains(&w) || w.chars().all(|c| c.is_ascii_digit()) || out.iter().any(|o| o == w) {
            continue;
        }
        out.push(w.to_string());
    }
    out
}

/// Split a plan step or a red flag into the fragments a negation applies to.
pub fn fragments(s: &str) -> Vec<String> {
    s.split([';', '.', '—', ':', '\n'])
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty())
        .collect()
}

const NEGATIONS: &[&str] = &[
    "do not", "don't", "never", "avoid", "must not", "should not", "not indicated", "not routinely", "contraindicated",
    "no ", "not be used", "stop all", "stop ", "withhold", "is not acceptable", "cannot", "not acceptable",
    "ห้าม", "หลีกเลี่ยง", "ไม่ควร", "งด",
];

/// Does this fragment forbid something?
pub fn negated(fragment: &str) -> bool {
    let f = fragment.to_lowercase();
    NEGATIONS.iter().any(|n| f.contains(n))
}

/// Seconds named by a time-critical phrase — `within 1 hour`, `within the first hour`,
/// `Hour-1`, `within 5 minutes`, `at once`, `immediately`. `None` when the sentence names no time.
pub fn time_named(s: &str) -> Option<f64> {
    let f = s.to_lowercase();
    let re_num = regex::Regex::new(r"within (?:the first )?(\d+)\s*(min|minute|minutes|h|hr|hour|hours)\b").ok()?;
    if let Some(c) = re_num.captures(&f) {
        let n: f64 = c[1].parse().ok()?;
        let unit = &c[2];
        return Some(if unit.starts_with('m') { n * 60.0 } else { n * 3600.0 });
    }
    if f.contains("within the first hour") || f.contains("hour-1") || f.contains("first hour") {
        return Some(3600.0);
    }
    if f.contains("at once") || f.contains("immediately") || f.contains("now,") || f.contains(" now") || f.contains("ทันที") {
        return Some(300.0);
    }
    None
}

/// Every string anywhere in a JSON value, with a path, for the scans that refuse a pack.
pub fn strings(v: &serde_json::Value, path: &str, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::String(s) => out.push((path.to_string(), s.clone())),
        serde_json::Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                strings(x, &format!("{path}[{i}]"), out);
            }
        }
        // Values only: the keys are the engine's vocabulary (`delta`, `flag`, `beat`), not prose,
        // and a patient who happens to be called Delta must not refuse every pack.
        serde_json::Value::Object(o) => {
            for (k, x) in o {
                strings(x, &format!("{path}.{k}"), out);
            }
        }
        _ => {}
    }
}

/// Name tokens worth scanning for: each word of the patient's name of three letters or more that
/// is not a title. Case-insensitive on the caller's side.
pub fn name_tokens(name: &str) -> BTreeSet<String> {
    const TITLES: &[&str] = &["mr", "mrs", "ms", "miss", "dr", "นาย", "นาง", "นางสาว", "ด.ช.", "ด.ญ.", "เด็กชาย", "เด็กหญิง"];
    name.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !TITLES.contains(&w.as_str()))
        .collect()
}

/// Replace the patient's name in a piece of prose with a neutral placeholder — `the patient`
/// for a Latin-script name matched as whole words, `ผู้ป่วย` for a Thai one matched as a
/// substring (Thai has no word spaces). The ward assigns its own persona; the case's opening
/// must not carry a name the persona will contradict.
pub fn scrub(text: &str, name: &str) -> String {
    let mut out = text.to_string();
    for t in name_tokens(name) {
        if t.is_ascii() {
            let mut rebuilt = String::with_capacity(out.len());
            let low = out.to_lowercase();
            let mut i = 0;
            while i < out.len() {
                let rest = &low[i..];
                if rest.starts_with(t.as_str()) {
                    let before = low[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
                    let after = low[i + t.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                    if before && after {
                        rebuilt.push_str("the patient");
                        i += t.len();
                        continue;
                    }
                }
                let ch = out[i..].chars().next().unwrap_or(' ');
                rebuilt.push(ch);
                i += ch.len_utf8();
            }
            out = rebuilt;
        } else {
            out = out.replace(t.as_str(), "ผู้ป่วย");
        }
    }
    out
}

// ── the persona placeholders ──────────────────────────────────────────────────────────
//
// The ward assigns its own persona — a name, an age within twelve years of the case's, the same
// sex — so the prose must not state the Embla patient's age or sex as facts. They become
// placeholders the ward's renderer fills: `{age}`, `{sex_word}`, `{he_she}`, `{his_her}`,
// `{him_her}`, `{himself_herself}`. A placeholder whose first letter is a capital (`{He_she}`)
// asks for a capitalised fill: it stood at the start of a sentence.
//
// Only words of the *patient's* sex are replaced. For a male patient, "she" in the text is
// somebody else (a sister, a nurse) and stays; for a female patient, "he" stays. A same-sex third
// party ("my sister says she noticed it first", spoken by a woman) is replaced too — a
// deterministic tool cannot tell them apart, and the docs say so. Only the patient's own age is
// replaced; a "4-year-old son" keeps his age.

fn keep_case(original: &str, placeholder: &str) -> String {
    if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut chars = placeholder.chars();
        let open = chars.next().unwrap_or('{');
        let first = chars.next().unwrap_or(' ').to_uppercase().collect::<String>();
        format!("{open}{first}{}", chars.as_str())
    } else {
        placeholder.to_string()
    }
}

/// `(word, placeholder)` for a male patient.
const MALE_WORDS: &[(&str, &str)] = &[
    ("man", "{sex_word}"), ("boy", "{sex_word}"), ("male", "{sex_word}"), ("gentleman", "{sex_word}"),
    ("he", "{he_she}"), ("his", "{his_her}"), ("him", "{him_her}"), ("himself", "{himself_herself}"),
];
/// `(word, placeholder)` for a female patient. `her` is decided by what follows it.
const FEMALE_WORDS: &[(&str, &str)] = &[
    ("woman", "{sex_word}"), ("girl", "{sex_word}"), ("female", "{sex_word}"), ("lady", "{sex_word}"),
    ("she", "{he_she}"), ("hers", "{his_her}"), ("herself", "{himself_herself}"),
];
/// Thai sex nouns, replaced as substrings (Thai has no word spaces). Longest first. The bare
/// noun is replaced only where the words that follow make it the patient (`ชายวัย 26 ปี`,
/// `หญิงอายุ 45 ปี`, `ชายไทย`), because `ชาย` and `หญิง` are syllables of other words too.
const THAI_MALE: &[&str] = &["ผู้ป่วยชาย", "ผู้ชาย", "เด็กชาย"];
const THAI_FEMALE: &[&str] = &["ผู้ป่วยหญิง", "ผู้หญิง", "เด็กหญิง"];
const THAI_CONTEXT: &str = "(วัย|อายุ|ไทย)";

/// Words after which `her` is the object ("carried her in"), not the possessive ("her mother").
const OBJECT_NEXT: &[&str] = &[
    "and", "or", "to", "in", "on", "at", "with", "for", "from", "by", "as", "that", "if", "when", "because",
    "but", "so", "up", "down", "out", "off", "away", "into", "onto", "over", "under", "again", "home", "back",
    "here", "there", "now", "then", "too", "also", "about", "after", "before", "until", "while", "once",
];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\''
}

/// Is this string English prose? Counted by letters: more Latin letters than letters of any
/// other script. A Thai sentence with one English drug name is Thai; an English sentence with one
/// Thai word is English; a string with no letters at all is left to the English path.
pub fn is_english(text: &str) -> bool {
    let mut latin = 0usize;
    let mut other = 0usize;
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        if c.is_ascii_alphabetic() || matches!(c, 'À'..='ÿ') {
            latin += 1;
        } else {
            other += 1;
        }
    }
    latin >= other
}

/// One sex's word table: the English `(word, placeholder)` pairs and the Thai nouns.
type SexWords = (&'static [(&'static str, &'static str)], &'static [&'static str]);

/// The patient's age and sex, with the regexes built once — a pack has hundreds of prose strings
/// and a library run has hundreds of packs.
pub struct Persona {
    age: Option<u32>,
    sex: Option<String>,
    age_en: Option<regex::Regex>,
    age_th: Option<regex::Regex>,
    thai_bare: Option<regex::Regex>,
}

impl Persona {
    pub fn new(age: Option<u32>, sex: Option<&str>) -> Persona {
        let age_en = age.map(|a| {
            regex::Regex::new(&format!(
                r"(?i)\b(?:(?:aged|age)\s+{a}\b|{a}(?:-|\s)(?:year|yr|y)s?(?:-|\s)?(?:old|o)\b|{a}\s*(?:yo|y/o|yrs|years old)\b)"
            )).expect("a literal regex compiles")
        });
        let age_th = age.map(|a| regex::Regex::new(&format!(r"(อายุ|วัย)\s*{a}\s*(ปี)?")).expect("a literal regex compiles"));
        let thai_bare = match sex {
            Some("male") => Some(regex::Regex::new(&format!("ชาย{THAI_CONTEXT}")).expect("a literal regex compiles")),
            Some("female") => Some(regex::Regex::new(&format!("หญิง{THAI_CONTEXT}")).expect("a literal regex compiles")),
            _ => None,
        };
        Persona { age, sex: sex.map(str::to_string), age_en, age_th, thai_bare }
    }

    fn words(&self) -> Option<SexWords> {
        match self.sex.as_deref() {
            Some("male") => Some((MALE_WORDS, THAI_MALE)),
            Some("female") => Some((FEMALE_WORDS, THAI_FEMALE)),
            _ => None,
        }
    }

    /// Replace the patient's age and sex words with placeholders.
    ///
    /// Only inside English prose. A placeholder is filled with an English word, and an English
    /// word inside Thai (or any other script's) prose is the wrong sheet whichever persona fills
    /// it — the ward showed exactly that. Until a translation step exists, a non-English string
    /// is left as written; the language gate refuses such cases upstream, and this is the belt
    /// to that brace for the day one gets through.
    pub fn depersonalise(&self, text: &str) -> String {
        if !is_english(text) {
            return text.to_string();
        }
        let mut out = text.to_string();
        if let Some(a) = self.age {
            let n = a.to_string();
            if let Some(re) = &self.age_en {
                out = re.replace_all(&out, |c: &regex::Captures| c[0].replace(&n, "{age}")).into_owned();
            }
            if let Some(re) = &self.age_th {
                out = re.replace_all(&out, |c: &regex::Captures| c[0].replace(&n, "{age}")).into_owned();
            }
        }
        let Some((words, thai)) = self.words() else { return out };
        for t in thai {
            out = out.replace(t, "{sex_word}");
        }
        if let Some(re) = &self.thai_bare {
            out = re.replace_all(&out, "{sex_word}$1").into_owned();
        }
        let female = self.sex.as_deref() == Some("female");
        // English, one pass over the words so a replacement is never re-read
        let mut result = String::with_capacity(out.len() + 16);
        let chars: Vec<char> = out.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if !is_word_char(chars[i]) {
                result.push(chars[i]);
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let low = word.to_lowercase();
            let placeholder = words.iter().find(|(w, _)| *w == low).map(|(_, p)| *p).or_else(|| {
                if female && low == "her" {
                    // possessive when a noun follows; object otherwise
                    let mut j = i;
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    let mut k = j;
                    while k < chars.len() && is_word_char(chars[k]) {
                        k += 1;
                    }
                    let next: String = chars[j..k].iter().collect::<String>().to_lowercase();
                    if j == i || next.is_empty() || OBJECT_NEXT.contains(&next.as_str()) || next.starts_with('{') {
                        Some("{him_her}")
                    } else {
                        Some("{his_her}")
                    }
                } else {
                    None
                }
            });
            match placeholder {
                Some(p) => result.push_str(&keep_case(&word, p)),
                None => result.push_str(&word),
            }
        }
        result
    }

    /// What a line of prose still states about the patient's age or sex. Empty means clean.
    pub fn leaks(&self, text: &str) -> Vec<String> {
        let mut leaks = Vec::new();
        if let Some(re) = &self.age_en {
            for m in re.find_iter(text) {
                leaks.push(format!("age {:?}", m.as_str()));
            }
        }
        if let Some(re) = &self.age_th {
            for m in re.find_iter(text) {
                leaks.push(format!("age {:?}", m.as_str()));
            }
        }
        let Some((words, thai)) = self.words() else { return leaks };
        for t in thai {
            if text.contains(t) {
                leaks.push(format!("sex word {t:?}"));
            }
        }
        if let Some(re) = &self.thai_bare {
            if re.is_match(text) {
                leaks.push("sex word (Thai noun)".to_string());
            }
        }
        let mut extra: Vec<&str> = words.iter().map(|(w, _)| *w).collect();
        if self.sex.as_deref() == Some("female") {
            extra.push("her");
        }
        let low = text.to_lowercase();
        for w in extra {
            let hit = low.match_indices(w).any(|(i, _)| {
                let before = low[..i].chars().next_back().is_none_or(|c| !is_word_char(c) && c != '{' && c != '_');
                let after = low[i + w.len()..].chars().next().is_none_or(|c| !is_word_char(c) && c != '}' && c != '_');
                before && after
            });
            if hit {
                leaks.push(format!("sex word {w:?}"));
            }
        }
        leaks
    }
}

/// Replace the patient's age and sex words with placeholders. `sex` is `male` | `female`.
pub fn depersonalise(text: &str, age: Option<u32>, sex: Option<&str>) -> String {
    Persona::new(age, sex).depersonalise(text)
}

/// What a line of prose still states about the patient's age or sex. Empty means clean.
pub fn prose_leaks(text: &str, age: Option<u32>, sex: Option<&str>) -> Vec<String> {
    Persona::new(age, sex).leaks(text)
}

// ── whole-word keyword matching ─────────────────────────────────────────────────────
//
// Role and gate keywords match whole words, case-folded, Unicode-aware; a multi-word keyword is
// a phrase. `stopped` does not contain the protective equipment, `nebulised` is not a
// nebuliser, a surgical mask is not surgery. Where a stem is meant, the table says so with a
// star: `transfus*` takes `transfusion` and `transfused`; `*stemi` takes `nstemi`. A keyword in
// a script without word spaces (Thai) matches as a substring, because there is no boundary to
// find.

fn is_kw_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Does `hay` contain the keyword `kw` as a whole word or phrase (or as the declared stem)?
pub fn contains_kw(hay: &str, kw: &str) -> bool {
    let kw = kw.trim();
    if kw.is_empty() {
        return false;
    }
    let prefix_ok = kw.starts_with('*');
    let suffix_ok = kw.ends_with('*');
    let core = kw.trim_matches('*').to_lowercase();
    if core.is_empty() {
        return false;
    }
    let hay = hay.to_lowercase();
    // no word spaces to find a boundary at: a substring is the best there is
    if core.chars().any(|c| c.is_alphabetic() && !c.is_ascii()) {
        return hay.contains(&core);
    }
    for (i, _) in hay.match_indices(&core) {
        let before_ok = prefix_ok || hay[..i].chars().next_back().is_none_or(|c| !is_kw_word_char(c));
        let after_ok = suffix_ok || hay[i + core.len()..].chars().next().is_none_or(|c| !is_kw_word_char(c));
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// The keyword as the engine's matcher should see it: the stem marks stripped. The engine
/// matches learner text by substring, which is what a stem wants anyway.
pub fn matcher_kw(kw: &str) -> String {
    kw.trim().trim_matches('*').to_string()
}
