//! The cases a pack may be built from, and who each one was written for.
//!
//! Which cases exist and what level each is are the ward's to say (`vitals_web::ward::CATALOGUE`,
//! `difficulty_of`) and are taken from there, never listed again here. So, since 949a76b, are the
//! sex and the age a station was written for: `vitals_web::ward::case_patient` is the door's own
//! reading of `demo/personas/<id>.json`, `age_band` is how far from the authored age the door
//! lets a pack sit, and a pack that contradicts either is refused at the door. The factory asks
//! those two functions and has no rule of its own.
//!
//! Where the door has nothing baked in — the four episodes today — it takes a pack at its word,
//! which makes the factory the only check. It reads, in order:
//!
//!   1. the persona file, `demo/personas/<id>.json`, parsed the way the door parses the twelve
//!      it carries, so a file that appears later is read the same way;
//!   2. a `ward` block in the scenario itself: `{"ward": {"sex": "f", "age": [30, 40]}}` — the
//!      case stating who its patient is, or a single authored age widened by `age_band`.
//!
//! A case that neither describes is **not built**. The alternative — a guess — is exactly the
//! pack nobody refuses: a woman's name on a man's presentation, or a child's disease on a woman
//! of seventy.

use std::ops::RangeInclusive;
use std::path::Path;

/// `f` or `m`, the two letters the persona pool uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    F,
    M,
}

impl Sex {
    /// One letter in either case, with the space around it forgiven. Nothing else: a word like
    /// "female" is a rendering, and rendering is not something to pattern-match a patient out of.
    pub fn parse(s: &str) -> Option<Sex> {
        match s.trim() {
            "f" | "F" => Some(Sex::F),
            "m" | "M" => Some(Sex::M),
            _ => None,
        }
    }

    /// The pool's own letter.
    pub fn letter(self) -> &'static str {
        match self {
            Sex::F => "f",
            Sex::M => "m",
        }
    }

    /// The word the portrait prompt uses.
    pub fn word(self) -> &'static str {
        match self {
            Sex::F => "woman",
            Sex::M => "man",
        }
    }
}

/// One case the factory can build a patient from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub id: String,
    pub sex: Sex,
    /// The ages a persona may be given for this case, inclusive.
    pub band: RangeInclusive<u16>,
    /// The ward's word for the level: `student`, `intern` or `resident`.
    pub difficulty: &'static str,
    /// Where the sex and the band were read from, for the log and for a reader of the pack.
    pub source: String,
}

/// A case the ward serves that the factory will not build, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unbuildable {
    pub id: String,
    pub why: String,
}

/// The whole catalogue, split into what can be built and what cannot.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub cases: Vec<Case>,
    pub unbuildable: Vec<Unbuildable>,
}

impl Catalogue {
    /// The buildable cases at one level.
    pub fn in_band(&self, difficulty: &str) -> Vec<&Case> {
        self.cases.iter().filter(|c| c.difficulty == difficulty).collect()
    }

    /// The buildable case with this id.
    pub fn get(&self, id: &str) -> Option<&Case> {
        self.cases.iter().find(|c| c.id == id)
    }
}

