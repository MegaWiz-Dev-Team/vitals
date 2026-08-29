//! "Deterministic, re-derivable by anyone" — checked from outside the process.
//!
//! Every anchored run names its scenario by sha256 and nothing else. That is the right binding,
//! and it was worth nothing to an outsider: the hash named a file that existed only on our disk,
//! only in its newest form. A leaf from before an edit pointed at bytes that no longer existed
//! anywhere a stranger could reach.
//!
//! So the property under test is not "the endpoint returns 200". It is: **for every hash this
//! deployment is willing to publish, the bytes it hands back hash to that hash** — computed here,
//! in the test, with an independent sha256 over the response body, exactly as a reviewer with
//! `curl` and `sha256sum` would do it.
//!
//! ## And the second property, which is worth more than the first
//!
//! A scenario file is the answer key: every intervention id, every matcher keyword, every
//! `(HARM)` the author wrote beside a wrong turn, the thresholds that decide the outcome, and a
//! `_note` that names the diagnosis. The first cut of this endpoint resolved through the *live*
//! shelf as well as the archive, so a candidate could open a station, read `sce_hash` off their
//! own view, and fetch the whole mark sheet while the clock ran. Every seal on `/api/marks` and
//! `/api/debrief` was one unauthenticated GET away from meaning nothing.
//!
//! **A hash that names a case anyone can still sit is refused.** Retirement is what makes a case
//! publishable, and the season's files are all on the shelf *and* all in the archive — so
//! "serve from the archive" is not by itself the fix, and the fixture below has to retire a case
//! to have anything to serve at all.

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    state: PathBuf,
    /// The scenario root this server was given, when it is a fixture of our own making.
    root: Option<PathBuf>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
        if let Some(r) = &self.root {
            let _ = std::fs::remove_dir_all(r);
        }
    }
}

fn scratch(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("vitals-rederive-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

impl Server {
    fn start() -> Server {
        Server::spawn(scratch("plain"), None, &[])
    }

    /// The server, pointed at `root` as its scenario root, with `extra` environment on top.
    fn spawn(state: PathBuf, root: Option<PathBuf>, extra: &[(&str, &str)]) -> Server {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vitals-web"));
        cmd.env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped());
        if let Some(r) = &root {
            cmd.env("VITALS_SCENARIOS", r);
        }
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let mut me = Server { child, port: 0, state, root };
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

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    for e in std::fs::read_dir(from).unwrap_or_else(|e| panic!("{}: {e}", from.display())) {
        let e = e.expect("dir entry");
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().expect("file type").is_dir() {
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap_or_else(|err| panic!("{}: {err}", src.display()));
        }
    }
}

/// A deployment with one case **retired**: everything the repository ships, minus one station
/// file, so its archived hash names a case nobody can sit any more.
///
/// This is the only way to exercise a 200 at all. Every scenario in the season is simultaneously
/// on the shelf and in the archive, so a live deployment publishes nothing — which is the
/// behaviour, not a gap in it.
struct Retired {
    server: Server,
    /// The hash of the case taken off the shelf, and its bytes.
    hash: String,
    text: String,
    /// Hashes still on the shelf: every one of these must be refused.
    live: Vec<String>,
}

const RETIRE: &str = "demo/stations/osce-d4.sce.json";

fn retired_fixture() -> Retired {
    let root = scratch("root");
    copy_tree(&repo().join("demo"), &root.join("demo"));
    copy_tree(&repo().join("conformance"), &root.join("conformance"));

    let gone = root.join(RETIRE);
    let text = std::fs::read_to_string(&gone).expect(RETIRE);
    let hash = sha256_hex(text.as_bytes());
    std::fs::remove_file(&gone).expect("retire the case");

    let mut live = Vec::new();
    for dir in ["demo/stations", "demo/scenarios"] {
        for e in std::fs::read_dir(root.join(dir)).expect("a shelf") {
            let p = e.expect("dir entry").path();
            if p.extension().is_some_and(|x| x == "json") {
                live.push(sha256_hex(&std::fs::read(&p).expect("read")));
            }
        }
    }
    live.push(sha256_hex(&std::fs::read(root.join("conformance/sce-anaphylaxis-ep1.json")).expect("ep1")));

    let server = Server::spawn(scratch("retired"), Some(root), &[]);
    Retired { server, hash, text, live }
}

/// What `sha256sum` on the reply must say, for a version this deployment will publish.
#[test]
fn a_retired_scenario_comes_back_and_hashes_to_what_was_asked_for() {
    let f = retired_fixture();
    let (code, ct, body) = f.server.get(&format!("/api/sce/{}", f.hash));
    assert_eq!(code, 200, "{} is retired and archived, and the server would not serve it: {body}", f.hash);
    assert!(ct.contains("application/json"), "{} came back as {ct:?}", f.hash);
    assert_eq!(
        sha256_hex(body.as_bytes()),
        f.hash,
        "the reply hashes to something else — a verifier would blame the chain"
    );
    assert_eq!(body, f.text, "the bytes are not the file that was retired");
    // And it is the scenario, not a stub that happens to hash right.
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("the reply is JSON");
    assert!(parsed["states"].is_array(), "the retired case came back without states");
}

/// **The leak.** Every hash that names a case someone can still sit is refused — including the
/// twelve stations, whose files sit in the archive as well as on the shelf, which is why
/// "resolve through the archive" is not on its own a fix.
#[test]
fn nothing_a_candidate_can_still_sit_is_published() {
    let f = retired_fixture();
    assert!(f.live.len() >= 16, "the shelf stopped being found: {:?}", f.live.len());
    for hash in &f.live {
        let (code, _, body) = f.server.get(&format!("/api/sce/{hash}"));
        assert_eq!(code, 404, "{hash} is playable and the server published it");
        assert!(
            body.contains("active use"),
            "{hash} was refused without saying why — a verifier holding this leaf will go \
             looking for a bug in the chain: {body}"
        );
        assert!(!body.contains("interventions"), "the refusal carried the file: {body}");
    }
}

/// The same property against the deployment as shipped, with no fixture in the way: the case a
/// candidate is playing *right now* names a hash the server will not resolve.
#[test]
fn the_case_you_are_playing_is_refused_by_the_hash_it_reports() {
    let s = Server::start();
    for ep in ["ep1", "osce-a", "osce-c", "osce-d3", "ep5"] {
        let (_, _, body) = s.get(&format!("/api/new?ep={ep}"));
        let v: serde_json::Value = serde_json::from_str(&body).expect("a view");
        let hash = v["view"]["sce_hash"].as_str().unwrap_or_else(|| panic!("{ep}: no sce_hash in the view"));
        let (code, _, sce) = s.get(&format!("/api/sce/{hash}"));
        assert_eq!(code, 404, "{ep} handed over its own answer key at {hash}");
        assert!(sce.contains("active use"), "{ep}: the refusal does not explain itself: {sce}");
        // The three things the leak was actually worth to a candidate.
        for needle in ["HARM", "any_kw", "_note"] {
            assert!(!sce.contains(needle), "{ep}: the refusal still leaks {needle}: {sce}");
        }
    }
}

/// The INDEX is a map from hash to the path it was archived from. Every row has to name bytes
/// that hash to it — checked on disk, because whether the *server* will publish a row is a
/// question about the shelf, not about the archive.
///
/// It is a list of **versions**, not of cases, so it is longer than the season and gets longer
/// every time a case is re-issued. What has to hold is that nothing is missing: every file on the
/// shelf is indexed, and every row resolves.
#[test]
fn every_row_of_the_index_names_the_bytes_it_says_it_does() {
    let dir = repo().join("conformance/sce-archive");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("INDEX.json")).expect("INDEX.json"))
            .expect("INDEX.json parses");
    let rows = index.as_array().expect("INDEX.json is a list");
    let indexed: std::collections::BTreeSet<String> =
        rows.iter().filter_map(|r| r["sce_hash"].as_str().map(str::to_string)).collect();
    assert_eq!(indexed.len(), rows.len(), "a hash is indexed twice");
    // Every case in the season, at its current bytes. A row per version on top of that is the
    // archive doing its job, so the count itself is not pinned.
    for dir in ["demo/stations", "demo/scenarios"] {
        for e in std::fs::read_dir(repo().join(dir)).expect("a shelf") {
            let f = e.expect("dir entry").path();
            if f.extension().is_some_and(|x| x == "json") {
                let h = sha256_hex(&std::fs::read(&f).expect("read"));
                assert!(indexed.contains(&h), "{} is on the shelf and not in INDEX.json", f.display());
            }
        }
    }
    assert!(rows.len() >= 17, "the index lost rows: {}", rows.len());
    for row in rows {
        let hash = row["sce_hash"].as_str().expect("a row with no hash");
        let from = row["path"].as_str().unwrap_or("?");
        let bytes = std::fs::read(dir.join(format!("{hash}.json")))
            .unwrap_or_else(|e| panic!("{from} ({hash}) is indexed and not in the archive: {e}"));
        assert_eq!(sha256_hex(&bytes), hash, "{from} is archived under the wrong name");
        assert_eq!(row["bytes"].as_u64(), Some(bytes.len() as u64), "{from} is not the length the index recorded");
    }
}

