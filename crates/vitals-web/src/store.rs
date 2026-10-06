//! A directory of JSON files, one per record.
//!
//! Sessions lived in a `HashMap` and died with the process — a deploy, a crash, or an OOM took
//! every run in progress with it, mid-resuscitation. That is also what stops the server from
//! running as more than one replica: a player's next request has to reach the same process that
//! holds their patient, or the patient is gone.
//!
//! No database, because there is nothing here a database would do better yet: records are small,
//! keyed, and written by one process. What matters is that a write is atomic — a half-written
//! session is worse than a missing one, since the missing one is at least detectable.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;
use std::path::PathBuf;

pub struct Store {
    backend: Backend,
    tokens: Tokens,
}

/// When to go back for a new access token: 80% of its life, or five minutes before it ends,
/// whichever comes first.
///
/// Both halves earn their place. The percentage keeps a long token from being held to its last
/// second; the five minutes keep a short one from being held at all, because a token that expires
/// in the middle of a request is a request that fails for no reason the caller can act on.
/// How long a looked-up address is kept. Firestore's addresses do not move in minutes.
const DNS_KEEP: std::time::Duration = std::time::Duration::from_secs(300);

/// Each host's addresses, and when they were looked up.
type Kept = std::sync::Mutex<std::collections::BTreeMap<String, (std::time::Instant, Vec<std::net::SocketAddr>)>>;

/// The addresses each host resolved to, and when.
fn kept_dns() -> &'static Kept {
    static KEPT: std::sync::OnceLock<Kept> = std::sync::OnceLock::new();
    KEPT.get_or_init(Default::default)
}

/// **A host is looked up once every few minutes, not once a call.**
///
/// `ureq::get` builds a new agent each time, so every Firestore call resolved its host again. On
/// 4 Oct, passes stalled in bursts of a few minutes on store calls of exactly 15.0 s, all of which
/// answered in the end, with the token fast. A lookup that stalls costs one call per five minutes
/// here, and it says so in the log.
struct KeptDns;

impl ureq::Resolver for KeptDns {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<std::net::SocketAddr>> {
        if let Some((at, addrs)) = kept_dns().lock().unwrap_or_else(|e| e.into_inner()).get(netloc) {
            if at.elapsed() < DNS_KEEP {
                return Ok(addrs.clone());
            }
        }
        let asked = std::time::Instant::now();
        let addrs: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(netloc)?.collect();
        let took = asked.elapsed();
        if took >= std::time::Duration::from_secs(1) {
            eprintln!("store      slow dns · {netloc} · {:.1}s", took.as_secs_f64());
        }
        if !addrs.is_empty() {
            kept_dns().lock().unwrap_or_else(|e| e.into_inner()).insert(netloc.to_string(), (std::time::Instant::now(), addrs.clone()));
        }
        Ok(addrs)
    }
}

/// The store's one HTTP agent. It looks hosts up through [`KeptDns`], gives each attempt to open a
/// connection [`CONNECT_WITHIN`], and, as before, keeps no connection between calls: a write is
/// never sent down a socket the server may have closed.
fn http() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new().resolver(KeptDns).timeout_connect(CONNECT_WITHIN).max_idle_connections(0).build()
    })
}

/// How long one attempt to open a connection may take.
///
/// On 4 Oct the store's stalls were all exactly 15.0 s and all answered in the end, after DNS was
/// kept (00080) — a connection opened slowly, as when a lost SYN is sent again at 1, 3, 7 and 15 s.
/// Three seconds and a fresh attempt beats waiting for the fourth resend.
pub const CONNECT_WITHIN: std::time::Duration = std::time::Duration::from_secs(3);

/// Attempts a call gets to connect. Five times three seconds is the fifteen it replaces, at worst.
pub const CONNECT_TRIES: usize = 5;

/// `run` again while it fails to connect, up to `tries` times in all, and how many it took.
///
/// A connection that never opened has sent nothing, so a write is as safe to repeat as a read.
/// Anything else (a status, a reset after sending) is returned as it is.
pub fn retry_connect<T, E>(tries: usize, failed_to_connect: impl Fn(&E) -> bool, mut run: impl FnMut() -> Result<T, E>) -> (Result<T, E>, usize) {
    let mut n = 0;
    loop {
        n += 1;
        let r = run();
        match &r {
            Err(e) if n < tries && failed_to_connect(e) => continue,
            _ => return (r, n),
        }
    }
}

