//! "Deterministic, re-derivable by anyone" — checked from outside the process.
//!
//! Every anchored run names its scenario by sha256 and nothing else. That is the right binding,
//! and it was worth nothing to an outsider: the hash named a file that existed only on our disk,
//! only in its newest form. A leaf from before an edit pointed at bytes that no longer existed
//! anywhere a stranger could reach.
//!
//! So the property under test is not "the endpoint returns 200". It is: **for every hash this
//! deployment claims to know, the bytes it hands back hash to that hash** — computed here, in the
//! test, with an independent sha256 over the response body, exactly as a reviewer with `curl` and
//! `sha256sum` would do it.

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    state: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

impl Server {
    fn start() -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-rederive-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);

        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped())
            .spawn()
            .expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let mut me = Server { child, port: 0, state };
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(a) = line.split("http://").nth(1) {
                me.port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
                break;
            }
        }
        assert!(me.port > 0, "server never said what port it took");
        me
    }

    /// (status, content-type, body) — everything a caller checking a hash actually looks at.
    fn get(&self, path: &str) -> (u16, String, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(r) => {
                let ct = r.header("content-type").unwrap_or_default().to_string();
                (200, ct, r.into_string().unwrap_or_default())
            }
            Err(ureq::Error::Status(c, r)) => {
                let ct = r.header("content-type").unwrap_or_default().to_string();
                (c, ct, r.into_string().unwrap_or_default())
            }
            Err(e) => panic!("{url}: {e}"),
        }
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// What `sha256sum` on the reply must say, for every hash in the archive.
#[test]
fn every_archived_scenario_comes_back_and_hashes_to_what_was_asked_for() {
    let s = Server::start();
    let dir = repo().join("conformance/sce-archive");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let name = e.expect("dir entry").file_name().to_string_lossy().to_string();
        let Some(hash) = name.strip_suffix(".json") else { continue };
        if hash.len() != 64 {
            continue; // INDEX.json — a map, not an archived file
        }
        let (code, ct, body) = s.get(&format!("/api/sce/{hash}"));
        assert_eq!(code, 200, "{hash} is archived and the server would not serve it");
        assert!(ct.contains("application/json"), "{hash} came back as {ct:?}");
        assert_eq!(
            sha256_hex(body.as_bytes()),
            hash,
            "{hash} came back as bytes that hash to something else — a verifier would blame the chain"
        );
        n += 1;
    }
    assert!(n >= 17, "only {n} archived scenarios were served; the season has seventeen");
}

/// The INDEX is a map from hash to the path it was archived from. Every row has to resolve, or
/// the index is a list of promises the server cannot keep.
#[test]
fn every_row_of_the_index_resolves() {
    let s = Server::start();
    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("conformance/sce-archive/INDEX.json")).expect("INDEX.json"),
    )
    .expect("INDEX.json parses");
    let rows = index.as_array().expect("INDEX.json is a list");
    assert_eq!(rows.len(), 17, "the index is not the season any more");
    for row in rows {
        let hash = row["sce_hash"].as_str().expect("a row with no hash");
        let from = row["path"].as_str().unwrap_or("?");
        let (code, _, body) = s.get(&format!("/api/sce/{hash}"));
        assert_eq!(code, 200, "{from} ({hash}) is indexed and does not resolve");
        assert_eq!(sha256_hex(body.as_bytes()), hash, "{from} resolved to the wrong bytes");
        assert_eq!(
            row["bytes"].as_u64(),
            Some(body.len() as u64),
            "{from} is not the length the index recorded"
        );
    }
}

