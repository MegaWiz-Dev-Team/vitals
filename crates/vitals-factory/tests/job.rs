//! The job as launchd will run it, and the binary as launchd will call it.
//!
//! The plist is checked here rather than trusted: a label or an interval typed wrong is a factory
//! that never runs or runs every second, and neither shows up in `cargo test` any other way. The
//! binary is run for real against a ward that is a socket in this test, in `--dry-run`, so the
//! real HTTP client and the real argument parsing are what is exercised — and so a dry run is
//! shown to write nothing.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use vitals_factory::tools::DOOR_SECRET;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const STAGING: &str = include_str!("fixtures/ward-staging-2026-09-16.json");
const CASES: &str = include_str!("fixtures/ward-cases-2026-09-16.json");

#[test]
fn the_launchd_job_is_the_one_the_brief_names_and_carries_no_secret() {
    let path = repo_root().join("deploy/launchd/com.vitals.world-factory.plist");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let flat: String = text.split_whitespace().collect();
    assert!(flat.contains("<key>Label</key><string>com.vitals.world-factory</string>"), "the label");
    assert!(flat.contains("<key>StartInterval</key><integer>600</integer>"), "every ten minutes");
    assert!(flat.contains("vitals-factory</string>"), "runs this binary");
    assert!(flat.contains("<key>WARD</key>"), "says which ward");
    assert!(!text.contains("VITALS_TOKEN"), "the token is read from Secret Manager at run time, never written here");
    // The door's own secret, since the Forseti sweep found the doors' old token injected into the
    // public /bay.js: `vitals-door-token`, in both projects, and nothing else by that name.
    assert_eq!(DOOR_SECRET, "vitals-door-token");
    assert!(text.contains("vitals-door-token") && !text.contains("vitals-token secret"), "the plist names the secret the factory reads");
    assert!(flat.contains("<key>PATH</key>"), "mflux-generate, gcloud and cwebp are on the user's path, not launchd's");
    // macOS's own linter, when this runs on a Mac.
    if let Ok(out) = Command::new("plutil").args(["-lint", "-s"]).arg(&path).output() {
        assert!(out.status.success(), "plutil: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn without_a_ward_the_binary_says_so_and_does_nothing() {
    let out = Command::new(env!("CARGO_BIN_EXE_vitals-factory"))
        .env_remove("WARD")
        .arg("--dry-run")
        .output()
        .expect("runs");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("WARD"), "names the variable: {err}");
    let help = Command::new(env!("CARGO_BIN_EXE_vitals-factory")).arg("--help").output().expect("runs");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--dry-run"));
    // `--once` is accepted: one tick is the only mode, and the word on a launchd line or a
    // hand-typed line should not be an error.
    let once = Command::new(env!("CARGO_BIN_EXE_vitals-factory")).env_remove("WARD").args(["--once", "--dry-run"]).output().expect("runs");
    assert!(String::from_utf8_lossy(&once.stderr).contains("WARD"), "past the arguments, it is WARD that stops it");
    // And the ward's project: a token read from the wrong project is a door that says unauthorised,
    // which is what the first real tick against staging did, so there is no default.
    let no_project = Command::new(env!("CARGO_BIN_EXE_vitals-factory")).env("WARD", "https://ward.test").env_remove("VITALS_GCP_PROJECT").arg("--dry-run").output().expect("runs");
    assert!(!no_project.status.success());
    assert!(String::from_utf8_lossy(&no_project.stderr).contains("VITALS_GCP_PROJECT"), "names the variable");
    let bad = Command::new(env!("CARGO_BIN_EXE_vitals-factory")).arg("--twice").output().expect("runs");
    assert_eq!(bad.status.code(), Some(2), "an argument it does not know is refused");
}

/// A ward that is one socket answering `GET /api/ward` with the staging fixture and
/// `GET /api/ward/cases` with the case list, and refusing anything else.
fn one_shot_ward() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let mut s = stream.unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let (status, body) = if req.starts_with("GET /api/ward ") {
                ("200 OK", STAGING.to_string())
            } else if req.starts_with("GET /api/ward/cases ") {
                ("200 OK", CASES.to_string())
            } else {
                ("500 Internal Server Error", r#"{"error":"a dry run must not POST"}"#.to_string())
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_dry_run_reads_a_real_socket_and_writes_nothing() {
    let world = std::env::temp_dir().join(format!("vitals-factory-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&world);
    let out = Command::new(env!("CARGO_BIN_EXE_vitals-factory"))
        .env("WARD", one_shot_ward())
        .env("VITALS_REPO", repo_root())
        .env("VITALS_WORLD_DIR", &world)
        .env("VITALS_GCP_PROJECT", "vitals-academy-dev")
        .env("QUEUE_DEPTH", "3")
        .env("FACTORY_SEED", "5")
        .arg("--dry-run")
        .output()
        .expect("runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("dry run"), "{stdout}");
    assert!(stdout.contains("would then build 3 pack(s)"), "{stdout}");
    assert!(stdout.contains("this build publishes no queue block"), "read the real fixture over the socket: {stdout}");
    assert!(stdout.contains("case door: 18 cases listed"), "read the case door over the socket: {stdout}");
    assert!(stdout.contains("case_id world-"), "names the World case of each draw: {stdout}");
    assert!(!stdout.contains("osce-"), "never a season id: {stdout}");
    assert!(!world.exists(), "a dry run creates nothing, not even the world directory");
}
