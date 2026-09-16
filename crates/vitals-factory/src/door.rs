//! The ward's door, from the factory's side: what it answers, and a client that asks.
//!
//! Three requests, all against `crates/vitals-web/src/main.rs` as it is on `cwf/ward`:
//!
//!   * `GET /api/ward` — the census, the policy, the board, and (once that build ships) the
//!     queue block;
//!   * `POST /api/ward/queue` — a page of packs, behind the token;
//!   * `POST /api/ward/pack/<id>` — more of one patient's pictures, add only, same token.
//!
//! Every answer is read for what it says. The door answers a closed ward with 503 and the word
//! `closed`, which is "come back later" and never "stop building"; it answers a bad page with
//! `error` and the shape it wanted; and it never answers a push with fewer than four numbers.
//! Anything else — HTML from a proxy, a body with no fields we know — is an error here, because a
//! client that read "0 queued" out of a 502 would top the queue up against a number it invented.

use std::collections::BTreeMap;
use std::time::Duration;
use vitals_web::ward::Pack;

/// The bearer the door wants. Held in memory, spelled out in exactly one place ([`Token::bearer`]),
/// and redacted from every `Debug`.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn new(secret: String) -> Token {
        Token(secret.trim().to_string())
    }
    /// The `Authorization` header value — the one place the secret is written out.
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// The queue block `/api/ward` publishes on `cwf/ward`. Absent on builds before it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Queue {
    pub waiting: usize,
    pub beds: usize,
    /// `open` or `closed`.
    pub door: String,
}

/// One entry on the board, with the fields the factory uses. Everything about who she is may be
/// null: a patient no pack describes yet is still a patient on a bed.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct BoardPatient {
    pub patient_id: u64,
    /// `on_ward`, `on_shift`, `went_home` or `died`.
    pub state: String,
    #[serde(default)]
    pub bed: Option<usize>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub age: Option<u16>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub case: Option<String>,
    #[serde(default)]
    pub endemic: bool,
    /// The one picture to draw now, as the board publishes it.
    #[serde(default)]
    pub portrait: Option<String>,
    /// Her whole set. Absent on builds before the set shipped, which reads as empty.
    #[serde(default)]
    pub portraits: BTreeMap<String, String>,
}

impl BoardPatient {
    /// Is she in a bed? `on_shift` is `on_ward` with somebody in the room.
    pub fn is_open(&self) -> bool {
        matches!(self.state.as_str(), "on_ward" | "on_shift")
    }
}

/// `/api/ward`, as the factory reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct WardView {
    pub readable: bool,
    pub source: String,
    /// Why the ward could not be read, when `readable` is false.
    pub why: Option<String>,
    pub beds: usize,
    pub catalogue: Vec<String>,
    pub queue: Option<Queue>,
    pub patients: Vec<BoardPatient>,
}

impl WardView {
    pub fn parse(body: &str) -> Result<WardView, String> {
        let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
        if let Some(where_it_is) = v.get("the_ward_is").and_then(|s| s.as_str()) {
            return Err(format!(
                "this host is not the ward ({}); the ward is {where_it_is}",
                v.get("ward").and_then(|s| s.as_str()).unwrap_or("?")
            ));
        }
        let policy = v.get("policy").cloned().unwrap_or(serde_json::Value::Null);
        let readable = v.get("readable").and_then(|b| b.as_bool()).unwrap_or(false);
        let patients = match v.get("patients") {
            Some(p) => serde_json::from_value(p.clone()).map_err(|e| format!("patients: {e}"))?,
            None => Vec::new(),
        };
        let queue = match v.get("queue") {
            Some(q) if !q.is_null() => Some(serde_json::from_value(q.clone()).map_err(|e| format!("queue: {e}"))?),
            _ => None,
        };
        Ok(WardView {
            readable,
            source: v.get("source").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            why: v.get("why").and_then(|s| s.as_str()).map(str::to_string),
            beds: policy.get("beds").and_then(|b| b.as_u64()).map(|b| b as usize).unwrap_or(vitals_web::ward::BEDS),
            catalogue: policy
                .get("catalogue")
                .and_then(|c| c.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            queue,
            patients,
        })
    }

    /// The patients in beds.
    pub fn open(&self) -> impl Iterator<Item = &BoardPatient> {
        self.patients.iter().filter(|p| p.is_open())
    }
}

/// What the door said to a page of packs.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Queued {
    pub queued: usize,
    pub duplicates: usize,
    pub rejected: Vec<String>,
    pub depth: usize,
}

/// A push, answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Pushed {
    Queued(Queued),
    /// The ward is not open. Not a refusal of the packs — come back later.
    Closed { why: String },
    /// The door did not take the page, and this is its sentence.
    Refused { error: String },
}