/// A run in play names its scenario in the view. That name has to resolve *now* — this is the
/// endpoint's real job, and the case a reviewer will try first.
#[test]
fn the_case_you_are_playing_can_be_fetched_by_the_hash_it_reports() {
    let s = Server::start();
    for ep in ["ep1", "osce-a", "ep5"] {
        let (_, _, body) = s.get(&format!("/api/new?ep={ep}"));
        let v: serde_json::Value = serde_json::from_str(&body).expect("a view");
        let hash = v["view"]["sce_hash"].as_str().unwrap_or_else(|| panic!("{ep}: no sce_hash in the view"));
        let (code, _, sce) = s.get(&format!("/api/sce/{hash}"));
        assert_eq!(code, 200, "{ep} reports a hash the server cannot resolve: {hash}");
        assert_eq!(sha256_hex(sce.as_bytes()), hash, "{ep} resolved to the wrong bytes");
        // And it is the scenario, not a stub that happens to hash right.
        let parsed: serde_json::Value = serde_json::from_str(&sce).expect("the reply is JSON");
        assert!(parsed["states"].is_array(), "{ep} came back without states");
    }
}

/// A hash nobody has ever produced is a 404, not a guess and not a 500.
#[test]
fn an_unknown_hash_is_a_flat_404() {
    let s = Server::start();
    for miss in [
        "f".repeat(64),
        "0".repeat(64),
        "deadbeef".repeat(8),
    ] {
        let (code, _, _) = s.get(&format!("/api/sce/{miss}"));
        assert_eq!(code, 404, "{miss} was answered with something other than 404");
    }
}

/// Nothing shaped like a path may be read, in any encoding, and the refusal must not depend on
/// the file being absent — `Cargo.toml` and `/etc/passwd` both exist.
#[test]
fn no_path_walks_out_of_the_archive() {
    let s = Server::start();
    for attempt in [
        "../../Cargo.toml",
        "..%2f..%2fCargo.toml",
        "%2e%2e%2f%2e%2e%2fCargo.toml",
        "../../../../../../etc/passwd",
        "..",
        ".",
        "",
        "INDEX",
        "INDEX.json",
        // A traversal padded out to exactly the length of a hash, in case the length were the
        // only check.
        "../aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        // Right length, right alphabet everywhere but one byte.
        "g".repeat(64).as_str(),
    ] {
        let (code, _, body) = s.get(&format!("/api/sce/{attempt}"));
        assert_eq!(code, 404, "{attempt:?} was not refused");
        assert!(!body.contains("[package]"), "{attempt:?} read a file off the disk");
        assert!(!body.contains("root:"), "{attempt:?} read a file off the disk");
    }
}

/// Case in the hash is not identity — a reviewer pasting an upper-case digest gets the file, and
/// gets bytes that still hash to what they pasted.
#[test]
fn an_upper_case_hash_is_the_same_hash() {
    let s = Server::start();
    let text = std::fs::read_to_string(repo().join("conformance/sce-anaphylaxis-ep1.json")).unwrap();
    let hash = sha256_hex(text.as_bytes());
    let (code, _, body) = s.get(&format!("/api/sce/{}", hash.to_uppercase()));
    assert_eq!(code, 200, "an upper-case hash 404s");
    assert_eq!(sha256_hex(body.as_bytes()), hash);
}

/// Ungated. A proof whose inputs need a token is not a proof anyone can check.
#[test]
fn the_endpoint_needs_no_token_even_when_the_bay_has_one() {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let state = std::env::temp_dir().join(format!("vitals-rederive-tok-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
        .env("VITALS_WEB_BIND", "127.0.0.1:0")
        .env("VITALS_STATE_DIR", &state)
        .env("VITALS_TOKEN", "not-for-you")
        .env_remove("VITALS_PROGRAM_ID")
        .env_remove("HEIMDALL_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .expect("start vitals-web");
    let out = child.stdout.take().expect("stdout");
    let mut port = 0u16;
    for line in BufReader::new(out).lines().map_while(Result::ok) {
        if let Some(a) = line.split("http://").nth(1) {
            port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
            break;
        }
    }
    let s = Server { child, port, state };
    assert!(s.port > 0, "server never said what port it took");
    let text = std::fs::read_to_string(repo().join("conformance/sce-anaphylaxis-ep1.json")).unwrap();
    let hash = sha256_hex(text.as_bytes());
    let (code, _, body) = s.get(&format!("/api/sce/{hash}"));
    assert_eq!(code, 200, "a token-protected bay refuses to hand over the scenario it anchored");
    assert_eq!(sha256_hex(body.as_bytes()), hash);
}
