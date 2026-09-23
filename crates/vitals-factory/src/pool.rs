//! The people a patient can be, read from the ward's own file.
//!
//! `crates/vitals-web/data/personas.json` is the file the ward bakes into its binary
//! (`vitals_web::ward::persona_pool`). The factory reads the same file from the checkout at run
//! time — the same types, so the shapes cannot drift — which is what lets somebody add a country
//! by adding three people to one file without rebuilding this job.

use crate::sex::Sex;

/// One person, keyed the way the portrait manifest keys her: `<ISO3>-<index>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub key: String,
    pub name: String,
    pub sex: Sex,
    /// ISO 3166-1 alpha-3.
    pub country: String,
    /// What the place is called, for the portrait prompt.
    pub place: String,
}

/// The pool, in the file's own order, so `<ISO3>-<index>` means what it meant when the sixty
/// faces were made.
pub fn read_pool(json: &str) -> Result<Vec<Person>, String> {
    #[derive(serde::Deserialize)]
    struct File {
        countries: Vec<vitals_web::ward::PoolCountry>,
    }
    let f: File = serde_json::from_str(json).map_err(|e| format!("personas.json: {e}"))?;
    let mut out = Vec::new();
    for c in f.countries {
        for (i, p) in c.personas.iter().enumerate() {
            let sex = Sex::parse(&p.sex).ok_or_else(|| format!("{}: {} has a sex the pool does not use: {}", c.country, p.name, p.sex))?;
            out.push(Person {
                key: format!("{}-{i}", c.country),
                name: p.name.clone(),
                sex,
                country: c.country.clone(),
                place: if c.place.is_empty() { c.country.clone() } else { c.place.clone() },
            });
        }
    }
    Ok(out)
}

/// Who a patient on the board is, by the two fields the board publishes about her.
pub fn person_for<'a>(pool: &'a [Person], name: &str, country: &str) -> Option<&'a Person> {
    pool.iter().find(|p| p.name == name && p.country == country)
}
