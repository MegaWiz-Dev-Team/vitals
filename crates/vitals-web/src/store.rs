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
pub fn class_of(kind: &str) -> Class {
    match kind {
        "sess" => Class::Ephemeral,
        _ => Class::Durable,
    }
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
        Ok(Store { backend })
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
        let r = ureq::get(
            "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token",
        )
        .set("Metadata-Flavor", "Google")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .map_err(|e| e.to_string())?;
        let v: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
        v["access_token"].as_str().map(str::to_string).ok_or_else(|| "no access_token".into())
    }

    fn fs_put(&self, url: &str, body: serde_json::Value) -> Result<(), String> {
        let tok = self.token()?;
        // PATCH creates or replaces. POST would refuse the second write to the same id.
        ureq::patch(url)
            .set("Authorization", &format!("Bearer {tok}"))
            .send_json(body)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn fs_get(&self, url: &str) -> Option<serde_json::Value> {
        let tok = self.token().ok()?;
        ureq::get(url)
            .set("Authorization", &format!("Bearer {tok}"))
            .call()
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
                    let _ = ureq::delete(&url).set("Authorization", &format!("Bearer {tok}")).call();
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
            let Ok(tok) = self.token() else { return Vec::new() };
            let mut out = Vec::new();
            let mut page = String::new();
            // Firestore pages at 300 by default. A server that silently read the first page and
            // called it "every run" would resume some of them and drop the rest.
            loop {
                let url = format!("{base}/{kind}?pageSize=300{page}");
                let Ok(r) = ureq::get(&url).set("Authorization", &format!("Bearer {tok}")).call() else {
                    break;
                };
                let Ok(v): Result<serde_json::Value, _> = r.into_json() else { break };
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
        if matches!(self.backend, Backend::Firestore { .. }) {
            return 0;
        }
        self.sweep_disk(kind, max_age)
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

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("vitals-store-{name}"));
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

    #[test]
    fn lists_what_survives_and_skips_what_does_not() {
        let s = Store::open(tmp("list")).unwrap();
        s.put("sess", "good", &7u8).unwrap();
        std::fs::write(s.dir("sess").join("broken.json"), b"{not json").unwrap();
        let got: Vec<(String, u8)> = s.list("sess");
        assert_eq!(got, vec![("good".to_string(), 7)]);
    }
}
