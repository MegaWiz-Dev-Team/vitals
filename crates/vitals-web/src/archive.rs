//! Scenario files, addressed by their own sha256.
//!
//! Every anchored run carries `sce_hash = sha256(<the whole scenario file>)`, and the leaf binds
//! the run to it: rewrite the scenario and old leaves stop proving anything about the new one,
//! which is the correct behaviour. The crypto was never the gap. The gap was that a stranger
//! holding a leaf had **no way to obtain the file that hash names** — the disk held the current
//! version and nothing else — so "deterministic, re-derivable by anyone" quietly meant
//! "re-derivable by whoever has our repository and guesses the right commit".
//!
//! This module closes it, for the cases it is allowed to close it for. A hash resolves through
//! exactly one place — `conformance/sce-archive/<hash>.json`, the append-only archive of every
//! version that has ever produced an anchored run — and only when that version is **retired**.
//! **Nothing in the archive is ever deleted**: deleting a file there destroys the evidence for
//! the runs that were played against it.
//!
//! ## Why a live scenario is never served
//!
//! A scenario file is the answer key. It carries every intervention id, every matcher keyword,
//! every `(HARM)` the author wrote beside a wrong turn, the trigger thresholds that decide the
//! outcome, and `_note` fields that name the diagnosis outright. `/api/sce` used to fall back to
//! the shelf — "a run anchored ten minutes ago names a file nobody has archived yet, and it has
//! to resolve" — so a candidate could start a station, read `sce_hash` off their own view, and
//! `GET` the whole mark sheet while the clock was still running. That defeats every seal on
//! `/api/marks`, `/api/debrief` and [`crate::news2`]'s neighbours in one unauthenticated request,
//! and it is a worse leak than the one `bank_case` was pulled off `/api/chain` to stop.
//!
//! So the shelf is a **deny list**, not a fallback. [`answer`] hashes every scenario the server
//! is playing right now and refuses anything that matches one, whether or not the archive also
//! holds it. What is left is the retired versions: a case that has been edited or withdrawn can
//! no longer be sat, so publishing it costs nobody a mark, and the leaves anchored against it
//! stay checkable forever. Verification of an anchored, retired case keeps working. A candidate
//! mid-exam gets a 404 that says the scenario is in play.
//!
//! The cost is stated where a verifier will read it (`VERIFICATION.md` §5): while a case is on
//! the shelf, its bytes come from the repository — `shasum -a 256 demo/stations/osce-a.sce.json`
//! — not from this endpoint. That is a worse convenience and the same proof.
//!
//! [`answer`] hashes what it read before it hands anything back. A content-addressed endpoint
//! that returns the wrong bytes is worse than one that returns nothing: the caller would
//! re-derive a different leaf and conclude the *chain* was lying.
//!
//! The archive lives under `conformance/` and not under `docs/`, and that is load-bearing:
//! `docs/internal/` is in `.gitignore` *and* in `.dockerignore`, so an archive kept there is in
//! no build, in no image, and in nobody's clone — an endpoint serving from it would 404 for every
//! hash in production. `conformance/` is committed and the Dockerfile copies it whole.

use std::path::{Path, PathBuf};
use vitals_replay::{hex, sce_hash};

/// Where the archive sits, relative to the scenario root.
///
/// A constant rather than a literal at the call site because two things have to agree about it —
/// the server that reads it and the test that proves the Dockerfile ships it.
pub const DIR: &str = "conformance/sce-archive";

/// Is this string a scenario hash and nothing else?
///
/// The only gate between the network and a file name. 64 hex digits cannot contain `/`, `.` or a
/// NUL, so traversal is refused by the shape of the thing rather than by stripping — nothing is
/// sanitised into a name the caller did not ask for.
pub fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What this deployment may say about one hash.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// A retired version: archived, and on nobody's shelf. The bytes, verified.
    Retired(String),
    /// A scenario the server is playing right now. Withheld until it is retired — see the module
    /// header. Held apart from [`Answer::Unknown`] so the refusal can explain itself: the caller
    /// is usually a verifier holding a leaf, and "no such hash" would send them hunting for a
    /// bug that is not there. It discloses nothing a leaf does not already carry — the hash came
    /// from the chain or from their own run — and nothing whatever about the file's contents.
    InPlay,
    /// Not a hash, no such hash, or a file under a name that is not its own.
    Unknown,
}

