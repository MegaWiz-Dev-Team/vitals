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

// ── the tally ───────────────────────────────────────────────────────────────

/// One case on an author's ledger.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoredCase {
    pub sce_hash: String,
    /// Where the case lives, from the archive index — so a reader can fetch and hash it.
    pub path: String,
    /// Proven replays of this case on this tree. See [`crate::authors`] on why not anchored.
    pub proven_replays: u64,
}

/// What one author key has to its name.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthorLedger {
    pub author: String,
    pub distinct_cases: usize,
    pub proven_replays: u64,
    pub cases: Vec<AuthoredCase>,
}

/// Join the side table to the chain's per-case counts.
///
/// Pure on purpose: everything that could be wrong about the arithmetic is wrong here, where a
/// test can hand it a known set and check the answer without a validator in the room. The chain
/// reading lives in [`crate::chain`]; this only adds up.
///
/// A case with no proven replays still appears, at zero. Authorship is not a reward for
/// popularity, and an author whose cases nobody has replayed yet has still written them — a
/// ledger that hides them would be a leaderboard.
pub fn tally(
    table: &[Attribution],
    paths: &std::collections::BTreeMap<String, String>,
    proven: &std::collections::BTreeMap<String, u64>,
) -> Vec<AuthorLedger> {
    let mut by_author: std::collections::BTreeMap<String, Vec<AuthoredCase>> = Default::default();
    for a in table {
        by_author.entry(a.author.clone()).or_default().push(AuthoredCase {
            sce_hash: a.sce_hash.clone(),
            path: paths.get(&a.sce_hash).cloned().unwrap_or_default(),
            proven_replays: proven.get(&a.sce_hash).copied().unwrap_or(0),
        });
    }
    by_author
        .into_iter()
        .map(|(author, mut cases)| {
            // Most-replayed first, then by hash, so the same inputs always print the same way —
            // a tally somebody is going to diff against a re-derivation must not reorder itself.
            cases.sort_by(|a, b| {
                b.proven_replays.cmp(&a.proven_replays).then(a.sce_hash.cmp(&b.sce_hash))
            });
            AuthorLedger {
                author,
                distinct_cases: cases.len(),
                proven_replays: cases.iter().map(|c| c.proven_replays).sum(),
                cases,
            }
        })
        .collect()
}

/// Every case in the archive index, by hash, with where it lives.
pub fn archive_paths(
    index: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    #[derive(Deserialize)]
    struct Entry {
        sce_hash: String,
        path: String,
    }
    let text = std::fs::read_to_string(index).map_err(|e| format!("{}: {e}", index.display()))?;
    let entries: Vec<Entry> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index.display()))?;
    Ok(entries.into_iter().map(|e| (e.sce_hash, e.path)).collect())
}