/// Read one case from the text of its files.
///
/// `scenario` is the case's own file; `persona` is `demo/personas/<id>.json` when there is one.
/// Pure over the texts so the rules can be tested without a repository — except that the door's
/// own baked reading (`case_patient`) wins whenever the door has one, because that is what a pack
/// is checked against.
pub fn read_case(id: &str, scenario: &str, persona: Option<&str>) -> Result<Case, Unbuildable> {
    use vitals_web::ward::{age_band, case_patient};
    let no = |why: String| Unbuildable { id: id.to_string(), why };
    let Some(difficulty) = vitals_web::ward::difficulty_of(id) else {
        return Err(no("the ward gives this case no level, so it is not one the ward serves".into()));
    };
    let file = format!("demo/personas/{id}.json");

    // 1. the door's own reading, where it has one. Nothing else may disagree with it.
    if let Some(theirs) = case_patient(id) {
        let sex = Sex::parse(&theirs.sex)
            .ok_or_else(|| no(format!("{file} gives the patient a sex the pool does not know: {}", theirs.sex)))?;
        return Ok(Case {
            id: id.into(),
            sex,
            band: age_band(theirs.age),
            difficulty,
            source: format!("{file}: patient {} {}, as the door reads it; band is the door's age_band", theirs.sex.to_uppercase(), theirs.age),
        });
    }

    // 2. a persona file the door has not baked in, read the way the door reads its own.
    if let Some(persona) = persona {
        let p: serde_json::Value = serde_json::from_str(persona).map_err(|e| no(format!("{file} is not JSON: {e}")))?;
        let patient = p.get("patient").ok_or_else(|| no(format!("{file} has no patient block")))?;
        let sex = patient
            .get("sex")
            .and_then(serde_json::Value::as_str)
            .and_then(Sex::parse)
            .ok_or_else(|| no(format!("{file} gives the patient no sex the pool knows (f or m)")))?;
        let age = patient
            .get("age")
            .and_then(serde_json::Value::as_u64)
            .and_then(|a| u16::try_from(a).ok())
            .filter(|a| vitals_web::ward_chain::AGE_RANGE.contains(a))
            .ok_or_else(|| no(format!("{file} gives the patient no age a person has")))?;
        return Ok(Case {
            id: id.into(),
            sex,
            band: age_band(age),
            difficulty,
            source: format!("{file}: patient {} {age}, read as the door reads its own; band is the door's age_band", sex.letter().to_uppercase()),
        });
    }

    // 3. the scenario's own word, for a case with no persona file.
    let sce: serde_json::Value = serde_json::from_str(scenario).map_err(|e| no(format!("the scenario is not JSON: {e}")))?;
    if let Some(block) = sce.get("ward") {
        let sex = block
            .get("sex")
            .and_then(serde_json::Value::as_str)
            .and_then(Sex::parse)
            .ok_or_else(|| no("the scenario's ward block names no sex the pool knows (f or m)".into()))?;
        let band = match block.get("age") {
            Some(serde_json::Value::Number(n)) => {
                let a = n.as_u64().and_then(|a| u16::try_from(a).ok()).ok_or_else(|| no("age is not a whole number".into()))?;
                age_band(a)
            }
            Some(serde_json::Value::Array(pair)) if pair.len() == 2 => {
                let at = |i: usize| pair[i].as_u64().and_then(|a| u16::try_from(a).ok());
                match (at(0), at(1)) {
                    (Some(lo), Some(hi)) if lo <= hi => lo..=hi,
                    _ => return Err(no("the ward block's age must be [low, high] with low <= high".into())),
                }
            }
            _ => return Err(no("the scenario's ward block names no age (a number or [low, high])".into())),
        };
        let range = vitals_web::ward_chain::AGE_RANGE;
        if !range.contains(band.start()) || !range.contains(band.end()) {
            return Err(no(format!("nobody is {}–{}", band.start(), band.end())));
        }
        return Ok(Case { id: id.into(), sex, band, difficulty, source: "the scenario's ward block (no persona file; the door takes this case at its word)".into() });
    }

    Err(no(format!(
        "no file states her sex and age — neither {file} nor a ward block in the scenario — and the door \
         would take a guess at its word"
    )))
}

/// Read the ward's whole catalogue from a checkout.
///
/// The ids are the ward's; the files are where `vitals_web::ward_chain::case_path` says the ward
/// itself reads them. A file that cannot be read is an unbuildable case, not a panic — the factory
/// runs unattended, and the log is where a missing file should show.
pub fn read_catalogue(root: &Path) -> Catalogue {
    let mut out = Catalogue::default();
    for id in vitals_web::ward::CATALOGUE {
        let sce_path = vitals_web::ward_chain::case_path(root, id);
        let scenario = match std::fs::read_to_string(&sce_path) {
            Ok(s) => s,
            Err(e) => {
                out.unbuildable.push(Unbuildable { id: id.into(), why: format!("{}: {e}", sce_path.display()) });
                continue;
            }
        };
        let persona = std::fs::read_to_string(root.join("demo/personas").join(format!("{id}.json"))).ok();
        match read_case(id, &scenario, persona.as_deref()) {
            Ok(c) => out.cases.push(c),
            Err(u) => out.unbuildable.push(u),
        }
    }
    out
}