/// Did this call fail before a connection opened?
///
/// Two shapes. ConnectionFailed: no address answered in time. And Io with a TimedOut source: when
/// the connect deadline runs out between addresses, ureq returns the deadline's own io timeout ("timed
/// out reading response") as kind Io. This agent sets no read or overall timeout, so a timeout can
/// only be that deadline, met before anything was sent. Seven listings stopped short on 4–5 Oct for
/// want of the second shape.
#[allow(clippy::borrowed_box)] // the store's errors are boxed (see `with_token`); this reads one where it is
fn failed_to_connect(e: &Box<ureq::Error>) -> bool {
    let ureq::Error::Transport(t) = &**e else { return false };
    match t.kind() {
        ureq::ErrorKind::ConnectionFailed => true,
        ureq::ErrorKind::Io => std::error::Error::source(t)
            .and_then(|s| s.downcast_ref::<io::Error>())
            .is_some_and(|io| io.kind() == io::ErrorKind::TimedOut),
        _ => false,
    }
}

/// One store call slow enough to explain a slow pass.
///
/// On 4 Oct, passes stalled for ~15.0 s on one patient at a time, in steps that share nothing but
/// this store (repair, reap, beds). The chain calls timed themselves and were not it. A call this
/// slow is named here by its document path, with the token's time apart from the call's.
pub const SLOW_STORE: std::time::Duration = std::time::Duration::from_secs(3);

/// The log line for one store call that took [`SLOW_STORE`] or longer in all, and `None` under it.
/// The path is what follows `/documents/`. It carries no secret: the token travels in a header.
pub fn slow_store_line(url: &str, token: std::time::Duration, call: std::time::Duration, outcome: &str) -> Option<String> {
    (token + call >= SLOW_STORE).then(|| {
        let path = url.split_once("/documents/").map_or(url, |(_, p)| p);
        format!("slow firestore · {path} · token {:.1}s · call {:.1}s · {outcome}", token.as_secs_f64(), call.as_secs_f64())
    })
}

fn note_slow_store(url: &str, token: std::time::Duration, call: std::time::Duration, outcome: &str) {
    if let Some(line) = slow_store_line(url, token, call, outcome) {
        eprintln!("store      {line}");
    }
}

pub fn refresh_after(expires_in: u64) -> std::time::Duration {
    let eighty = expires_in.saturating_mul(4) / 5;
    let five_early = expires_in.saturating_sub(300);
    std::time::Duration::from_secs(eighty.min(five_early))
}

/// The access token this store is using, and when to stop using it.
///
/// One fetch per window rather than one per call. Every Firestore operation used to ask the
/// metadata server first — five call sites, no cache — which on a boot that restores sessions in
/// series is the difference between two minutes and two seconds.
#[derive(Default)]
pub struct Tokens {
    held: std::sync::Mutex<Option<(String, std::time::Instant)>>,
}

impl Tokens {
    /// The token, fetching one only if there is none or the one in hand is near its end.
    ///
    /// The lock is held across the fetch on purpose: it is the cheapest way to say "one in flight
    /// at a time", and the alternative — a thundering herd of metadata requests on a cold instance
    /// — is the thing being fixed. A fetch that fails is not remembered, or one bad second would
    /// become an hour of a store that cannot read.
    pub fn get(
        &self,
        fetch: impl FnOnce() -> Result<(String, u64), String>,
    ) -> Result<String, String> {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((token, until)) = held.as_ref() {
            if std::time::Instant::now() < *until {
                return Ok(token.clone());
            }
        }
        let (token, expires_in) = fetch()?;
        *held = Some((token.clone(), std::time::Instant::now() + refresh_after(expires_in)));
        Ok(token)
    }

