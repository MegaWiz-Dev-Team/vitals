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
use std::path::{Path, PathBuf};

pub struct Store {
    root: PathBuf,
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

impl Store {
    pub fn open(root: PathBuf) -> io::Result<Store> {
        std::fs::create_dir_all(&root)?;
        Ok(Store { root })
    }

    fn dir(&self, kind: &str) -> PathBuf {
        self.root.join(kind)
    }

    fn path(&self, kind: &str, key: &str) -> Option<PathBuf> {
        Some(self.dir(kind).join(format!("{}.json", safe(key)?)))
    }

    /// Write via a temp file and rename. `rename` within a directory is atomic, so a reader sees
    /// either the previous record or the new one, never half of the new one.
    pub fn put<T: Serialize>(&self, kind: &str, key: &str, v: &T) -> io::Result<()> {
        let path = self
            .path(kind, key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unsafe key"))?;
        std::fs::create_dir_all(self.dir(kind))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(v)?)?;
        std::fs::rename(&tmp, &path)
    }

    pub fn get<T: DeserializeOwned>(&self, kind: &str, key: &str) -> Option<T> {
        let path = self.path(kind, key)?;
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn del(&self, kind: &str, key: &str) {
        if let Some(p) = self.path(kind, key) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Everything under `kind` that still parses. A record written by an older build that no
    /// longer deserialises is skipped, not fatal — one unreadable session must not stop the
    /// server from serving the rest.
    pub fn list<T: DeserializeOwned>(&self, kind: &str) -> Vec<(String, T)> {
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
    pub fn sweep(&self, kind: &str, max_age: std::time::Duration) -> usize {
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

    pub fn root(&self) -> &Path {
        &self.root
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
