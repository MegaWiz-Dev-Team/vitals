//! Who wrote a case, and how many proven replays it has — the ledger a payment would read.
//!
//! `SYSTEM_DESIGN.md` §7 says authors are paid per replay. No money exists, `$VIGIL` does not
//! exist, and none of that is built. What is built is the counting, so that the claim can become
//! *"every proven replay of your case is already counted against your key"* without a single line
//! about currency anywhere near it.
//!
//! ## Why attribution lives outside the case file
//!
//! A case's identity **is** the sha256 of its bytes: `sce_hash` names it on chain, in every leaf,
//! and in `conformance/sce-archive/`. Adding an `author` field inside `demo/scenarios` or
//! `demo/stations` would rotate that hash, and with it every leaf already anchored — the archive
//! would need sweeping across both record layouts for a line of metadata that is not part of what
//! the case *is*.
//!
//! So authorship is a side table: `AUTHORS.json`, a sibling of `INDEX.json` in the same
//! directory. It can be written, corrected, and re-assigned without the case changing at all,
//! which is the property that matters — a case does not become a different case because we
//! learned who wrote it.
//!
//! ## Proven, not anchored
//!
//! The count is of **proven** replays and the word is load-bearing.
//!
//! `AnchorReplay` appends a leaf hash to an incremental Merkle tree. The tree holds a root, a
//! next index and the filled path — no case, no per-leaf record. Nothing on it can be attributed
//! to anything. `ProveAttempt` is what writes a [`vitals_program::ProvenAttempt`] into the
//! player's claim account, and *that* carries the case hash. So a per-case count can only ever be
//! of proven attempts.
//!
//! Anchored-without-proven is a real state, not a theoretical one: the two are separate
//! transactions signed together, and the second can fail on its own — the bug `reconcile_leaves`
//! exists to prevent used to cause exactly that. On devnet on 2026-09-03 the tree held 6 leaves
//! and the claim accounts held 6 proven attempts, so the two happened to agree and the gap was
//! invisible. **The day they differ, the ledger has not broken.** It is counting the replays
//! anyone can check, which is the only kind this project has any business counting.

//! ## Payout
//!
//! What the ledger counts, [`crate::payout`] pays: the author of a case is paid the moment a
//! replay of it is proven. The two halves are kept apart on purpose and neither is derived from
//! the other — `proven_replays` is what the chain accepted, `paid` is what actually left a
//! wallet, and a replay can be one without the other. No token exists, no royalty is owed, and
//! nothing here says "earn": an author is **paid per proven replay**, on devnet, in devnet SOL
//! that every surface showing a number calls illustrative.
//!
//! Being named and being payable are also different. A signature proves the holder of a key
//! consented to be named; `VITALS_PAYOUT_ALLOWLIST`, set by the operator and never in the
//! repository, says who may receive money. See [`crate::payout`] for why that separation exists
//! and what replaces it before mainnet.

use serde::{Deserialize, Serialize};

/// Signed over, so a signature here cannot be replayed as a signature for anything else.
const DOMAIN: &str = "vitals.author.v1\n";

/// Where the side table lives, relative to the repository root.
pub const AUTHORS_PATH: &str = "conformance/sce-archive/AUTHORS.json";
/// The archive index it must agree with.
pub const INDEX_PATH: &str = "conformance/sce-archive/INDEX.json";

/// One case, and the key that claims to have written it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribution {
    /// The case, by the hash that is its identity everywhere else.
    pub sce_hash: String,
    /// The author's Ed25519 public key, base58.
    pub author: String,
    /// Their signature over [`message`], base58.
    pub signature: String,
}

/// The bytes an author signs: the domain, then the hash as *bytes* rather than as its text.
///
/// Hex is a rendering. Signing the rendering would make a signature valid for one spelling of a
/// hash and not another, and both spellings name the same case.
pub fn message(sce_hash_hex: &str) -> Result<Vec<u8>, String> {
    let raw = unhex32(sce_hash_hex)?;
    let mut m = DOMAIN.as_bytes().to_vec();
    m.extend_from_slice(&raw);
    Ok(m)
}

