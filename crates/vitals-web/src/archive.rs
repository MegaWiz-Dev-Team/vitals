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
//! version that has ever produced an anchored run — and only when the **case** that version
//! belongs to has left the shelf. **Nothing in the archive is ever deleted**: deleting a file
//! there destroys the evidence for the runs that were played against it.
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
//! holds it.
//!
//! ## Retirement is a fact about a case, not about a byte sequence
//!
//! That deny list used to be the whole rule, and byte equality was the wrong test for it. A
//! scenario's identity is its own sha256, so **any edit mints a new hash** and leaves the old
//! one matching nothing on the shelf. The old rule read that as "retired" and published it. But
//! nothing had retired: one file in a live case had rotated, and the version it replaced was
//! still, in every respect that matters, that case's mark sheet.
//!
//! It was measured, not theorised. `ep2`'s previous version differs from the one on the shelf by
//! three lines of `rhythm` — the same ten intervention ids, the same matcher keywords, the same
//! `nitrate in RV infarct` trap at the same `rv_involvement > 0.5` threshold. Editing the live
//! case would have started answering `GET /api/sce/44f9c597…` with `200` and all of it, to
//! anyone, while the station was still sittable. The endpoint built to stop a candidate reading
//! their own mark sheet would have been opened by the act of fixing a case.
//!
//! So [`answer`] asks the question retirement actually poses — *can this case still be sat?* —
//! and answers it from `INDEX.json`, which already recorded which file each version was archived
//! from and which nothing had ever read. Every version of a live case is withheld together
//! ([`Answer::Superseded`]), and they all become publishable on the same day: the day the case
//! comes off the shelf. Verification of an anchored, retired case keeps working. A candidate
//! mid-exam gets a 404 that says the scenario is in play, whichever version they asked for.
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

use std::collections::BTreeMap;
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
///
/// Three of the four are refusals, and they are kept apart on purpose. The caller is usually a
/// verifier holding a leaf, so "no such hash" for a hash that plainly exists would send them
/// hunting for a bug in the chain. Each refusal says which situation they are in; none of them
/// says anything whatever about the file's contents.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// A retired version: archived, and its case has left the shelf. The bytes, verified.
    Retired(String),
    /// These exact bytes are on the shelf right now.
    InPlay,
    /// A **past** version of a case that is still on the shelf.
    ///
    /// This is the arm the byte-equality rule did not have, and its absence was the bug. A
    /// scenario's identity on chain is its own sha256, so any edit mints a new hash and leaves
    /// the old one matching nothing on the shelf — which the old rule read as "retired" and
    /// published. But the case had not retired; one file in it had rotated. `ep2`'s previous
    /// version differed from the live one by three lines of `rhythm`, and serving it handed out
    /// every matcher, every threshold and every `harm` of a station a candidate could still be
    /// sitting. Retirement is a fact about a **case**, not about a byte sequence.
    Superseded,
    /// In the archive, and no `INDEX.json` row says which case it belongs to.
    ///
    /// Withheld while anything at all is playable, because a version that cannot be attributed
    /// cannot be shown not to be a live case's mark sheet. Fails closed: the cost of the wrong
    /// guess in one direction is a verifier who has to clone the repository, and in the other it
    /// is handing a candidate the answers. Adding the row publishes it.
    Unattributed,
    /// Not a hash, no such hash, or a file under a name that is not its own.
    Unknown,
}

/// Which case each archived version belongs to — `hash` → the path it was archived from.
///
/// Read from the archive's own `INDEX.json`, which already carried this fact and which nothing
/// consumed. It is the only thing on disk that ties a superseded version to the case it is a
/// version *of*: the bytes cannot say so themselves, and the file name is a digest.
///
/// Read fresh on every call, like the shelf and for the same reason — a deny list that caches is
/// a deny list that goes stale in the unsafe direction.
pub fn index(archive: &Path) -> BTreeMap<String, PathBuf> {
    let Ok(text) = std::fs::read_to_string(archive.join("INDEX.json")) else {
        return BTreeMap::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return BTreeMap::new();
    };
    rows.iter()
        .filter_map(|r| {
            let h = r.get("sce_hash")?.as_str()?.to_ascii_lowercase();
            let p = r.get("path")?.as_str()?;
            is_hash(&h).then(|| (h, PathBuf::from(p)))
        })
        .collect()
}