impl Pushed {
    pub fn parse(status: u16, body: &str) -> Result<Pushed, String> {
        let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("HTTP {status}, not JSON: {e}"))?;
        if let Some(where_it_is) = v.get("the_ward_is").and_then(|s| s.as_str()) {
            return Err(format!("this host is not the ward; the ward is {where_it_is}"));
        }
        if v.get("door").and_then(|d| d.as_str()) == Some("closed") {
            return Ok(Pushed::Closed { why: v.get("why").and_then(|s| s.as_str()).unwrap_or("").to_string() });
        }
        if let Some(error) = v.get("error").and_then(|s| s.as_str()) {
            return Ok(Pushed::Refused { error: error.to_string() });
        }
        serde_json::from_value(v).map(Pushed::Queued).map_err(|e| format!("HTTP {status}: not the door's four numbers: {e}"))
    }
}

/// What the door said to a set of portraits.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Filled {
    pub added: usize,
    pub kept: usize,
    pub rejected: Vec<String>,
    pub states: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FillReply {
    Filled(Filled),
    Closed { why: String },
    Refused { error: String },
}

impl FillReply {
    pub fn parse(status: u16, body: &str) -> Result<FillReply, String> {
        let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("HTTP {status}, not JSON: {e}"))?;
        if let Some(where_it_is) = v.get("the_ward_is").and_then(|s| s.as_str()) {
            return Err(format!("this host is not the ward; the ward is {where_it_is}"));
        }
        if v.get("door").and_then(|d| d.as_str()) == Some("closed") {
            return Ok(FillReply::Closed { why: v.get("why").and_then(|s| s.as_str()).unwrap_or("").to_string() });
        }
        if let Some(error) = v.get("error").and_then(|s| s.as_str()) {
            return Ok(FillReply::Refused { error: error.to_string() });
        }
        serde_json::from_value(v).map(FillReply::Filled).map_err(|e| format!("HTTP {status}: not the door's answer to portraits: {e}"))
    }
}

/// The body of a push: `{"packs": [...]}`, and nothing else.
pub fn push_body(packs: &[Pack]) -> String {
    serde_json::json!({ "packs": packs }).to_string()
}

/// The body of a portrait push: `{"portrait": {state: url}}`.
pub fn fill_body(set: &BTreeMap<String, String>) -> String {
    serde_json::json!({ "portrait": set }).to_string()
}

/// The three requests, so a tick can be tested against a door that is not there.
pub trait Door {
    fn read_ward(&self) -> Result<WardView, String>;
    fn push(&self, token: &Token, packs: &[Pack]) -> Result<Pushed, String>;
    /// More of an admitted patient's pictures, by patient id. Add only.
    fn fill(&self, token: &Token, patient_id: u64, set: &BTreeMap<String, String>) -> Result<FillReply, String>;
    /// A waiting pack's pictures, by pack id (64 hex), replaced. Refused once she is in a bed.
    fn replace(&self, token: &Token, pack_id: &str, set: &BTreeMap<String, String>) -> Result<FillReply, String>;
}

/// The real door, over HTTP.
pub struct Http {
    /// `https://vitals-world-….run.app`, no trailing slash.
    pub ward: String,
    pub timeout: Duration,
}

impl Http {
    pub fn new(ward: &str) -> Http {
        Http { ward: ward.trim_end_matches('/').to_string(), timeout: Duration::from_secs(60) }
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new().timeout(self.timeout).build()
    }

    /// Status and body, whatever the status. ureq treats 4xx/5xx as `Err`; the door's 503 is an
    /// answer, so both arms are read the same way.
    fn post(&self, path: &str, token: &Token, body: &str) -> Result<(u16, String), String> {
        let r = self
            .agent()
            .post(&format!("{}{path}", self.ward))
            .set("Authorization", &token.bearer())
            .set("Content-Type", "application/json")
            .send_string(body);
        match r {
            Ok(resp) => Ok((resp.status(), resp.into_string().map_err(|e| e.to_string())?)),
            Err(ureq::Error::Status(code, resp)) => Ok((code, resp.into_string().map_err(|e| e.to_string())?)),
            Err(e) => Err(format!("POST {path}: {e}")),
        }
    }
}

impl Door for Http {
    fn read_ward(&self) -> Result<WardView, String> {
        let body = self
            .agent()
            .get(&format!("{}/api/ward", self.ward))
            .call()
            .map_err(|e| format!("GET /api/ward: {e}"))?
            .into_string()
            .map_err(|e| e.to_string())?;
        WardView::parse(&body)
    }

    fn push(&self, token: &Token, packs: &[Pack]) -> Result<Pushed, String> {
        let (status, body) = self.post("/api/ward/queue", token, &push_body(packs))?;
        Pushed::parse(status, &body)
    }

    fn fill(&self, token: &Token, patient_id: u64, set: &BTreeMap<String, String>) -> Result<FillReply, String> {
        let (status, body) = self.post(&format!("/api/ward/pack/{patient_id}"), token, &fill_body(set))?;
        FillReply::parse(status, &body)
    }

    fn replace(&self, token: &Token, pack_id: &str, set: &BTreeMap<String, String>) -> Result<FillReply, String> {
        let (status, body) = self.post(&format!("/api/ward/pack/{pack_id}"), token, &fill_body(set))?;
        FillReply::parse(status, &body)
    }
}
