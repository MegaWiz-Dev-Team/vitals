//! The author side table, and the two things that must never be true about this repository.
//!
//! `AUTHORS.json` is not committed until a key exists to sign it, so most of what is checked here
//! is checked against a keypair the test makes and throws away. That is deliberate: the
//! verification path has to be proved by a signature that really verifies and one that really
//! does not, and neither of those needs a secret anybody keeps.

use solana_sdk::signature::{Keypair, Signer};
use std::path::PathBuf;
use vitals_web::authors::{self, Attribution};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

fn signed_by(kp: &Keypair, sce_hash: &str) -> Attribution {
    let msg = authors::message(sce_hash).expect("a hash to sign");
    Attribution {
        sce_hash: sce_hash.to_string(),
        author: kp.pubkey().to_string(),
        signature: kp.sign_message(&msg).to_string(),
    }
}

/// A real hash from the archive, so the test is about signatures rather than about hashes.
const A_REAL_CASE: &str = "4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d";

// ── the verifier ────────────────────────────────────────────────────────────

#[test]
fn a_signature_over_the_case_hash_verifies() {
    let kp = Keypair::new();
    signed_by(&kp, A_REAL_CASE).verify().expect("a signature this test just made did not verify");
}

/// The check has to be able to say no, or it is not a check.
#[test]
fn a_signature_from_the_wrong_key_is_refused() {
    let kp = Keypair::new();
    let mut a = signed_by(&kp, A_REAL_CASE);
    a.author = Keypair::new().pubkey().to_string();
    assert!(a.verify().is_err(), "any key could claim any case");
}

/// Signing one case must not sign another. This is what the hash being *in* the signed message
/// buys, and it is the difference between attribution and a blank cheque.
#[test]
fn a_signature_does_not_carry_to_another_case() {
    let kp = Keypair::new();
    let mut a = signed_by(&kp, A_REAL_CASE);
    a.sce_hash = "f9dbb7de0551815620d88e7b8ff66292e5fbdf565801c972e216ce329c380e45".into();
    assert!(a.verify().is_err(), "one signature covered a second case");
}

#[test]
fn a_mangled_signature_is_refused() {
    let kp = Keypair::new();
    let mut a = signed_by(&kp, A_REAL_CASE);
    a.signature = Keypair::new().sign_message(b"something else").to_string();
    assert!(a.verify().is_err(), "a signature over other bytes was accepted");
    a.signature = "not base58 at all!!".into();
    assert!(a.verify().is_err(), "unparseable input was treated as a signature");
}

/// The signed bytes are the hash's *bytes*, not its spelling.
#[test]
fn the_signed_message_is_domain_separated_and_binary() {
    let m = authors::message(A_REAL_CASE).expect("a message");
    assert_eq!(m.len(), "vitals.author.v1\n".len() + 32, "the message is not domain + 32 bytes");
    assert!(m.starts_with(b"vitals.author.v1\n"), "no domain separation: a signature here \
        could be replayed as a signature for something else");
    assert!(!m.windows(4).any(|w| w == b"4ee5"), "the hex spelling was signed, not the hash");
    assert!(authors::message("nonsense").is_err());
    assert!(authors::message(&"z".repeat(64)).is_err(), "non-hex was accepted as a hash");
}

// ── the table as a whole ────────────────────────────────────────────────────

#[test]
fn a_case_the_archive_does_not_have_cannot_be_claimed() {
    let kp = Keypair::new();
    let stranger = "0".repeat(64);
    let table = vec![signed_by(&kp, &stranger)];
    let problems = authors::audit(&table, &[A_REAL_CASE.to_string()]);
    assert!(
        problems.iter().any(|p| p.contains("not in the archive index")),
        "a case that is in no archive was attributed without complaint: {problems:?}"
    );
}