/// What to answer for `want`.
///
/// `live` is every scenario the server can be asked to play *today*; `archive` is the directory
/// of past versions. `live` is a deny list and is checked first: a hash that is on the shelf is
/// refused even when the archive also holds a copy of it, because the archive holding it does
/// not make sitting the station any less of an exam.
///
/// Anything returned is verified — hashed again here, and a file that does not hash to `want` is
/// treated as absent. That is not belt-and-braces: the archive is named by hash, so a mis-named
/// file would otherwise be served under a hash it does not have, and the whole point of this
/// endpoint is that its answer can be checked by hashing it.
pub fn answer(want: &str, live: &[PathBuf], archive: &Path) -> Answer {
    if !is_hash(want) {
        return Answer::Unknown;
    }
    let want = want.to_ascii_lowercase();
    // The shelf, first and always. Reading seventeen small files per request is nothing beside
    // getting this wrong, and reading them fresh means a case retired by a deploy stops being
    // withheld the moment it stops being playable — no cache to go stale in the unsafe direction.
    if live.iter().any(|p| verified(p, &want).is_some()) {
        return Answer::InPlay;
    }
    match verified(&archive.join(format!("{want}.json")), &want) {
        Some(text) => Answer::Retired(text),
        None => Answer::Unknown,
    }
}

/// Read a file and hand it back only if it hashes to `want`.
fn verified(p: &Path, want: &str) -> Option<String> {
    let text = std::fs::read_to_string(p).ok()?;
    (hex(&sce_hash(&text)) == want).then_some(text)
}

/// Every hash the archive *holds*, sorted — not every hash it may serve. See [`servable`].
///
/// Reads names only — a file whose name is not a hash is not in the archive's vocabulary, so it
/// is skipped rather than reported.
pub fn hashes(archive: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(archive) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.strip_suffix(".json").filter(|s| is_hash(s)).map(|s| s.to_string())
        })
        .collect();
    v.sort();
    v
}

