//! Scenario files, addressed by their own sha256.
//!
//! Every anchored run carries `sce_hash = sha256(<the whole scenario file>)`, and the leaf binds
//! the run to it: rewrite the scenario and old leaves stop proving anything about the new one,
//! which is the correct behaviour. The crypto was never the gap. The gap was that a stranger
//! holding a leaf had **no way to obtain the file that hash names** — the disk held the current
//! version and nothing else — so "deterministic, re-derivable by anyone" quietly meant
//! "re-derivable by whoever has our repository and guesses the right commit".
//!
//! This module closes it. A hash resolves to bytes through two places, in this order:
//!
//!   1. `conformance/sce-archive/<hash>.json` — the append-only archive of every version that has
//!      ever produced an anchored run. **Nothing here is ever deleted**: deleting a file here
//!      destroys the evidence for the runs that were played against it.
//!   2. the scenario files the server is playing right now, which is what makes today's runs
//!      re-derivable before anyone remembers to archive them.
//!
//! Both paths go through [`find`], and [`find`] hashes what it read before it hands anything
//! back. A content-addressed endpoint that returns the wrong bytes is worse than one that returns
//! nothing: the caller would re-derive a different leaf and conclude the *chain* was lying.
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

/// The bytes whose sha256 is `want`, or nothing.
///
/// `live` is where the server's current scenarios are; `archive` is the directory of past
/// versions. The returned string is verified: it is hashed again here, and a file that does not
/// hash to `want` is treated as absent. That is not belt-and-braces — the archive is named by
/// hash, so a mis-named file would otherwise be served under a hash it does not have, and the
/// whole point of this endpoint is that its answer can be checked by hashing it.
pub fn find(want: &str, live: &[PathBuf], archive: &Path) -> Option<String> {
    if !is_hash(want) {
        return None;
    }
    let want = want.to_ascii_lowercase();
    // The archive first: it is the O(1) hit, and it is the copy that outlives an edit.
    let by_name = archive.join(format!("{want}.json"));
    if let Some(text) = verified(&by_name, &want) {
        return Some(text);
    }
    // Then whatever is on the shelf today. A run anchored an hour ago names a file that nobody
    // has archived yet, and it has to resolve or the endpoint is useless for the newest runs —
    // which are exactly the ones a reviewer will check.
    live.iter().find_map(|p| verified(p, &want))
}

/// Read a file and hand it back only if it hashes to `want`.
fn verified(p: &Path, want: &str) -> Option<String> {
    let text = std::fs::read_to_string(p).ok()?;
    (hex(&sce_hash(&text)) == want).then_some(text)
}

/// Every hash the archive can serve, sorted.
///
/// Used by the startup line and by the tests. Reads names only — a file whose name is not a hash
/// is not in the archive's vocabulary, so it is skipped rather than reported.
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

    #[test]
    fn every_archived_hash_resolves_to_bytes_that_hash_to_it() {
        let a = archive();
        let all = hashes(&a);
        assert!(all.len() >= 17, "the archive lost files: only {} found in {}", all.len(), a.display());
        for h in all {
            let text = find(&h, &[], &a).unwrap_or_else(|| panic!("{h} is in the archive and did not resolve"));
            assert_eq!(hex(&sce_hash(&text)), h, "{h} resolved to bytes with a different hash");
        }
    }

    #[test]
    fn a_hash_nobody_has_seen_resolves_to_nothing() {
        assert!(find(&"f".repeat(64), &[], &archive()).is_none());
    }

    #[test]
    fn a_file_under_the_wrong_name_is_not_served_under_it() {
        // The failure this guards: an archive entry copied under a hash that is not its own.
        // Serving it would hand a verifier bytes that re-derive a different leaf, and the
        // verifier would conclude the chain was wrong rather than the archive.
        let tmp = std::env::temp_dir().join("vitals-archive-liar");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let lie = "b".repeat(64);
        std::fs::write(tmp.join(format!("{lie}.json")), "{\"not\":\"that file\"}").unwrap();
        assert!(hashes(&tmp).contains(&lie), "the fixture is not where the test thinks it is");
        assert!(find(&lie, &[], &tmp).is_none(), "bytes were served under a hash they do not have");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_scenario_on_the_shelf_resolves_before_anyone_archives_it() {
        let live = root().join("conformance/sce-anaphylaxis-ep1.json");
        let text = std::fs::read_to_string(&live).unwrap();
        let h = hex(&sce_hash(&text));
        // Nowhere to look but the live list — an empty directory stands in for a fresh deploy.
        let nowhere = std::env::temp_dir().join("vitals-archive-empty");
        std::fs::create_dir_all(&nowhere).unwrap();
        assert_eq!(find(&h, &[live], &nowhere).as_deref(), Some(text.as_str()));
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