fn unhex32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{s:?} is not a 64-character hex hash"));
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("{s:?}: {e}"))?;
    }
    Ok(out)
}

impl Attribution {
    /// Does this entry's signature actually say what it claims?
    pub fn verify(&self) -> Result<(), String> {
        let msg = message(&self.sce_hash)?;
        let key = bs58::decode_32(&self.author)
            .ok_or_else(|| format!("author {:?} is not a base58 32-byte key", self.author))?;
        let sig = bs58::decode_64(&self.signature)
            .ok_or_else(|| format!("signature for {} is not base58 64 bytes", self.sce_hash))?;
        solana_sdk::signature::Signature::from(sig)
            .verify(&key, &msg)
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "the signature on {} does not verify against {}",
                    self.sce_hash, self.author
                )
            })
    }
}

/// Read the side table. A file that is not there is an empty ledger, not an error — the format
/// and its checks ship before any key exists to sign with.
pub fn load(path: &std::path::Path) -> Result<Vec<Attribution>, String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
    }
}

/// Every case the archive knows, by hash.
pub fn archive_hashes(index: &std::path::Path) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct Entry {
        sce_hash: String,
    }
    let text = std::fs::read_to_string(index).map_err(|e| format!("{}: {e}", index.display()))?;
    let entries: Vec<Entry> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index.display()))?;
    Ok(entries.into_iter().map(|e| e.sce_hash).collect())
}

/// Check the whole table: every signature real, every case known, no case claimed twice.
///
/// Returns every complaint rather than the first, because someone fixing a signed file wants the
/// whole list in one pass.
pub fn audit(table: &[Attribution], known: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for a in table {
        if let Err(e) = a.verify() {
            problems.push(e);
        }
        if !known.iter().any(|h| h == &a.sce_hash) {
            problems.push(format!(
                "{} is attributed but is not in the archive index — a case nobody can fetch \
                 cannot be one anybody wrote",
                a.sce_hash
            ));
        }
        if seen.contains(&a.sce_hash.as_str()) {
            problems.push(format!(
                "{} is attributed twice. Re-assignment replaces an entry; it does not add one",
                a.sce_hash
            ));
        }
        seen.push(&a.sce_hash);
    }
    problems
}

/// Just enough base58 for two fixed widths, rather than a dependency for it.
mod bs58 {
    const A: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    fn decode(s: &str) -> Option<Vec<u8>> {
        let mut out: Vec<u8> = vec![0];
        for c in s.bytes() {
            let mut carry = A.iter().position(|&a| a == c)?;
            for byte in out.iter_mut() {
                carry += (*byte as usize) * 58;
                *byte = (carry & 0xff) as u8;
                carry >>= 8;
            }
            while carry > 0 {
                out.push((carry & 0xff) as u8);
                carry >>= 8;
            }
        }
        // Leading '1's are leading zero bytes, and they are significant in a key.
        let leading_zeros = s.bytes().take_while(|&b| b == b'1').count();
        out.extend(std::iter::repeat_n(0u8, leading_zeros));
        out.reverse();
        Some(out)
    }

    pub fn decode_32(s: &str) -> Option<[u8; 32]> {
        let v = decode(s)?;
        (v.len() == 32).then(|| {
            let mut a = [0u8; 32];
            a.copy_from_slice(&v);
            a
        })
    }

    pub fn decode_64(s: &str) -> Option<[u8; 64]> {
        let v = decode(s)?;
        (v.len() == 64).then(|| {
            let mut a = [0u8; 64];
            a.copy_from_slice(&v);
            a
        })
    }
}