/// A hash nobody has ever produced is a 404, not a guess and not a 500 — and it says something
/// different from a hash that is merely being withheld.
#[test]
fn an_unknown_hash_is_a_flat_404() {
    let s = Server::start();
    for miss in ["f".repeat(64), "0".repeat(64), "deadbeef".repeat(8)] {
        let (code, _, body) = s.get(&format!("/api/sce/{miss}"));
        assert_eq!(code, 404, "{miss} was answered with something other than 404");
        assert!(body.contains("no scenario with that hash"), "{miss}: {body}");
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
/// gets bytes that still hash to what they pasted. Asked of a retired case, because a live one
/// is refused in either case.
#[test]
fn an_upper_case_hash_is_the_same_hash() {
    let f = retired_fixture();
    let (code, _, body) = f.server.get(&format!("/api/sce/{}", f.hash.to_uppercase()));
    assert_eq!(code, 200, "an upper-case hash 404s");
    assert_eq!(sha256_hex(body.as_bytes()), f.hash);
}

/// Ungated. A proof whose inputs need a token is not a proof anyone can check.
#[test]
fn the_endpoint_needs_no_token_even_when_the_bay_has_one() {
    let root = scratch("root-tok");
    copy_tree(&repo().join("demo"), &root.join("demo"));
    copy_tree(&repo().join("conformance"), &root.join("conformance"));
    let gone = root.join(RETIRE);
    let text = std::fs::read_to_string(&gone).expect(RETIRE);
    let hash = sha256_hex(text.as_bytes());
    std::fs::remove_file(&gone).expect("retire the case");

    let s = Server::spawn(scratch("tok"), Some(root), &[("VITALS_TOKEN", "not-for-you")]);
    let (code, _, body) = s.get(&format!("/api/sce/{hash}"));
    assert_eq!(code, 200, "a token-protected bay refuses to hand over a scenario it anchored");
    assert_eq!(sha256_hex(body.as_bytes()), hash);
}
