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

// ── the ledger ──────────────────────────────────────────────────────────────

use std::collections::{BTreeMap, BTreeSet};
use vitals_web::authors::{ledger, IndexEntry, Inputs};

fn entry(sce_hash: &str, path: &str) -> IndexEntry {
    IndexEntry { sce_hash: sce_hash.to_string(), path: path.to_string() }
}

fn attributed(author: &str, sce_hash: &str) -> Attribution {
    Attribution {
        sce_hash: sce_hash.to_string(),
        author: author.to_string(),
        signature: String::new(),
    }
}

fn per_case(attempts: &[&str]) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for c in attempts {
        *m.entry(c.to_string()).or_insert(0u64) += 1;
    }
    m
}

/// The shape the ruling asks for: versions are signed, cases are lineages.
///
/// osce-a is the real example — five replays against a version that is no longer the file on the
/// shelf. Grouped by hash the card would say nothing; grouped by lineage it says five.
#[test]
fn a_case_is_its_lineage_and_carries_every_versions_replays() {
    let (old, live) = ("11".repeat(32), "22".repeat(32));
    let alice = Keypair::new().pubkey().to_string();
    let index = vec![entry(&old, "demo/stations/osce-a.sce.json"),
                     entry(&live, "demo/stations/osce-a.sce.json")];
    let table = vec![attributed(&alice, &old), attributed(&alice, &live)];

    let counts = per_case(&[&old, &old, &old, &old, &old]);
    let led = ledger(&Inputs {
        table: &table,
        index: &index,
        eps: &BTreeMap::from([(live.clone(), "osce-a".to_string())]),
        live: &BTreeSet::from([live.clone()]),
        proven: &counts,
        paid: &BTreeMap::new(),
        payable: None,
    });

    assert_eq!(led.cases.len(), 1, "two versions of one case became two cases");
    let c = &led.cases[0];
    assert_eq!(c.proven_replays, 5, "the lineage lost the replays of its older version");
    assert_eq!(c.ep, "osce-a", "the card is taken from the live version");
    assert_eq!(c.versions.len(), 2);
    assert!(c.versions[0].live, "the live version is not listed first");
    assert_eq!(c.versions[0].proven_replays, 0, "the live version has been played zero times");
    assert_eq!(c.versions[1].proven_replays, 5, "the played version lost its count");
}

/// Rule 4: replays never move. A key's own total counts only what it signed.
#[test]
fn replays_stay_with_the_version_that_was_played() {
    let (old, live) = ("11".repeat(32), "22".repeat(32));
    let (alice, bob) = (Keypair::new().pubkey().to_string(), Keypair::new().pubkey().to_string());
    let index = vec![entry(&old, "case.json"), entry(&live, "case.json")];
    // Alice wrote the version people played; Bob revised it and nobody has played his yet.
    let table = vec![attributed(&alice, &old), attributed(&bob, &live)];

    let counts = per_case(&[&old, &old, &old]);
    let led = ledger(&Inputs {
        table: &table,
        index: &index,
        eps: &BTreeMap::new(),
        live: &BTreeSet::from([live.clone()]),
        proven: &counts,
        paid: &BTreeMap::new(),
        payable: None,
    });

    let a = led.authors.iter().find(|a| a.author == alice).expect("alice");
    let b = led.authors.iter().find(|a| a.author == bob).expect("bob");
    assert_eq!(a.proven_replays, 3, "the replays moved off the version that was played");
    assert_eq!(b.proven_replays, 0, "revising a case collected somebody else's replays");
    // Both are authors of record for the one case, which is what the byline shows.
    assert_eq!(led.cases[0].authors.len(), 2);
    assert_eq!(led.cases[0].proven_replays, 3);
    assert_eq!(a.distinct_cases, 1, "one lineage counted as more than one case");
}

/// **The re-derivation.** The ledger must be recomputable from the same attempts by other means.
#[test]
fn the_ledger_is_recomputable_from_the_attempts_it_came_from() {
    let (a1, a2, b1, unplayed) =
        ("11".repeat(32), "22".repeat(32), "33".repeat(32), "44".repeat(32));
    let alice = Keypair::new().pubkey().to_string();
    let bob = Keypair::new().pubkey().to_string();
    let index = vec![entry(&a1, "a.json"), entry(&a2, "a.json"),
                     entry(&b1, "b.json"), entry(&unplayed, "c.json")];
    let table = vec![attributed(&alice, &a1), attributed(&alice, &a2),
                     attributed(&bob, &b1), attributed(&alice, &unplayed)];
    let raw = [&a1, &a1, &a1, &a2, &b1, &b1];

    let counts = per_case(&raw.map(|s| s.as_str()));
    let led = ledger(&Inputs {
        table: &table,
        index: &index,
        eps: &BTreeMap::new(),
        live: &BTreeSet::new(),
        proven: &counts,
        paid: &BTreeMap::new(),
        payable: None,
    });

    // Counted the long way: walk the raw attempts per author, with no grouping.
    for entry in &led.authors {
        let signed: Vec<&String> =
            table.iter().filter(|t| t.author == entry.author).map(|t| &t.sce_hash).collect();
        let expected = raw.iter().filter(|c| signed.contains(c)).count() as u64;
        assert_eq!(entry.proven_replays, expected,
                   "{}'s total does not survive being counted the other way", entry.author);
    }
    // And per lineage, the same again.
    for c in &led.cases {
        let hashes: Vec<&String> =
            index.iter().filter(|e| e.path == c.path).map(|e| &e.sce_hash).collect();
        let expected = raw.iter().filter(|h| hashes.contains(h)).count() as u64;
        assert_eq!(c.proven_replays, expected, "{} does not add up", c.path);
    }
    assert_eq!(led.cases.iter().map(|c| c.proven_replays).sum::<u64>(), raw.len() as u64,
               "the ledger and the attempt list disagree about how many replays happened");
}