#[test]
fn one_case_cannot_be_claimed_twice() {
    let kp = Keypair::new();
    let table = vec![signed_by(&kp, A_REAL_CASE), signed_by(&Keypair::new(), A_REAL_CASE)];
    let problems = authors::audit(&table, &[A_REAL_CASE.to_string()]);
    assert!(
        problems.iter().any(|p| p.contains("attributed twice")),
        "two authors claimed one case and nothing objected: {problems:?}"
    );
}

/// **The gate.** Whatever is committed must verify, or the build is red.
///
/// An absent file is an empty ledger and passes: the format, the verifier and the signing tool
/// ship before any key exists to sign with, which is the whole reason this can be checked at all.
#[test]
fn whatever_is_committed_verifies_against_the_archive() {
    let root = repo();
    let table = authors::load(&root.join(authors::AUTHORS_PATH)).expect("read AUTHORS.json");
    if table.is_empty() {
        return;
    }
    let known = authors::archive_hashes(&root.join(authors::INDEX_PATH)).expect("read INDEX.json");
    let problems = authors::audit(&table, &known);
    assert!(problems.is_empty(), "the committed attributions do not hold up:\n  {}",
            problems.join("\n  "));
}

// ── the secret that must not be here ────────────────────────────────────────

/// No Solana keypair anywhere git would carry.
///
/// A keypair is a JSON array of 64 bytes, so this looks for the shape rather than for a name.
/// `sign-attribution` refuses to read a key from inside the repo, but refusing at use time does
/// not help if one has already been copied in — and gitleaks, which would catch a committed key,
/// is "not installed, CI still runs it" on this machine. This closes that gap locally.
///
/// **Scoped to what git would include**: tracked files, plus untracked ones that are not
/// ignored — which is exactly what gitleaks means by the repository, and what a `git add -A`
/// would sweep up. Ignored paths are deliberately outside it, and are where a machine that has
/// deployed the program or run a local validator legitimately keeps keys: this repo's own
/// `.gitignore` excludes `keys/` and `.test-ledger/` for that reason. Scanning them instead
/// would make this permanently red on every machine that has ever done real work, which is how a
/// check gets switched off.
#[test]
fn no_signing_key_is_anywhere_git_would_carry_it() {
    fn looks_like_a_keypair(bytes: &[u8]) -> bool {
        // Cheap rejects first: 64 bytes as JSON is small, and it starts with '['.
        if bytes.len() > 1200 || !bytes.starts_with(b"[") {
            return false;
        }
        serde_json::from_slice::<Vec<u8>>(bytes).is_ok_and(|v| v.len() == 64)
    }

    let root = repo();
    // Asking git rather than reimplementing its ignore rules — the question really is "what
    // would git carry", and git is the only thing that answers that exactly.
    let out = std::process::Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
        .current_dir(&root)
        .output()
        .expect("git is needed to know which files this repository actually contains");
    assert!(out.status.success(), "git ls-files failed: {}", String::from_utf8_lossy(&out.stderr));

    let found: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let full = root.join(p);
            std::fs::metadata(&full).is_ok_and(|m| m.len() <= 1200)
                && std::fs::read(&full).is_ok_and(|b| looks_like_a_keypair(&b))
        })
        .map(str::to_string)
        .collect();

    assert!(
        found.is_empty(),
        "these look like Ed25519 keypairs and are inside what git carries:\n  {}",
        found.join("\n  ")
    );
}

// ── the tally ───────────────────────────────────────────────────────────────

use std::collections::BTreeMap;
use vitals_web::authors::{tally, AuthorLedger};

/// One proven attempt, as the chain holds it: a case, and nothing about who wrote it.
fn attempts(cases: &[&str]) -> Vec<String> {
    cases.iter().map(|c| c.to_string()).collect()
}

/// Count a set of attempts per case — the shape `Chain::proven_by_case` returns.
fn per_case(attempts: &[String]) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for c in attempts {
        *m.entry(c.clone()).or_insert(0u64) += 1;
    }
    m
}