/// Is `want` a version of a case that is on the shelf right now?
///
/// `INDEX.json` records a repo-relative path (`demo/scenarios/ep2-stemi.json`); the shelf holds
/// absolute ones under whatever `VITALS_SCENARIOS` points at. Compared with [`Path::ends_with`],
/// which matches whole path components from the right, so `.../demo/scenarios/ep2-stemi.json`
/// matches and a file that merely ends in the same characters does not.
fn is_a_live_case(want: &str, live: &[PathBuf], archive: &Path) -> Option<bool> {
    let of = index(archive).get(want)?.clone();
    // `exists`, because the shelf is a list of *declared* cases and a declared case whose file is
    // not on disk is not sittable — that is what "coming soon" looks like, and it is also what
    // withdrawing a case looks like. Retiring one is deleting its file; if a missing file still
    // counted as live, nothing could ever be published.
    Some(live.iter().any(|p| p.ends_with(&of) && p.exists()))
}

/// What to answer for `want`.
///
/// `live` is every scenario the server can be asked to play *today*; `archive` is the directory
/// of past versions. `live` is a deny list and is checked first, in two passes, because a case
/// is bigger than a byte sequence:
///
/// 1. **These bytes are on the shelf.** [`Answer::InPlay`].
/// 2. **These bytes are a past version of something on the shelf** — `INDEX.json` names the file
///    it was archived from, and that file is still playable. [`Answer::Superseded`]. This is the
///    pass the endpoint shipped without, and without it every edit to a live case published the
///    version it replaced. Editing is not retiring.
///
/// What is left is a version whose case has genuinely left the shelf, which is the only thing
/// this endpoint may publish. A version the index cannot attribute is refused rather than
/// guessed at ([`Answer::Unattributed`]).
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
    // Only versions the archive actually holds can be published, so everything below is about a
    // file that is there; a hash that is not there is Unknown whatever the index says.
    let Some(text) = verified(&archive.join(format!("{want}.json")), &want) else {
        return Answer::Unknown;
    };
    match is_a_live_case(&want, live, archive) {
        Some(true) => Answer::Superseded,
        // Attributed, and its case is off the shelf. The one publishable state.
        Some(false) => Answer::Retired(text),
        // No row. Safe only when there is nothing it could be a mark sheet for.
        None if live.is_empty() => Answer::Retired(text),
        None => Answer::Unattributed,
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

/// Every hash this deployment will actually hand over: archived, and belonging to a case that
/// has left the shelf.
///
/// The startup line prints this rather than [`hashes`] because the two are different numbers and
/// the difference is the whole point — an archive whose every file belongs to a case still in
/// the season serves nothing, and a line that said "21 archived" would read like a working
/// endpoint. It counts *versions of retired cases*, so a case with four archived versions moves
/// this number by four on the day it retires and by nothing at all when it is edited.
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

    /// The versions that anchored runs on devnet actually name, which the archive must never
    /// stop holding.
    ///
    /// These four are the previous versions of EP2–EP5, retired on 2026-08-28 by the
    /// `"kind": "lose"` fix — the commit titled *"a patient the script has killed stops having a
    /// heartbeat"* — whose message recorded the belief that "the only anchored
    /// runs today are osce-a". That was checked against one Merkle tree. Across every
    /// `ClaimAccount` the program owns there were **eleven** leaves naming these four hashes, on
    /// trees #488253275, #488321238 and #487877348, and the edit orphaned all of them: for four
    /// months the digest in those leaves resolved to no file any stranger could fetch.
    ///
    /// Pinned here by hash so the archive cannot quietly lose them again. A failure means an
    /// anchored run has stopped being re-derivable, which is the one thing this directory exists
    /// to prevent.
    #[test]
    fn the_versions_anchored_runs_name_are_still_here() {
        let a = archive();
        for (h, case) in [
            ("36b6d1c22d41c681eb0edb565d58ff32e1f32b24d4d074ee7db2220d80b6be72", "demo/scenarios/ep2-stemi.json"),
            ("242a0c9f770e22b87031fa0c2917346d47839ff133385b1251fa9d9df341bf28", "demo/scenarios/ep3-epiglottitis.json"),
            ("0a74511e605d02ec2ec5ff1d23705fea54b7d01b66db0997f7782a3096adeb7f", "demo/scenarios/ep4-pulmonary-embolism.json"),
            ("9433956764028dd157b71c5c0a1f06333ca107ece26b781f4b311811d2229f33", "demo/scenarios/ep5-the-night-the-stars-fell.json"),
        ] {
            // The bytes are there and they are the bytes the chain named.
            let Answer::Retired(text) = answer(h, &[], &a) else {
                panic!("{h} is named by an anchored leaf and is not in the archive")
            };
            assert_eq!(hex(&sce_hash(&text)), h);
            // …and the index says which case, or the endpoint cannot tell live from retired.
            assert_eq!(index(&a).get(h).map(|p| p.to_string_lossy().to_string()).as_deref(), Some(case));
            // …and while that case is on the shelf it stays withheld, which is what made
            // restoring these safe to do while EP2-EP5 are still being sat.
            assert_eq!(answer(h, &shelf(), &a), Answer::Superseded);
        }
    }

    /// The audit, run against the real archive on every `cargo test`: nothing this deployment
    /// would publish belongs to a case a candidate can still sit.
    ///
    /// Stated over `index()` rather than over bytes, because bytes were the broken test. A
    /// version is a live case's mark sheet when the file it was archived *from* is on the shelf,
    /// however far its contents have since drifted from the copy there.
    #[test]
    fn nothing_publishable_belongs_to_a_case_still_on_the_shelf() {
        let a = archive();
        let live = shelf();
        let idx = index(&a);
        let mut leaking = Vec::new();
        for h in servable(&live, &a) {
            if let Some(of) = idx.get(&h) {
                if live.iter().any(|p| p.ends_with(of)) {
                    leaking.push(format!("{h} is published and is a version of {}", of.display()));
                }
            }
        }
        assert!(leaking.is_empty(), "{leaking:#?}");
    }

    /// Every archived version is attributable. A row missing from `INDEX.json` is not a
    /// cosmetic omission — it is the only thing that ties a digest to the case it is a version
    /// of, so without it the endpoint cannot tell a retired file from a live one and refuses.
    #[test]
    fn every_archived_version_says_which_case_it_belongs_to() {
        let a = archive();
        let idx = index(&a);
        let orphans: Vec<String> =
            hashes(&a).into_iter().filter(|h| !idx.contains_key(h)).collect();
        assert!(orphans.is_empty(), "archived with no INDEX.json row: {orphans:#?}");
    }

    /// The bug this rule was written for, with the shapes it actually had.
    ///
    /// A previous version of `ep2` — same case, different bytes — must be refused for exactly as
    /// long as `ep2` can be sat, and must say *which* refusal it is.
    #[test]
    fn a_previous_version_of_a_live_case_is_withheld() {
        let (dir, past) = superseded_fixture("withheld");
        let ep2 = root().join("demo/scenarios/ep2-stemi.json");

        assert_eq!(answer(&past, std::slice::from_ref(&ep2), &dir), Answer::Superseded,
            "a past version of a station still on the shelf was published");

        // …and the day ep2 leaves the season, the same bytes publish.
        let Answer::Retired(text) = answer(&past, &[], &dir) else {
            panic!("a version of a retired case still would not publish")
        };
        assert_eq!(hex(&sce_hash(&text)), past);
    }

    /// A version whose case cannot be established is refused while anything is playable, and is
    /// held apart from "no such hash" so the operator is told to add the row rather than sent
    /// looking for a missing file.
    #[test]
    fn a_version_the_index_cannot_attribute_is_withheld() {
        let (dir, past) = superseded_fixture("unattributed");
        std::fs::write(dir.join("INDEX.json"), "[]").unwrap();
        let ep2 = root().join("demo/scenarios/ep2-stemi.json");
        assert_eq!(answer(&past, &[ep2], &dir), Answer::Unattributed);
        // Nothing playable, nothing it could be the answers to.
        assert!(matches!(answer(&past, &[], &dir), Answer::Retired(_)));
    }

    /// An archive holding one superseded version of `ep2`, and its index row. Returns the
    /// directory and the hash of the past version.
    fn superseded_fixture(tag: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir()
            .join(format!("vitals-archive-superseded-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A real past version: today's ep2 with one field dropped, which is the size of edit
        // that rotated the hash in the first place.
        let now = std::fs::read_to_string(root().join("demo/scenarios/ep2-stemi.json")).unwrap();
        let past = now.replace("\"setting\": \"ED\",\n", "");
        assert_ne!(past, now, "the fixture stopped differing from the live file");
        let h = hex(&sce_hash(&past));
        std::fs::write(dir.join(format!("{h}.json")), &past).unwrap();
        std::fs::write(
            dir.join("INDEX.json"),
            serde_json::to_string_pretty(&serde_json::json!([{
                "sce_hash": h,
                "path": "demo/scenarios/ep2-stemi.json",
                "bytes": past.len(),
            }])).unwrap(),
        ).unwrap();
        (dir, h)
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