/// A case nobody has replayed is still a case somebody wrote.
#[test]
fn an_unreplayed_case_stays_on_the_ledger_at_zero() {
    let h = "44".repeat(32);
    let alice = Keypair::new().pubkey().to_string();
    let led = ledger(&Inputs { table: &[attributed(&alice, &h)], index: &[entry(&h, "c.json")], eps: &BTreeMap::new(), live: &BTreeSet::new(), proven: &BTreeMap::new(), paid: &BTreeMap::new(), payable: None });
    assert_eq!(led.cases.len(), 1, "the case disappeared for not being popular");
    assert_eq!(led.authors[0].distinct_cases, 1);
    assert_eq!(led.authors[0].proven_replays, 0);
}

/// A case nobody has signed is not on the ledger at all — the same rule the byline follows.
#[test]
fn an_unsigned_case_is_absent_rather_than_blank() {
    let h = "55".repeat(32);
    let led = ledger(&Inputs { table: &[], index: &[entry(&h, "c.json")], eps: &BTreeMap::new(), live: &BTreeSet::new(), proven: &per_case(&[&h]), paid: &BTreeMap::new(), payable: None });
    assert!(led.cases.is_empty(), "an unattributed case appeared with an empty author");
    assert!(led.authors.is_empty());
}

/// The same inputs must produce the same ledger, or a diff against a re-derivation is noise.
#[test]
fn the_ledger_is_ordered_and_not_merely_grouped() {
    let (a, b) = ("11".repeat(32), "22".repeat(32));
    let alice = Keypair::new().pubkey().to_string();
    let index = vec![entry(&a, "a.json"), entry(&b, "b.json")];
    let table = vec![attributed(&alice, &a), attributed(&alice, &b)];
    let counts = per_case(&[&b, &b]);

    let first = ledger(&Inputs { table: &table, index: &index, eps: &BTreeMap::new(), live: &BTreeSet::new(), proven: &counts, paid: &BTreeMap::new(), payable: None });
    let flipped: Vec<_> = table.iter().rev().cloned().collect();
    let second = ledger(&Inputs { table: &flipped, index: &index, eps: &BTreeMap::new(), live: &BTreeSet::new(), proven: &counts, paid: &BTreeMap::new(), payable: None });
    assert_eq!(first, second, "the ledger depends on the order the table happened to be in");
    assert_eq!(first.cases[0].path, "b.json", "cases are not ordered by how often they were played");
}

// ── what was paid, which is not what was proven ─────────────────────────────

use vitals_web::authors::Paid;

/// A payment is a different event from a proof, and the ledger must not compute one from the
/// other. A replay can be proven and unpaid — no attribution, an unlisted key, the daily cap —
/// and the ledger has to be able to say so.
#[test]
fn paid_is_reported_separately_from_proven_and_never_derived_from_it() {
    let (old, live) = ("11".repeat(32), "22".repeat(32));
    let alice = Keypair::new().pubkey().to_string();
    let index = vec![entry(&old, "case.json"), entry(&live, "case.json")];
    let table = vec![attributed(&alice, &old), attributed(&alice, &live)];
    // Five proven on the old version; only two of them paid.
    let counts = per_case(&[&old, &old, &old, &old, &old]);
    let paid = BTreeMap::from([(old.clone(), Paid { paid: 2, paid_lamports: 1_700_000 })]);

    let led = ledger(&Inputs {
        table: &table,
        index: &index,
        eps: &BTreeMap::new(),
        live: &BTreeSet::from([live.clone()]),
        proven: &counts,
        paid: &paid,
        payable: None,
    });

    let c = &led.cases[0];
    assert_eq!(c.proven_replays, 5, "the lineage's replays moved");
    assert_eq!(c.paid.paid, 2, "the ledger invented payments to match the replays");
    assert_eq!(c.paid.paid_lamports, 1_700_000);
    let a = &led.authors[0];
    assert_eq!((a.proven_replays, a.paid.paid), (5, 2), "an author's two numbers were conflated");
}

/// The allowlist answers "may be paid", and only when somebody asked.
#[test]
fn payable_is_absent_rather_than_guessed() {
    let h = "33".repeat(32);
    let alice = Keypair::new().pubkey().to_string();
    let mk = |payable: Option<&BTreeSet<String>>| {
        ledger(&Inputs {
            table: &[attributed(&alice, &h)],
            index: &[entry(&h, "c.json")],
            eps: &BTreeMap::new(),
            live: &BTreeSet::new(),
            proven: &BTreeMap::new(),
            paid: &BTreeMap::new(),
            payable,
        })
    };
    assert_eq!(mk(None).authors[0].payable, None, "an answer was invented with nothing to go on");
    let empty = BTreeSet::new();
    assert_eq!(mk(Some(&empty)).authors[0].payable, Some(false), "an empty list pays nobody");
    let listed: BTreeSet<String> = [alice.clone()].into_iter().collect();
    assert_eq!(mk(Some(&listed)).authors[0].payable, Some(true));
}