    /// Forget it. Called when the store is told the token is no good — a 401 and nothing else.
    pub fn invalidate(&self) {
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Where records actually live.
///
/// Files work in the cluster, where a volume outlives the pod. They do not work on Cloud Run: a
/// container there has no disk that survives a request, and two instances share nothing. Firestore
/// is what the rest of this company already reaches for, over REST, keeping each record as a
/// single JSON string — the same convention `embla-cloud` uses.
#[derive(Debug, Clone)]
pub enum Backend {
    Disk { root: PathBuf },
    Firestore { base: String },
}

impl Backend {
    /// Firestore when a project is configured, files otherwise. Nothing else decides this —
    /// a server that silently picked the wrong one would look like data loss.
    pub fn from_env(project: Option<&str>, root: &str) -> Backend {
        match project.filter(|p| !p.is_empty()) {
            Some(p) => {
                let db = std::env::var("VITALS_FIRESTORE_DB").unwrap_or_else(|_| "(default)".into());
                Backend::Firestore {
                    base: format!(
                        "https://firestore.googleapis.com/v1/projects/{p}/databases/{db}/documents"
                    ),
                }
            }
            None => Backend::Disk { root: PathBuf::from(root) },
        }
    }

    /// The address of one record, or `None` when the key is not a name. Keys reach this from the
    /// network and become a path segment either way — a slash would quietly address a different
    /// collection, and the failure would read as missing data rather than a bad key.
    pub fn doc_path(&self, kind: &str, key: &str) -> Option<String> {
        let key = safe(key)?;
        Some(match self {
            Backend::Firestore { base } => format!("{base}/{kind}/{key}"),
            Backend::Disk { root } => root.join(kind).join(format!("{key}.json")).display().to_string(),
        })
    }

    pub fn describe(&self) -> String {
        match self {
            Backend::Disk { root } => root.display().to_string(),
            Backend::Firestore { base } => {
                base.split("/projects/").nth(1).map(|t| format!("firestore:{}", t.split('/').next().unwrap_or("?")))
                    .unwrap_or_else(|| "firestore".into())
            }
        }
    }
}

/// Keys reach this from the network. A key is a file name, so anything that is not plainly a name
/// is refused rather than sanitised — sanitising invents a key the caller did not ask for, and
/// two callers can be sanitised onto the same one.
/// Would this store take that key?
///
/// Public because a caller with an id of its own — a case from somebody else's library — needs to
/// know before it tries, so a name this store cannot file becomes the caller's filing problem
/// rather than an outage at write time.
pub fn is_safe_key(key: &str) -> bool {
    safe(key).is_some()
}

fn safe(key: &str) -> Option<&str> {
    let ok = !key.is_empty()
        && key.len() <= 64
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then_some(key)
}

/// What losing a record costs.
///
/// Every record in this store is JSON behind the same six methods, which makes them look alike.
/// They are not. A session is one learner's run in progress; the tree is the leaf list every
/// Merkle proof this server ever issued is rebuilt from. The root is anchored on chain and
/// survives anything, but the *path* to a leaf is not on chain — it is here. Expire this list
/// and the anchor stays, provably meaningless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Safe to expire. Losing it costs one learner one run.
    Ephemeral,
    /// Never expired by age.
    Durable,
}

/// Classify a kind. Anything unrecognised is durable.
///
/// The default is the point. A kind added later reaches `sweep` before anyone remembers to
/// classify it; defaulting to expiry makes that oversight delete data, while defaulting to
/// keeping makes it use disk — which gets noticed, and can be fixed after the fact.
/// How many expired documents one sweep may delete.
///
/// Bounded because this runs at boot, and boot is where a request waits. Two hundred is a few
/// seconds of deletes at Firestore's latency and clears a day of staging's sessions in one pass;
/// what it does not clear, the next boot does.
const SWEEP_AT_MOST: usize = 200;

pub fn class_of(kind: &str) -> Class {
    match kind {
        // Both spellings: the server stores runs under "sessions", and the mismatch with the
        // shorthand here meant the 24-hour sweep classified them durable and never deleted
        // one — the startup line reported "0 expired" forever and read as a quiet server.
        "sess" | "sessions" => Class::Ephemeral,
        _ => Class::Durable,
    }
}

/// A Firestore timestamp as a unix second. `None` for anything that is not one.
///
/// `2026-09-18T01:02:03.456789Z` — fixed width to the second, with a fraction Firestore adds and
/// this drops. Written out rather than pulled in: the one thing needed from a date library here is
/// twenty lines, and a record is never deleted on a timestamp that failed to parse.
pub fn unix_from_rfc3339(t: &str) -> Option<i64> {
    let b = t.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |from: usize, to: usize| t.get(from..to)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from the civil epoch — Howard Hinnant's algorithm, the same arithmetic `ward::at_slot`
    // runs in the other direction to print one.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Which of these documents are old enough to delete — at most `limit`, longest-dead first.
///
/// The bound is not a detail. A sweep that deletes everything it finds at boot is the next two
/// minutes in somebody's first request, which is the thing being fixed two commits up. A document
/// with no timestamp is never chosen: deleting on a guess is how the only copy of something goes.
pub fn stale_docs(
    docs: &[(String, Option<i64>)],
    now: i64,
    max_age: std::time::Duration,
    limit: usize,
) -> Vec<String> {
    let cutoff = now - max_age.as_secs() as i64;
    let mut old: Vec<(&String, i64)> = docs
        .iter()
        .filter_map(|(name, at)| at.filter(|t| *t < cutoff).map(|t| (name, t)))
        .collect();
    old.sort_by_key(|(_, t)| *t);
    old.into_iter().take(limit).map(|(name, _)| name.clone()).collect()
}

/// Where this deployment's leaf list lives.
///
/// The tree used to sit at the constant key `tree/current`, which says nothing about who filled
/// it or which chain it was anchored to. Two servers sharing a store therefore shared the list —
/// and the list is what every Merkle proof is rebuilt from, so the anchor survives on chain while
/// nothing can be proven against it. The same defect was fixed on chain this morning, where the
/// tree *account* was addressed by a globally-guessable id; this is one layer down.
///
/// Keyed on all three things that make a tree a different tree: the relay that funds it (the
/// on-chain tree is seeded on that same key), the program, and the cluster. The RPC url is what
/// separates devnet from mainnet, because one relay key can legitimately serve both while their
/// leaves must never land in one list.
///
/// Deterministic, because a server restarting has to find the tree it was filling. A fresh empty
/// one would silently drop the ability to prove everything anchored before the restart.
pub fn tree_key(relay: &str, program: &str, rpc: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"vitals.tree.v1\n");
    for part in [relay, program, rpc] {
        h.update(part.as_bytes());
        h.update(b"\n");
    }
    let d = h.finalize();
    // Readable prefix so a document can be matched to its relay by eye, then enough hash to make
    // the whole thing unique.
    format!("{}-{}", &relay[..8.min(relay.len())], hex16(&d))
}

fn hex16(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

impl Store {
    pub fn open(root: PathBuf) -> io::Result<Store> {
        let project = std::env::var("GOOGLE_CLOUD_PROJECT")
            .or_else(|_| std::env::var("VITALS_GCP_PROJECT"))
            .ok();
        Store::with(Backend::from_env(project.as_deref(), &root.display().to_string()))
    }

    pub fn with(backend: Backend) -> io::Result<Store> {
        if let Backend::Disk { root } = &backend {
            std::fs::create_dir_all(root)?;
        }
        Ok(Store { backend, tokens: Tokens::default() })
    }

    /// A record as Firestore wants it: one JSON string in one field.
    ///
    /// The typed representation would need a mapping per struct and would break every time one of
    /// them gained a field. What is stored here is opaque to Firestore on purpose — the only
    /// reader is this program.
    pub fn wrap(json: &str) -> serde_json::Value {
        serde_json::json!({ "fields": { "json": { "stringValue": json } } })
    }

    /// The other direction. `None` for anything this program did not write, which is skipped
    /// rather than fatal — one unreadable record must not stop the server serving the rest.
    pub fn unwrap(doc: &serde_json::Value) -> Option<String> {
        doc.get("fields")?.get("json")?.get("stringValue")?.as_str().map(str::to_string)
    }

    fn root_dir(&self) -> Option<&PathBuf> {
        match &self.backend {
            Backend::Disk { root } => Some(root),
            Backend::Firestore { .. } => None,
        }
    }

    fn dir(&self, kind: &str) -> PathBuf {
        self.root_dir().expect("disk backend").join(kind)
    }

    fn path(&self, kind: &str, key: &str) -> Option<PathBuf> {
        Some(self.dir(kind).join(format!("{}.json", safe(key)?)))
    }

    /// Write via a temp file and rename. `rename` within a directory is atomic, so a reader sees
    /// either the previous record or the new one, never half of the new one.
    /// A short-lived access token for Firestore.
    ///
    /// On Cloud Run the metadata server hands one over with no credentials configured anywhere,
    /// which is the whole point of running there. `GOOGLE_ACCESS_TOKEN` overrides it so the same
    /// binary can be pointed at Firestore from a laptop.
    fn token(&self) -> Result<String, String> {
        if let Ok(t) = std::env::var("GOOGLE_ACCESS_TOKEN") {
            if !t.is_empty() {
                return Ok(t);
            }
        }
        self.tokens.get(|| {
            let r = http().get(
                "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token",
            )
            .set("Metadata-Flavor", "Google")
            .timeout(std::time::Duration::from_secs(5))
            .call()
            .map_err(|e| e.to_string())?;
            let v: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
            let token = v["access_token"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "no access_token".to_string())?;
            // What the metadata server says it is good for. A response without it is treated as
            // the shortest useful life rather than as for ever.
            let expires_in = v["expires_in"].as_u64().unwrap_or(600);
            Ok((token, expires_in))
        })
    }

    /// Run a Firestore call with the held token, and give it exactly one more go with a fresh one
    /// if the answer is 401.
    ///
    /// 401 is the single failure a new token can fix, and a token can go stale between the moment
    /// it is checked and the moment it is used. Anything else — a 404, a 500, a socket that dies —
    /// is not made truer by asking twice.
    ///
    /// The closure's error is boxed: `ureq::Error` is 272 bytes and every `Ok` would otherwise be
    /// carried in a slot that size (clippy 1.98's `result_large_err`). The caller pays one
    /// `map_err(Box::new)` and the 401 branch reads through the box.
    fn with_token<T>(
        &self,
        url: &str,
        mut run: impl FnMut(&str) -> Result<T, Box<ureq::Error>>,
    ) -> Result<T, String> {
        let asked = std::time::Instant::now();
        let token = self.token();
        let token_took = asked.elapsed();
        let token = match token {
            Ok(t) => t,
            Err(e) => {
                note_slow_store(url, token_took, std::time::Duration::ZERO, &format!("token: {e}"));
                return Err(e);
            }
        };
        let called = std::time::Instant::now();
        let (first, tries) = retry_connect(CONNECT_TRIES, failed_to_connect, || run(&token));
        if tries > 1 {
            let path = url.split_once("/documents/").map_or(url, |(_, p)| p);
            eprintln!("store      connect tried {tries} times · {path} · {:.1}s", called.elapsed().as_secs_f64());
        }
        let out = match first {
            Ok(v) => Ok(v),
            Err(e) if matches!(*e, ureq::Error::Status(401, _)) => {
                self.tokens.invalidate();
                let token = self.token()?;
                run(&token).map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        };
        let outcome = match &out { Ok(_) => "ok".to_string(), Err(e) => e.clone() };
        note_slow_store(url, token_took, called.elapsed(), &outcome);
        out
    }

    fn fs_put(&self, url: &str, body: serde_json::Value) -> Result<(), String> {
        // PATCH creates or replaces. POST would refuse the second write to the same id.
        self.with_token(url, |tok| {
            http().patch(url)
                .set("Authorization", &format!("Bearer {tok}"))
                .send_json(body.clone())
                .map(|_| ())
                .map_err(Box::new)
        })
    }

    fn fs_get(&self, url: &str) -> Option<serde_json::Value> {
        self.with_token(url, |tok| {
            http().get(url).set("Authorization", &format!("Bearer {tok}")).call().map_err(Box::new)
        })
            .ok()?
            .into_json()
            .ok()
    }

    pub fn put<T: Serialize>(&self, kind: &str, key: &str, v: &T) -> io::Result<()> {
        let bad = || io::Error::new(io::ErrorKind::InvalidInput, "unsafe key");
        match &self.backend {
            Backend::Firestore { .. } => {
                let url = self.backend.doc_path(kind, key).ok_or_else(bad)?;
                let body = Store::wrap(&serde_json::to_string(v)?);
                self.fs_put(&url, body)
                    .map_err(io::Error::other)
            }
            Backend::Disk { .. } => {
                let path = self.path(kind, key).ok_or_else(bad)?;
                std::fs::create_dir_all(self.dir(kind))?;
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, serde_json::to_vec(v)?)?;
                std::fs::rename(&tmp, &path)
            }
        }
    }

    pub fn get<T: DeserializeOwned>(&self, kind: &str, key: &str) -> Option<T> {
        match &self.backend {
            Backend::Firestore { .. } => {
                let url = self.backend.doc_path(kind, key)?;
                let doc = self.fs_get(&url)?;
                serde_json::from_str(&Store::unwrap(&doc)?).ok()
            }
            Backend::Disk { .. } => {
                let bytes = std::fs::read(self.path(kind, key)?).ok()?;
                serde_json::from_slice(&bytes).ok()
            }
        }
    }

    pub fn del(&self, kind: &str, key: &str) {
        match &self.backend {
            Backend::Firestore { .. } => {
                if let (Some(url), Ok(tok)) = (self.backend.doc_path(kind, key), self.token()) {
                    let _ = http().delete(&url).set("Authorization", &format!("Bearer {tok}")).call();
                }
            }
            Backend::Disk { .. } => {
                if let Some(p) = self.path(kind, key) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }

    /// Everything under `kind` that still parses. A record written by an older build that no
    /// longer deserialises is skipped, not fatal — one unreadable session must not stop the
    /// server from serving the rest.
    /// Every record under `kind` that still parses, from either backend.
    pub fn list<T: DeserializeOwned>(&self, kind: &str) -> Vec<(String, T)> {
        if let Backend::Firestore { base } = &self.backend {
            let mut out = Vec::new();
            let mut page = String::new();
            // Firestore pages at 300 by default. A server that silently read the first page and
            // called it "every run" would resume some of them and drop the rest.
            loop {
                let url = format!("{base}/{kind}?pageSize=300{page}");
                // Through the same one-retry path as every other call: a long paged list is
                // exactly where a token can go stale halfway.
                // A page that fails ends the list — and says so. It used to end it silently, and on
                // 29 Sep 2026 a list of large blobs came back short with no trace of why.
                let r = match self.with_token(&url, |tok| {
                    http().get(&url).set("Authorization", &format!("Bearer {tok}")).call().map_err(Box::new)
                }) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("store      list {kind} stopped after {} documents: {e}", out.len());
                        break;
                    }
                };
                let v: serde_json::Value = match r.into_json() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("store      list {kind} stopped after {} documents: {e}", out.len());
                        break;
                    }
                };
                for d in v["documents"].as_array().unwrap_or(&Vec::new()) {
                    let Some(name) = d["name"].as_str().and_then(|n| n.rsplit('/').next()) else {
                        continue;
                    };
                    if let Some(parsed) = Store::unwrap(d).and_then(|j| serde_json::from_str(&j).ok()) {
                        out.push((name.to_string(), parsed));
                    }
                }
                match v["nextPageToken"].as_str().filter(|t| !t.is_empty()) {
                    Some(t) => page = format!("&pageToken={t}"),
                    None => break,
                }
            }
            return out;
        }
        self.list_disk(kind)
    }