// ── the ledger ──────────────────────────────────────────────────────────────
//
// **An author signs bytes. A case is a lineage.** Those are different things and the ledger
// keeps them apart on purpose.
//
// 1. A signature is over one `sce_hash`, exactly as it was built. You sign what you wrote, and a
//    version nobody signed counts for nobody.
// 2. A case is every archive entry sharing a `path` — 38 hashes over 17 paths today, sixteen of
//    them with more than one version and the deepest with four. The index is the lineage: no new
//    identifier was invented, because one more id for the same thing is one more thing that can
//    disagree.
// 3. What a reader sees on a card is the lineage: its total across every version, and the keys
//    that signed any of them. That is why osce-a shows its five replays even though they were
//    played against `4ee55216…` and the file on the shelf today is `ac52be1c…`.
// 4. **Replays never move between versions.** Whoever signed the version that was played keeps
//    those replays for good; whoever signs the live one collects from here. Whether a small edit
//    ought to carry credit forward is **OPEN** — a policy question that waits for an author who
//    is not us, and is deliberately not answered by a default in this code.
// 5. Signatures are per version and the lineage is derived from a committed file that has its own
//    verifier, so nothing published now has to be withdrawn when (4) is decided. That is the
//    property that makes it safe to sign before the policy exists.

use std::collections::{BTreeMap, BTreeSet};

/// One archived version of a case.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Version {
    pub sce_hash: String,
    /// The key that signed these bytes. Empty when nobody has.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    pub proven_replays: u64,
    /// Whether this is the version on the shelf right now.
    pub live: bool,
}

/// What a case's author has actually been paid, from the payout wallet's memos.
///
/// Separate from `proven_replays` on purpose, and never derived from it. A replay is proven when
/// the chain accepted its proof; a payment happened when a transfer landed. They are different
/// events with different failure modes — the cap, the allowlist, an unsigned attribution — and a
/// ledger that computed one from the other would be reporting an intention as a fact.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct Paid {
    /// Replays of this case that have been paid for.
    pub paid: u64,
    /// Lamports paid to the author for them.
    pub paid_lamports: u64,
}

/// One case, across every version of it the archive holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CaseLedger {
    /// The lineage. `INDEX.json`'s path, which is what makes these versions one case.
    pub path: String,
    /// The shelf card, from whichever version is live.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ep: String,
    /// Every proven replay of every version.
    pub proven_replays: u64,
    /// Keys that signed any version of this case, in order.
    pub authors: Vec<String>,
    #[serde(flatten)]
    pub paid: Paid,
    pub versions: Vec<Version>,
}

/// One key, and what it signed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthorLedger {
    pub author: String,
    /// Lineages, not versions: signing three revisions of one case is one case.
    pub distinct_cases: usize,
    /// Replays of the versions **this key signed** — see rule 4. Not the lineage total, which
    /// may include versions somebody else wrote.
    pub proven_replays: u64,
    #[serde(flatten)]
    pub paid: Paid,
    /// Whether the operator has authorised this key to receive money. `None` when nothing was
    /// asked — a tool run without the allowlist says nothing rather than guessing "no".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable: Option<bool>,
    pub cases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Ledger {
    pub cases: Vec<CaseLedger>,
    pub authors: Vec<AuthorLedger>,
}

/// An archive entry: the two fields the lineage is built from.
#[derive(Debug, Clone, Deserialize)]
pub struct IndexEntry {
    pub sce_hash: String,
    pub path: String,
}

/// Build both views from the archive, the attributions, and the chain's per-case counts.
///
/// Pure, so every way the arithmetic could be wrong is wrong here where a test can hand it a
/// known set and check the answer with no validator in the room.
///
/// A case with no replays still appears, at zero, and so does an author whose cases nobody has
/// played. Authorship is not a reward for popularity, and a ledger that hid unplayed cases would
/// be a leaderboard.
/// Everything `ledger` reads, named rather than positional — seven arguments in a row is how a
/// caller swaps two maps of the same type and nothing complains.
pub struct Inputs<'a> {
    pub table: &'a [Attribution],
    pub index: &'a [IndexEntry],
    /// Case hash → the shelf card it is, for whichever version is live.
    pub eps: &'a BTreeMap<String, String>,
    /// Case hashes that are the file on the shelf right now.
    pub live: &'a BTreeSet<String>,
    /// Case hash → proven attempts.
    pub proven: &'a BTreeMap<String, u64>,
    /// Case hash → what has actually been paid for it. Never derived from `proven`.
    pub paid: &'a BTreeMap<String, Paid>,
    /// Keys the operator authorised to be paid. `None` means nobody asked, and the answer is
    /// left out rather than guessed.
    pub payable: Option<&'a BTreeSet<String>>,
}