/// **The re-derivation.** The ledger must be recomputable from the same attempts by other means.
///
/// `tally` groups by author and sums; this walks the raw attempt list per author and counts, with
/// no grouping and no shared code. If the two ever disagree, the ledger is arithmetic nobody
/// should trust — and a ledger nobody should trust is worse than no ledger, because it looks
/// like one.
#[test]
fn the_ledger_is_recomputable_from_the_attempts_it_came_from() {
    let (a, b) = ("11".repeat(32), "22".repeat(32));
    let (c, unplayed) = ("33".repeat(32), "44".repeat(32));
    let alice = Keypair::new().pubkey().to_string();
    let bob = Keypair::new().pubkey().to_string();

    let table = vec![
        attributed(&alice, &a),
        attributed(&alice, &b),
        attributed(&alice, &unplayed),
        attributed(&bob, &c),
    ];
    let raw = attempts(&[&a, &a, &a, &b, &c, &c]);
    let paths = BTreeMap::new();

    let ledger = tally(&table, &paths, &per_case(&raw));

    // Recomputed the long way round: for each author, walk every attempt and count the ones
    // whose case that author is credited with.
    for entry in &ledger {
        let mine: Vec<&String> = table
            .iter()
            .filter(|t| t.author == entry.author)
            .map(|t| &t.sce_hash)
            .collect();
        let expected: u64 = raw.iter().filter(|c| mine.contains(c)).count() as u64;
        assert_eq!(
            entry.proven_replays, expected,
            "{}'s total does not survive being counted the other way",
            entry.author
        );
        assert_eq!(entry.distinct_cases, mine.len(), "{}'s case count is wrong", entry.author);
        assert_eq!(
            entry.cases.iter().map(|c| c.proven_replays).sum::<u64>(),
            entry.proven_replays,
            "{}'s per-case rows do not add up to their own total",
            entry.author
        );
    }
    // And the whole ledger accounts for every attempt, none twice.
    assert_eq!(
        ledger.iter().map(|e| e.proven_replays).sum::<u64>(),
        raw.len() as u64,
        "the ledger and the attempt list disagree about how many replays happened"
    );
}

/// A case nobody has replayed is still a case somebody wrote.
#[test]
fn an_unreplayed_case_stays_on_its_authors_ledger_at_zero() {
    let unplayed = "44".repeat(32);
    let alice = Keypair::new().pubkey().to_string();
    let ledger = tally(&[attributed(&alice, &unplayed)], &BTreeMap::new(), &BTreeMap::new());
    assert_eq!(ledger.len(), 1, "the author disappeared with their unplayed case");
    assert_eq!(ledger[0].distinct_cases, 1);
    assert_eq!(ledger[0].proven_replays, 0);
}

/// The same inputs must print the same way, or a diff against a re-derivation is noise.
#[test]
fn the_ledger_is_ordered_and_not_merely_grouped() {
    let (a, b, c) = ("11".repeat(32), "22".repeat(32), "33".repeat(32));
    let alice = Keypair::new().pubkey().to_string();
    let table = vec![attributed(&alice, &a), attributed(&alice, &b), attributed(&alice, &c)];
    let counts = per_case(&attempts(&[&b, &b, &b, &c, &c]));

    let first = tally(&table, &BTreeMap::new(), &counts);
    let shuffled: Vec<_> = table.iter().rev().cloned().collect();
    let second = tally(&shuffled, &BTreeMap::new(), &counts);
    assert_eq!(first, second, "the ledger depends on the order the table happened to be in");

    let order: Vec<u64> = first[0].cases.iter().map(|c| c.proven_replays).collect();
    assert_eq!(order, vec![3, 2, 0], "cases are not ordered by how often they were replayed");
}

fn attributed(author: &str, sce_hash: &str) -> Attribution {
    Attribution {
        sce_hash: sce_hash.to_string(),
        author: author.to_string(),
        signature: String::new(),
    }
}

/// Unused today and load-bearing tomorrow: silences nothing, proves the type is public.
#[allow(dead_code)]
fn _ledger_is_nameable(_: AuthorLedger) {}
