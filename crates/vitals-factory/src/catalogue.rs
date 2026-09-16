//! The cases a pack may be built from, and who each one was written for.
//!
//! Which cases exist and what level each is are the ward's to say (`vitals_web::ward::CATALOGUE`,
//! `difficulty_of`) and are taken from there, never listed again here. What the ward cannot say —
//! and says so, in `validate_pack` — is the sex and the age the case was written for, because
//! those live in the case's own files. They are read from two places, in order:
//!
//!   1. a `ward` block in the scenario itself: `{"ward": {"sex": "f", "age": [30, 40]}}` — the
//!      case stating who its patient is, in one place, for this purpose;
//!   2. the station's persona file, `demo/personas/<id>.json`, whose `patient` carries the sex
//!      and the one age the bay renders. The band is widened around that age by [`band_around`].
//!
//! A case that neither describes is **not built**. The alternative — a guess — is exactly the
//! pack the door cannot refuse: a woman's name on a man's presentation, or a child's disease on a
//! woman of seventy.

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

/// The oldest a child is, for the purpose of not letting a band cross into adulthood.
const LAST_CHILD_AGE: u16 = 17;

/// The band around an age a case was written with.
///
/// Wide enough that the sixty faces already made — at 28, 45 and 63 — serve every adult case in
/// the catalogue, and no wider: a quarter of the age, at least a year, at most ten, and clipped so
/// a child stays a child and an adult stays an adult. Seventy-one becomes 61–81, which a face
/// made at 63 fits; twenty-five becomes 19–31, which the same face does not.
///
/// Mechanical, and said so. A band the case states for itself (the `ward` block) always wins.
pub fn band_around(age: u16) -> RangeInclusive<u16> {
    let w = (age / 4).clamp(1, 10);
    let (lo, hi) = (age.saturating_sub(w).max(1), age.saturating_add(w));
    if age <= LAST_CHILD_AGE {
        lo..=hi.min(LAST_CHILD_AGE)
    } else {
        lo.max(LAST_CHILD_AGE + 1)..=hi.min(*vitals_web::ward_chain::AGE_RANGE.end())
    }
}

/// Read one case from the text of its files.
///
/// `scenario` is the case's own file; `persona` is `demo/personas/<id>.json` when there is one.
/// Pure over the texts so the rules can be tested without a repository.
pub fn read_case(id: &str, scenario: &str, persona: Option<&str>) -> Result<Case, Unbuildable> {
    let no = |why: String| Unbuildable { id: id.to_string(), why };
    let Some(difficulty) = vitals_web::ward::difficulty_of(id) else {
        return Err(no("the ward gives this case no level, so it is not one the ward serves".into()));
    };

    // 1. the scenario's own word.
    let sce: serde_json::Value = serde_json::from_str(scenario)
        .map_err(|e| no(format!("the scenario is not JSON: {e}")))?;
    if let Some(block) = sce.get("ward") {
        let sex = block
            .get("sex")
            .and_then(serde_json::Value::as_str)
            .and_then(Sex::parse)
            .ok_or_else(|| no("the scenario's ward block names no sex the pool knows (f or m)".into()))?;
        let band = match block.get("age") {
            Some(serde_json::Value::Number(n)) => {
                let a = n.as_u64().and_then(|a| u16::try_from(a).ok()).ok_or_else(|| no("age is not a whole number".into()))?;
                a..=a
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
        if !vitals_web::ward_chain::AGE_RANGE.contains(band.start()) || !vitals_web::ward_chain::AGE_RANGE.contains(band.end()) {
            return Err(no(format!("nobody is {}–{}", band.start(), band.end())));
        }
        return Ok(Case { id: id.into(), sex, band, difficulty, source: "the scenario's ward block".into() });
    }

    // 2. the station's persona file.
    let where_it_would_be = format!("demo/personas/{id}.json");
    let Some(persona) = persona else {
        return Err(no(format!(
            "no file states her sex and age — neither a ward block in the scenario nor {where_it_would_be}"
        )));
    };
    let p: serde_json::Value = serde_json::from_str(persona)
        .map_err(|e| no(format!("{where_it_would_be} is not JSON: {e}")))?;
    let patient = p.get("patient").ok_or_else(|| no(format!("{where_it_would_be} has no patient block")))?;
    let sex = patient
        .get("sex")
        .and_then(serde_json::Value::as_str)
        .and_then(Sex::parse)
        .ok_or_else(|| no(format!("{where_it_would_be} gives the patient no sex the pool knows (f or m)")))?;
    let age = patient
        .get("age")
        .and_then(serde_json::Value::as_u64)
        .and_then(|a| u16::try_from(a).ok())
        .filter(|a| vitals_web::ward_chain::AGE_RANGE.contains(a))
        .ok_or_else(|| no(format!("{where_it_would_be} gives the patient no age a person has")))?;
    Ok(Case {
        id: id.into(),
        sex,
        band: band_around(age),
        difficulty,
        source: format!("{where_it_would_be}: patient {} {age}, band widened around the age", sex.letter().to_uppercase()),
    })
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
