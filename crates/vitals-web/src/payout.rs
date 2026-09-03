//! Paying a case's author the moment a run of it is proven.
//!
//! `SYSTEM_DESIGN.md` §7 says authors are paid per replay. This is that sentence becoming a
//! transfer: a student finishes, the proof lands, and seconds later the author's wallet shows it.
//! No token, no royalty, no "earn" — an author is **paid per proven replay**, and on devnet the
//! rate is illustrative devnet SOL, which every surface that shows a number says out loud.
//!
//! ## Why the payout is its own transaction
//!
//! **Our money must never be able to lose their proof.** Bundled into `ProveAttempt`, a payout
//! that fails — an empty wallet, a cap reached, a misconfigured address — takes the student's
//! proof down with it, and that is the priority exactly inverted. A payout is ours to get wrong;
//! a proof is theirs to keep.
//!
//! The packet limit settles it independently: `chain.rs` records that anchor and prove were
//! already split when the pair reached 1,656 bytes against a 1,232-byte limit, and `ProveAttempt`
//! still carries the record plus a 384-byte Merkle path. There is no room to add transfers.
//!
//! ## Who may be paid, in two layers
//!
//! A signature in `AUTHORS.json` proves that **the holder of a key consented to be named** as a
//! case's author. It does not prove they wrote it: `authors::audit` checks that a signature is
//! real, that the case is in the archive, and that no case is claimed twice — it cannot check a
//! fact about the world. Anyone could sign a valid entry for any case in the index.
//!
//! So payability is a second, separate thing:
//!
//!   1. the attribution says who is named — published, checkable by anyone, in the repository;
//!   2. **`VITALS_PAYOUT_ALLOWLIST` says who may be paid** — set by the operator at deploy,
//!      never in the repository, and **empty by default, which pays nobody**. An attribution is
//!      displayed whether or not it is payable; being named and being paid are different claims.
//!
//! Without (2), a commit to `AUTHORS.json` would be a payment instruction, and "somebody reviewed
//! the pull request" would be the only thing standing between a signature and money. That is a
//! weaker gate than everything else in this system, all of which proves itself from a chain or
//! from the hash of a file.
//!
//! **OPEN, for mainnet, and deliberately not built now:** replace the allowlist with a company
//! countersignature stored in each attribution entry — the same domain string, the case hash and
//! the author's pubkey, signed by the operator — so that payability is provable from the file
//! alone rather than from an environment variable nobody outside the deploy can see.
//!
//! ## The chain is the ledger
//!
//! Every payout carries a memo, [`MEMO_PREFIX`] followed by the leaf's hex. Nothing else records
//! that a payout happened: the paid set is read back from the payout key's own transactions, so
//! it survives a restart, and a stranger can re-derive every payment this project has ever made
//! without asking us for anything. A local set is a cache in front of that and never the truth —
//! the same rule `reconcile_leaves` follows about the leaf list.

use serde::Serialize;
use std::collections::BTreeSet;

/// What a payout memo starts with. The version is in it because the day this format changes,
/// every payout ever made still has to be readable by whatever reads it next.
pub const MEMO_PREFIX: &str = "vitals.payout.v1 ";

/// Devnet's genesis hash. The payout subsystem refuses to run against any other cluster, which is
/// the line that keeps this off mainnet until somebody deliberately moves it.
pub const DEVNET_GENESIS: &str = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";

/// How the rate divides. Integer arithmetic only: money in floats is how a fraction of a lamport
/// becomes a discrepancy nobody can account for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Split {
    pub author: u64,
    pub platform: u64,
}

/// `platform = rate * bps / 10000`, and the author takes the remainder.
///
/// The remainder rather than a second multiplication, so the two halves always sum to the rate
/// exactly — rounding can lose a lamport, and it must lose it from the platform's side, never
/// from the author's total.
pub fn split(rate: u64, platform_bps: u32) -> Split {
    let platform = (rate as u128 * platform_bps as u128 / 10_000) as u64;
    Split { author: rate.saturating_sub(platform), platform }
}

/// The memo a payout carries.
pub fn memo(leaf_hex: &str) -> String {
    format!("{MEMO_PREFIX}{leaf_hex}")
}

/// The leaf a memo names, if it is one of ours.
pub fn leaf_from_memo(memo: &str) -> Option<String> {
    let rest = memo.trim().strip_prefix(MEMO_PREFIX)?;
    let leaf = rest.split_whitespace().next()?;
    (leaf.len() == 64 && leaf.bytes().all(|b| b.is_ascii_hexdigit())).then(|| leaf.to_lowercase())
}

/// Every leaf these memos say has been paid.
pub fn paid_from_memos<'a>(memos: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    memos.into_iter().filter_map(leaf_from_memo).collect()
}

/// Whether to pay for a proven leaf, and why not when not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pay(Split),
    /// Not paid, and the reason, which is always logged and never queued for later. A payout
    /// that did not happen is a payout that did not happen; a queue would be a second ledger
    /// disagreeing with the chain.
    Skip(String),
}

/// Everything the decision depends on, in one place so the decision can be a pure function.
pub struct Ask<'a> {
    pub rate: u64,
    pub platform_bps: u32,
    /// Keys the operator has authorised to receive money. Empty pays nobody.
    pub allowlist: &'a BTreeSet<String>,
    /// The author this leaf's case is attributed to, if any.
    pub author: Option<&'a str>,
    pub leaf: &'a str,
    /// Leaves already paid, read from the chain's memos.
    pub paid: &'a BTreeSet<String>,
    pub spent_today: u64,
    pub daily_cap: u64,
    pub balance: u64,
}

/// The whole policy, with no clock, no network and no key in it.
///
/// Ordered so the cheapest and most absolute refusals come first, and so the reason a payout did
/// not happen is the most specific true one rather than whichever check ran first by accident.
pub fn decide(ask: &Ask) -> Verdict {
    if ask.rate == 0 {
        return Verdict::Skip("payouts are off — VITALS_PAYOUT_LAMPORTS is 0".into());
    }
    if ask.paid.contains(ask.leaf) {
        return Verdict::Skip(format!("already paid — the chain has a memo for leaf {}", ask.leaf));
    }
    let Some(author) = ask.author else {
        return Verdict::Skip(format!("no attribution for the case behind leaf {}", ask.leaf));
    };
    if !ask.allowlist.contains(author) {
        return Verdict::Skip(format!("attributed, not payable — {author} is not in the allowlist"));
    }
    let split = split(ask.rate, ask.platform_bps);
    if ask.daily_cap > 0 && ask.spent_today.saturating_add(ask.rate) > ask.daily_cap {
        return Verdict::Skip(format!(
            "payout cap reached — {} of {} lamports spent today",
            ask.spent_today, ask.daily_cap
        ));
    }
    // Twice the rate, so a wallet is never drained to the point where the next payout is the one
    // that fails: running out is a state to see coming, not to discover.
    if ask.balance < ask.rate.saturating_mul(2) {
        return Verdict::Skip(format!(
            "balance {} is below twice the rate — not paying down to empty",
            ask.balance
        ));
    }
    Verdict::Pay(split)
}