pub fn ledger(inputs: &Inputs) -> Ledger {
    let Inputs { table, index, eps, live, proven, paid, payable } = *inputs;
    let signed: BTreeMap<&str, &str> =
        table.iter().map(|a| (a.sce_hash.as_str(), a.author.as_str())).collect();

    let mut by_path: BTreeMap<&str, Vec<Version>> = BTreeMap::new();
    for e in index {
        by_path.entry(&e.path).or_default().push(Version {
            sce_hash: e.sce_hash.clone(),
            author: signed.get(e.sce_hash.as_str()).map(|a| a.to_string()).unwrap_or_default(),
            proven_replays: proven.get(&e.sce_hash).copied().unwrap_or(0),
            live: live.contains(&e.sce_hash),
        });
    }

    let mut cases: Vec<CaseLedger> = by_path
        .into_iter()
        .filter(|(_, versions)| versions.iter().any(|v| !v.author.is_empty()))
        .map(|(path, mut versions)| {
            // Live first, then most-replayed: the version on the shelf is the one a reader is
            // looking at, and after that the ones that were actually played.
            versions.sort_by(|a, b| {
                b.live.cmp(&a.live)
                    .then(b.proven_replays.cmp(&a.proven_replays))
                    .then(a.sce_hash.cmp(&b.sce_hash))
            });
            let mut authors: Vec<String> =
                versions.iter().filter(|v| !v.author.is_empty()).map(|v| v.author.clone()).collect();
            authors.sort();
            authors.dedup();
            // Summed over the lineage's versions, the same way replays are — a payment belongs
            // to the case it was made for, whichever revision was played.
            let money = versions.iter().fold(Paid::default(), |acc, v| {
                let p = paid.get(&v.sce_hash).copied().unwrap_or_default();
                Paid { paid: acc.paid + p.paid, paid_lamports: acc.paid_lamports + p.paid_lamports }
            });
            CaseLedger {
                ep: versions
                    .iter()
                    .find(|v| v.live)
                    .and_then(|v| eps.get(&v.sce_hash))
                    .cloned()
                    .unwrap_or_default(),
                proven_replays: versions.iter().map(|v| v.proven_replays).sum(),
                authors,
                paid: money,
                versions,
                path: path.to_string(),
            }
        })
        .collect();
    cases.sort_by(|a, b| b.proven_replays.cmp(&a.proven_replays).then(a.path.cmp(&b.path)));

    // Per key: only the versions that key signed. Rule 4 — replays never move.
    let mut by_author: BTreeMap<String, (u64, Paid, BTreeSet<String>)> = BTreeMap::new();
    for c in &cases {
        for v in &c.versions {
            if v.author.is_empty() {
                continue;
            }
            let e = by_author.entry(v.author.clone()).or_insert((0, Paid::default(), BTreeSet::new()));
            e.0 += v.proven_replays;
            let p = paid.get(&v.sce_hash).copied().unwrap_or_default();
            e.1.paid += p.paid;
            e.1.paid_lamports += p.paid_lamports;
            e.2.insert(c.path.clone());
        }
    }
    let authors = by_author
        .into_iter()
        .map(|(author, (proven_replays, money, paths))| AuthorLedger {
            payable: payable.map(|a| a.contains(&author)),
            author,
            distinct_cases: paths.len(),
            proven_replays,
            paid: money,
            cases: paths.into_iter().collect(),
        })
        .collect();

    Ledger { cases, authors }
}

/// Every archive entry, for the lineage.
pub fn archive_entries(index: &std::path::Path) -> Result<Vec<IndexEntry>, String> {
    let text = std::fs::read_to_string(index).map_err(|e| format!("{}: {e}", index.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index.display()))
}