/// Every hash this deployment will actually hand over: archived, and retired.
///
/// The startup line prints this rather than [`hashes`] because the two are different numbers and
/// the difference is the whole point — an archive of seventeen files that are all still on the
/// shelf serves nothing, and a line that said "17 archived" would read like a working endpoint.
pub fn servable(live: &[PathBuf], archive: &Path) -> Vec<String> {
    hashes(archive)
        .into_iter()
        .filter(|h| matches!(answer(h, live, archive), Answer::Retired(_)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn archive() -> PathBuf {
        root().join(DIR)
    }

    #[test]
    fn a_hash_is_sixty_four_hex_digits_and_nothing_else() {
        assert!(is_hash(&"a".repeat(64)));
        assert!(is_hash(&"0123456789ABCDEF".repeat(4)));
        assert!(!is_hash(&"a".repeat(63)));
        assert!(!is_hash(&"a".repeat(65)));
        assert!(!is_hash(""));
        // The shapes a traversal needs. None of them survive the gate.
        for bad in ["../../etc/passwd", "..", "./x", "a/b", "a".repeat(62).as_str()] {
            assert!(!is_hash(bad), "{bad:?} passed as a hash");
        }
        // A 64-character path is still not hex.
        assert!(!is_hash(&format!("..{}", "/a".repeat(31))));
    }

    /// Nothing may be lost from the archive, and nothing in it may be misfiled. Asked with an
    /// empty shelf, because this is a property of the archive rather than of what is playable.
    #[test]
    fn every_archived_hash_resolves_to_bytes_that_hash_to_it() {
        let a = archive();
        let all = hashes(&a);
        assert!(all.len() >= 17, "the archive lost files: only {} found in {}", all.len(), a.display());
        for h in all {
            let Answer::Retired(text) = answer(&h, &[], &a) else {
                panic!("{h} is in the archive and did not resolve")
            };
            assert_eq!(hex(&sce_hash(&text)), h, "{h} resolved to bytes with a different hash");
        }
    }

    #[test]
    fn a_hash_nobody_has_seen_resolves_to_nothing() {
        assert_eq!(answer(&"f".repeat(64), &[], &archive()), Answer::Unknown);
    }

    #[test]
    fn a_file_under_the_wrong_name_is_not_served_under_it() {
        // The failure this guards: an archive entry copied under a hash that is not its own.
        // Serving it would hand a verifier bytes that re-derive a different leaf, and the
        // verifier would conclude the chain was wrong rather than the archive.
        // Per-process: this test writes a fixture and then deletes the directory, so a fixed
        // path had it wiping the fixture of a concurrent `cargo test` run on the same checkout.
        let tmp = std::env::temp_dir().join(format!("vitals-archive-liar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let lie = "b".repeat(64);
        std::fs::write(tmp.join(format!("{lie}.json")), "{\"not\":\"that file\"}").unwrap();
        assert!(hashes(&tmp).contains(&lie), "the fixture is not where the test thinks it is");
        assert_eq!(answer(&lie, &[], &tmp), Answer::Unknown, "bytes were served under a hash they do not have");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// **The leak.** The shelf used to be a fallback, so a case that was being sat resolved to
    /// its own answer key. It is a deny list now, and the deny list wins over the archive: the
    /// season's files are *both* live and archived, so a rule of "archive first" would have
    /// published every station in the season while the season was being played.
    #[test]
    fn a_scenario_on_the_shelf_is_refused_even_though_the_archive_holds_it() {
        let live = root().join("conformance/sce-anaphylaxis-ep1.json");
        let text = std::fs::read_to_string(&live).unwrap();
        let h = hex(&sce_hash(&text));
        let a = archive();
        assert!(a.join(format!("{h}.json")).exists(), "the fixture is not also archived any more");
        assert_eq!(answer(&h, &[live], &a), Answer::InPlay, "a playable case handed over its own answer key");
    }

    /// Every file the server can be asked to play is withheld, not just the one above.
    #[test]
    fn nothing_on_the_shelf_is_served() {
        let a = archive();
        let live: Vec<PathBuf> = shelf();
        assert!(live.len() >= 17, "the shelf stopped being found: {live:?}");
        for p in &live {
            let text = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let h = hex(&sce_hash(&text));
            assert_eq!(answer(&h, &live, &a), Answer::InPlay, "{} is playable and servable", p.display());
        }
    }

    /// Retirement is what makes a case publishable. Take one file off the shelf — which is what
    /// editing or withdrawing a case does to its old hash — and the archived copy resolves.
    #[test]
    fn a_retired_version_still_resolves() {
        let a = archive();
        let mut live = shelf();
        let retired = live.pop().expect("a shelf to retire from");
        let text = std::fs::read_to_string(&retired).unwrap();
        let h = hex(&sce_hash(&text));
        assert_eq!(
            answer(&h, &live, &a),
            Answer::Retired(text),
            "{} came off the shelf and the archive still would not publish it",
            retired.display()
        );
    }

    /// What the startup line prints, and why it is not `hashes().len()`.
    #[test]
    fn servable_is_the_archive_minus_the_shelf() {
        let a = archive();
        assert!(servable(&shelf(), &a).is_empty(), "a case that is on the shelf today is being published");
        assert_eq!(servable(&[], &a).len(), hashes(&a).len(), "an empty shelf withholds nothing");
    }

    /// Every scenario file this repository can play, found from the disk rather than from
    /// `main.rs` — this module is the library half and cannot see the binary's case table.
    fn shelf() -> Vec<PathBuf> {
        let r = root();
        let mut v: Vec<PathBuf> = Vec::new();
        for d in ["demo/stations", "demo/scenarios"] {
            let mut found: Vec<PathBuf> = std::fs::read_dir(r.join(d))
                .unwrap_or_else(|e| panic!("{d}: {e}"))
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect();
            found.sort();
            v.extend(found);
        }
        v.push(r.join("conformance/sce-anaphylaxis-ep1.json"));
        v
    }

    /// The archive is worth nothing if the image does not carry it.
    ///
    /// `docs/internal/` — where these files were first parked — is ignored by git *and* by
    /// Docker, so this is the assertion that stops them drifting back there and taking the
    /// endpoint's answer with them.
    #[test]
    fn the_image_carries_the_archive() {
        let dockerfile = std::fs::read_to_string(root().join("Dockerfile")).expect("Dockerfile");
        assert!(
            dockerfile.contains("COPY conformance /app/conformance"),
            "the image stopped copying conformance/, so /api/sce would 404 in production"
        );
        assert!(DIR.starts_with("conformance/"), "the archive moved out from under the COPY");
        let ignore = std::fs::read_to_string(root().join(".dockerignore")).expect(".dockerignore");
        for line in ignore.lines().map(str::trim) {
            assert!(
                !line.starts_with("conformance"),
                ".dockerignore now excludes the archive from the image: {line:?}"
            );
        }
    }
}
