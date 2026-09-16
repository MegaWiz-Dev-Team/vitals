//! What this factory has sent, so a re-run after a crash queues nobody twice and nobody's face is
//! on the ward twice.
//!
//! The door is already content-addressed — the same pack pushed twice is one patient — so the
//! ledger is the factory's half of the same promise, and the half the door cannot keep: the queue
//! is not published pack by pack, so who is *waiting* is known only here. A pack is `unseen`
//! from the moment it is sent until the board shows her; then it carries her patient id; then,
//! when the board says she went home or died, it is `closed` and her face is free again.
//!
//! Resending an unseen pack is safe and is what a tick does first: the door answers `duplicates`
//! if she is still queued and `queued` if the queue lost her, and either is right.

use crate::door::WardView;
use crate::pool::Person;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use vitals_web::ward::{Pack, Persona};

/// One pack, as it left here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sent {
    pub case: String,
    /// `<ISO3>-<index>`, the person.
    pub key: String,
    pub name: String,
    pub country: String,
    pub sex: String,
    pub age: u16,
    pub endemic: bool,
    /// Her `stable` picture, if the pack carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable: Option<String>,
    pub sent_at: u64,
    /// Which ward it went to, so a ledger is never read against another host's board.
    pub ward: String,
    /// Set when the board first shows her.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patient_id: Option<u64>,
    /// Set when the board shows she has left. Her face is free from then on.
    #[serde(default)]
    pub closed: bool,
    /// Set once her waiting pack carries the 256 px siblings — the door that takes them accepted.
    #[serde(default)]
    pub variants_sent: bool,
}

impl Sent {
    pub fn new(case: &str, who: &Person, age: u16, endemic: bool, stable: Option<String>, sent_at: u64, ward: &str) -> Sent {
        Sent {
            case: case.into(),
            key: who.key.clone(),
            name: who.name.clone(),
            country: who.country.clone(),
            sex: who.sex.letter().into(),
            age,
            endemic,
            stable,
            sent_at,
            ward: ward.into(),
            patient_id: None,
            closed: false,
            variants_sent: false,
        }
    }

    /// The pack again, exactly as it was sent — so its address at the door is the same.
    pub fn to_pack(&self) -> Pack {
        Pack {
            case: self.case.clone(),
            persona: Persona { name: self.name.clone(), age: self.age, country: self.country.clone(), sex: self.sex.clone() },
            portrait: self.stable.iter().map(|u| ("stable".to_string(), u.clone())).collect(),
            endemic: self.endemic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Ledger {
    /// By the door's pack id (`vitals_web::ward_chain::pack_id`).
    pub sent: BTreeMap<String, Sent>,
}

impl Ledger {
    pub fn parse(json: &str) -> Result<Ledger, String> {
        serde_json::from_str(json).map_err(|e| format!("factory-ledger.json: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialises")
    }

    pub fn load(path: &Path) -> Result<Ledger, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ledger::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        crate::manifest::write_atomically(path, &self.to_json())
    }

    /// Sent, and not yet on the board.
    pub fn unseen(&self) -> Vec<(&String, &Sent)> {
        self.sent.iter().filter(|(_, s)| s.patient_id.is_none()).collect()
    }

    /// Learn from the board: an unseen pack whose person, case and age are now on a bed gets her
    /// patient id; a pack whose patient has left is closed. Returns what changed, in words.
    pub fn reconcile(&mut self, ward: &WardView) -> Vec<String> {
        let mut notes = Vec::new();
        let mut claimed: BTreeSet<u64> = self.sent.values().filter_map(|s| s.patient_id).collect();
        for (id, s) in self.sent.iter_mut() {
            if s.patient_id.is_none() {
                let hit = ward.patients.iter().find(|p| {
                    !claimed.contains(&p.patient_id)
                        && p.name.as_deref() == Some(&s.name)
                        && p.country.as_deref() == Some(&s.country)
                        && p.case.as_deref() == Some(&s.case)
                        && p.age == Some(s.age)
                });
                if let Some(p) = hit {
                    s.patient_id = Some(p.patient_id);
                    claimed.insert(p.patient_id);
                    notes.push(format!("{} ({}) is patient {} on the board — pack {}", s.name, s.case, p.patient_id, &id[..12.min(id.len())]));
                }
            }
            if let Some(pid) = s.patient_id {
                if !s.closed {
                    if let Some(p) = ward.patients.iter().find(|p| p.patient_id == pid) {
                        if !p.is_open() {
                            s.closed = true;
                            notes.push(format!("{} ({}) has left — {}; her face is free", s.name, pid, p.state));
                        }
                    }
                }
            }
        }
        notes
    }

    /// The people whose face is on the ward right now, or waiting to be: every unseen pack, every
    /// sent pack whose patient is still in a bed, and anyone in a bed this ledger never sent.
    pub fn busy_keys(&self, ward: &WardView, pool: &[Person]) -> BTreeSet<String> {
        let mut busy: BTreeSet<String> = self
            .sent
            .values()
            .filter(|s| s.patient_id.is_none() || !s.closed)
            .filter(|s| match s.patient_id {
                None => true,
                Some(pid) => ward.patients.iter().any(|p| p.patient_id == pid && p.is_open()),
            })
            .map(|s| s.key.clone())
            .collect();
        for p in ward.open() {
            if let (Some(name), Some(country)) = (&p.name, &p.country) {
                if let Some(who) = crate::pool::person_for(pool, name, country) {
                    busy.insert(who.key.clone());
                }
            }
        }
        busy
    }
}