    /// The keys under `kind`, without reading what is in them.
    ///
    /// [`Store::list`] deserialises every record, which is the right shape for restoring sessions
    /// at boot and the wrong one for a question that is only about keys. A public endpoint that
    /// has to ask "is this already filed?" on every request should not pay to parse every
    /// document beside it — and on this store a review can be a fifth of a megabyte.
    pub fn keys(&self, kind: &str) -> Result<Vec<String>, String> {
        if let Backend::Firestore { base } = &self.backend {
            let tok = self.token().map_err(|e| format!("no token for the store: {e}"))?;
            let mut out = Vec::new();
            let mut page = String::new();
            loop {
                // `mask.fieldPaths=__name__` asks Firestore for the document names and no fields,
                // so a page of keys costs a page of names rather than a page of records.
                //
                // It read `mask.fieldPaths=` — no path at all — until 16 ก.ย., and Firestore
                // answers that with 400 "Invalid empty property path string". The call has always
                // been wrong; what made it invisible was this function returning an empty list
                // for it, which is why the signature changed at the same time as the token.
                let url = format!("{base}/{kind}?pageSize=300&mask.fieldPaths=__name__{page}");
                let r = http().get(&url)
                    .set("Authorization", &format!("Bearer {tok}"))
                    .call()
                    .map_err(|e| format!("listing {kind}: {e}"))?;
                let v: serde_json::Value = r
                    .into_json()
                    .map_err(|e| format!("listing {kind}: the answer was not JSON: {e}"))?;
                for d in v["documents"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(name) = d["name"].as_str().and_then(|n| n.rsplit('/').next()) {
                        out.push(name.to_string());
                    }
                }
                match v["nextPageToken"].as_str().filter(|t| !t.is_empty()) {
                    Some(t) => page = format!("&pageToken={t}"),
                    None => break,
                }
            }
            return Ok(out);
        }
        // A directory that does not exist yet is an empty collection, and that is a fact rather
        // than a failure: nothing has been written under this kind. Anything else — a permission,
        // a broken disk — is reported.
        let rd = match std::fs::read_dir(self.dir(kind)) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("listing {kind}: {e}")),
        };
        Ok(rd
            .flatten()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .collect())
    }

    fn list_disk<T: DeserializeOwned>(&self, kind: &str) -> Vec<(String, T)> {
        let Ok(rd) = std::fs::read_dir(self.dir(kind)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Some(key) = p.file_stem().and_then(|s| s.to_str()) else { continue };
            if let Some(v) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
                out.push((key.to_string(), v));
            }
        }
        out
    }

    /// Drop records older than `max_age`. Abandoned runs are the common case — someone closes the
    /// tab mid-case — and without a sweep the directory only grows.
    /// Drop records older than `max_age`.
    ///
    /// Only the disk backend can do this cheaply — Firestore would need a timestamp field and a
    /// query, and abandoned runs there are small and cost nothing to leave. Returning zero is
    /// honest: nothing was swept.
    /// Delete records of `kind` older than `max_age`, and report how many went.
    ///
    /// Refuses durable kinds outright rather than trusting the caller to only pass expiring
    /// ones. The call site that matters passes a constant today, but the signature takes a
    /// string, and the cost of the wrong string is losing every proof this server can issue.
    /// Returns zero on Firestore too — expiring by age there needs a stored timestamp and a
    /// query, and reporting a number it did not delete would be worse than doing nothing.
    pub fn sweep(&self, kind: &str, max_age: std::time::Duration) -> usize {
        if class_of(kind) == Class::Durable {
            return 0;
        }
        match &self.backend {
            Backend::Firestore { .. } => self.sweep_firestore(kind, max_age, SWEEP_AT_MOST),
            Backend::Disk { .. } => self.sweep_disk(kind, max_age),
        }
    }

    /// The same sweep, where the records are documents rather than files.
    ///
    /// This returned 0 without looking for as long as the Firestore backend has existed — and
    /// Firestore is the only backend Cloud Run uses, so nothing was ever swept in the two places
    /// that run all day. A session that restored cleanly stayed for ever; only the ones the boot
    /// restore *failed* on were ever deleted, by the code that failed on them.
    ///
    /// One page, at most `limit` deletions, longest-dead first: the next boot takes the next batch.
    /// A sweep that clears everything it finds is the next two-minute boot, which is the fault this
    /// sits next to.
    fn sweep_firestore(&self, kind: &str, max_age: std::time::Duration, limit: usize) -> usize {
        let Backend::Firestore { base } = &self.backend else { return 0 };
        let url = format!("{base}/{kind}?pageSize=300");
        let Ok(r) = self.with_token(&url, |tok| {
            http().get(&url).set("Authorization", &format!("Bearer {tok}")).call().map_err(Box::new)
        }) else {
            return 0;
        };
        let Ok(v): Result<serde_json::Value, _> = r.into_json() else { return 0 };
        let empty = Vec::new();
        let docs: Vec<(String, Option<i64>)> = v["documents"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|d| {
                let name = d["name"].as_str()?.rsplit('/').next()?.to_string();
                // Firestore's own record of when it last wrote the document — the disk sweep reads
                // mtime for exactly this.
                Some((name, d["updateTime"].as_str().and_then(unix_from_rfc3339)))
            })
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut gone = 0;
        for key in stale_docs(&docs, now, max_age, limit) {
            self.del(kind, &key);
            gone += 1;
        }
        gone
    }

    fn sweep_disk(&self, kind: &str, max_age: std::time::Duration) -> usize {
        let Ok(rd) = std::fs::read_dir(self.dir(kind)) else { return 0 };
        let mut n = 0;
        for e in rd.flatten() {
            let stale = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t.elapsed().map(|d| d > max_age).unwrap_or(false))
                .unwrap_or(false);
            if stale && std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// What to print at startup. A server that does not say where it is keeping things is a
    /// server nobody can debug.
    pub fn describe(&self) -> String {
        self.backend.describe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 00081 gave each connect three seconds. With several addresses, ureq splits that deadline
    /// between them, and when it runs out mid-loop it returns the deadline's own io timeout ("timed
    /// out reading response") through `From<io::Error>`, as kind Io, not as ConnectionFailed. That
    /// was never retried, and seven `ward_case` listings stopped short on 4–5 Oct. This store's agent
    /// sets no read or overall timeout, so a timeout can only come from that deadline, before
    /// anything was sent. Any other io failure is still not retried.
    #[test]
    fn a_connect_that_runs_out_of_time_on_any_address_counts_as_failed_to_connect() {
        let deadline = ureq::Error::from(io::Error::new(io::ErrorKind::TimedOut, "timed out reading response"));
        assert!(failed_to_connect(&Box::new(deadline)), "the connect deadline, as ureq reports it");
        let reset = ureq::Error::from(io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
        assert!(!failed_to_connect(&Box::new(reset)), "a reset may come after the request went out");
    }

    /// On 4 Oct 00079 showed every stalled store call at 15.0 s with the token fast and the call
    /// slow, across reads and writes of every kind. Each `ureq::get` built a new agent, so every
    /// call looked its host up again. The store's agent now keeps an answer for a few minutes.
    /// Issue #13: the bill showed ~105k reads an hour, and nothing in the logs said which kind they
    /// were. Every read is now counted by kind (documents returned, at least one per query, which is
    /// how Firestore bills), and each pass logs the reads since the last one.
    #[test]
    fn reads_are_counted_by_kind_and_taken_once() {
        let dir = tmp("reads");
        let st = Store::open(dir.clone()).unwrap();
        let _ = take_reads();
        for k in ["a", "b", "c"] {
            st.put("count_me", k, &1u32).unwrap();
        }
        let _: Vec<(String, u32)> = st.list("count_me");
        let _: Option<u32> = st.get("count_me", "a");
        let _: Option<u32> = st.get("count_me_too", "nobody");
        let got = take_reads();
        assert!(got.contains(&("count_me".to_string(), 4)), "three listed and one got: {got:?}");
        assert!(got.contains(&("count_me_too".to_string(), 1)), "a miss is still a read: {got:?}");
        assert!(take_reads().iter().all(|(k, _)| !k.starts_with("count_me")), "taken once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_host_is_looked_up_once_and_kept() {
        use ureq::Resolver;
        let first = KeptDns.resolve("127.0.0.1:443").unwrap();
        let again = KeptDns.resolve("127.0.0.1:443").unwrap();
        assert_eq!(first, again);
        let kept = kept_dns().lock().unwrap_or_else(|e| e.into_inner());
        assert!(kept.get("127.0.0.1:443").is_some_and(|(at, a)| at.elapsed() < DNS_KEEP && a == &first));
    }

    fn tmp(name: &str) -> PathBuf {
        // Per-process: a fixed path let a second `cargo test` run against this checkout
        // delete this one's directory mid-write. See `tests/durability.rs`.
        let p = std::env::temp_dir().join(format!("vitals-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn round_trips() {
        let s = Store::open(tmp("rt")).unwrap();
        s.put("sess", "s1", &vec![1u8, 2, 3]).unwrap();
        assert_eq!(s.get::<Vec<u8>>("sess", "s1"), Some(vec![1, 2, 3]));
        s.del("sess", "s1");
        assert_eq!(s.get::<Vec<u8>>("sess", "s1"), None);
    }

    /// The key is a path component and it comes from the network.
    #[test]
    fn refuses_keys_that_are_not_names() {
        let s = Store::open(tmp("keys")).unwrap();
        for bad in ["../../etc/passwd", "a/b", "", "s1.json", "a b", &"x".repeat(65)] {
            assert!(s.put("sess", bad, &1u8).is_err(), "accepted {bad:?}");
            assert_eq!(s.get::<u8>("sess", bad), None, "read back {bad:?}");
        }
    }

    /// The kind the server actually stores runs under must be sweepable — the mismatch with
    /// the shorthand is exactly the bug this pins down.
    #[test]
    fn the_kind_the_server_uses_for_runs_is_ephemeral() {
        assert_eq!(class_of("sessions"), Class::Ephemeral);
        assert_eq!(class_of("sess"), Class::Ephemeral);
        assert_eq!(class_of("tree"), Class::Durable);
        assert_eq!(class_of("meter"), Class::Durable);
    }

    /// Keys without records: the same set `list` would report, and nothing parsed to get it.
    /// A record this build cannot deserialise still has a key, and a caller asking "is this key
    /// taken?" has to be told yes — otherwise it writes over it.
    #[test]
    fn keys_are_listed_without_reading_the_records() {
        let s = Store::open(tmp("keys-list")).unwrap();
        s.put("sess", "one", &7u8).unwrap();
        s.put("sess", "two", &8u8).unwrap();
        std::fs::write(s.dir("sess").join("broken.json"), b"{not json").unwrap();
        std::fs::write(s.dir("sess").join("notes.txt"), b"ignored").unwrap();
        let mut got = s.keys("sess").expect("a disk store lists");
        got.sort();
        assert_eq!(got, vec!["broken", "one", "two"]);
        assert!(s.keys("nothing-here").expect("a kind never written is empty, not broken").is_empty());
    }

    #[test]
    fn lists_what_survives_and_skips_what_does_not() {
        let s = Store::open(tmp("list")).unwrap();
        s.put("sess", "good", &7u8).unwrap();
        std::fs::write(s.dir("sess").join("broken.json"), b"{not json").unwrap();
        let got: Vec<(String, u8)> = s.list("sess");
        assert_eq!(got, vec![("good".to_string(), 7)]);
    }
}
