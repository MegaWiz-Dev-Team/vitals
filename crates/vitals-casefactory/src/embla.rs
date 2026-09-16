//! The Embla `case.json`, read leniently, and its vital signs read out of prose.

/// The vital-sign vector a scenario starts from, plus the list of what had to be assumed.
#[derive(Debug, Clone, PartialEq)]
pub struct Vitals0 {
    pub hr: f64,
    pub sbp: f64,
    pub dbp: f64,
    pub spo2: f64,
    pub rr: f64,
    pub temp: f64,
    pub gcs: u8,
    /// Which fields were not in the case and were filled with a resting default. Carried into the
    /// pack so a reviewer sees an assumption rather than a measurement.
    pub assumed: Vec<String>,
}

pub struct Case {
    pub meta: Meta,
    pub patient: Patient,
}

pub struct Meta {
    pub id: String,
    pub difficulty: Option<String>,
    pub country: Option<String>,
    pub clinical_tier: Option<u8>,
}

pub struct Patient {
    pub age: Option<u32>,
}

impl Case {
    pub fn vitals0(&self) -> Result<Vitals0, String> {
        todo!("vitals are not read yet")
    }
}

pub fn parse_case(_json: &str) -> Result<Case, String> {
    todo!("cases are not read yet")
}
