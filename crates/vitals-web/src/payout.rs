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
///
/// **The RPC does not hand back the memo as it was written.** `getSignaturesForAddress` returns
/// it framed with the instruction's length — `[81] vitals.payout.v1 <leaf>` — and, when a
/// transaction carries more than one, joined with `; `. A parser written against the string we
/// sent passes every unit test and then finds nothing at all on a real cluster, which is exactly
/// what happened here on the first live payout.
///
/// So a leading `[N] ` marker is stripped, and each `; `-separated part is tried. Anything else
/// in front is still refused: a length marker is the transport's, arbitrary text is somebody's.
pub fn leaf_from_memo(memo: &str) -> Option<String> {
    memo.split("; ").find_map(|part| {
        let part = part.trim();
        // The RPC's own framing, and only in the shape the RPC writes it.
        let part = match part.strip_prefix('[') {
            Some(rest) => match rest.split_once("] ") {
                Some((len, text)) if !len.is_empty() && len.bytes().all(|b| b.is_ascii_digit()) => text,
                _ => part,
            },
            None => part,
        };
        let leaf = part.strip_prefix(MEMO_PREFIX)?.split_whitespace().next()?;
        (leaf.len() == 64 && leaf.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| leaf.to_lowercase())
    })
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

// ── the chain half ──────────────────────────────────────────────────────────

use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use std::str::FromStr;

/// SPL Memo v2 — verified on devnet as an executable account under BPFLoader2.
const MEMO_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

/// How many signatures to read back when rebuilding the paid set. One request's worth: past this
/// the scan pages, and a payout key that has made a thousand payments needs a different design
/// than a timer anyway.
const SCAN_LIMIT: usize = 1000;

/// The wallet that pays authors, and nothing else.
///
/// Deliberately not the relay. The relay pays network fees and never plays; this pays authors and
/// never signs attributions and never plays. A bug in one cannot spend the other's budget, and
/// that separation is the only reason either budget can be reasoned about.
pub struct Payer {
    rpc: RpcClient,
    key: Keypair,
    memo_program: Pubkey,
    platform: Option<Pubkey>,
    pub rate: u64,
    pub platform_bps: u32,
    pub daily_cap: u64,
    pub allowlist: BTreeSet<String>,
    /// Leaves this process has paid but the RPC has not indexed yet.
    ///
    /// `getSignaturesForAddress` is an index, and an index lags. Measured on devnet: a payout
    /// confirmed and its memo was still absent from the listing moments later. A paid set read
    /// only from the chain therefore has a window in which it does not know about a payment that
    /// has already gone out — and anything retrying inside that window would pay twice.
    ///
    /// So this is unioned into every read. It is not a second ledger: it only ever adds, it is
    /// lost on restart, and the chain remains the thing that is true. It closes a window; it does
    /// not open a competing record.
    just_paid: std::sync::Mutex<BTreeSet<String>>,
}

/// What a payout did, once the chain accepted it.
#[derive(Debug, Clone, Serialize)]
pub struct Paid {
    pub leaf: String,
    pub author: String,
    pub author_lamports: u64,
    pub platform_lamports: u64,
    pub signature: String,
}

impl Payer {
    /// Read the configuration, and refuse to exist unless every part of it is safe.
    ///
    /// Returns `Ok(None)` when payouts are simply off — rate 0 needs no key, no cluster check and
    /// no wallet, and the whole subsystem stays inert. `Err` is a configuration that asks for
    /// payouts and cannot have them safely, which must stop a deploy rather than degrade quietly.
    pub fn from_env(rpc_url: &str) -> Result<Option<Payer>, String> {
        let rate = num_env("VITALS_PAYOUT_LAMPORTS");
        if rate == 0 {
            return Ok(None);
        }
        let path = std::env::var("VITALS_PAYOUT_KEY")
            .map_err(|_| "VITALS_PAYOUT_LAMPORTS is set but VITALS_PAYOUT_KEY is not. There is \
                          deliberately no default: a wallet that pays people has no business \
                          living anywhere this could guess.".to_string())?;
        refuse_inside_repo(std::path::Path::new(&path))?;
        let key = read_keypair_file(&path).map_err(|e| format!("{path}: {e}"))?;

        let rpc = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
        // The line that keeps this off mainnet. Asked of the cluster itself rather than inferred
        // from a URL, because a URL is a string somebody can point anywhere.
        let genesis = rpc
            .get_genesis_hash()
            .map_err(|e| format!("cannot check which cluster this is, so not paying anyone: {e}"))?
            .to_string();
        if genesis != DEVNET_GENESIS {
            return Err(format!(
                "payouts are devnet-only and this cluster's genesis is {genesis}. Refusing to \
                 start. Moving real money is a decision somebody makes on purpose, not something \
                 a config file does on the way past."
            ));
        }

        let allowlist: BTreeSet<String> = std::env::var("VITALS_PAYOUT_ALLOWLIST")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let platform = match std::env::var("VITALS_PLATFORM_ADDRESS") {
            Ok(a) if !a.trim().is_empty() => Some(
                Pubkey::from_str(a.trim()).map_err(|e| format!("VITALS_PLATFORM_ADDRESS: {e}"))?,
            ),
            // Unset means the platform's share simply stays where it already is. No transfer, no
            // pretend address, nothing that looks like a payment nobody receives.
            _ => None,
        };

        Ok(Some(Payer {
            rpc,
            key,
            memo_program: Pubkey::from_str(MEMO_PROGRAM).expect("a compiled-in address"),
            platform,
            rate,
            platform_bps: std::env::var("VITALS_PLATFORM_BPS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1500),
            daily_cap: match std::env::var("VITALS_PAYOUT_DAILY_CAP_LAMPORTS") {
                Ok(v) => v.parse().unwrap_or(100_000_000),
                Err(_) => 100_000_000,
            },
            allowlist,
            just_paid: Default::default(),
        }))
    }

    pub fn address(&self) -> String {
        self.key.pubkey().to_string()
    }

    pub fn balance(&self) -> u64 {
        self.rpc.get_balance(&self.key.pubkey()).unwrap_or(0)
    }

    /// Every leaf this wallet has already paid for, read from its own transactions.
    ///
    /// The memo rides in the signature listing itself, so this is one request rather than one per
    /// payment. This is the truth about what has been paid; anything the server remembers is a
    /// cache in front of it.
    pub fn paid_leaves(&self) -> Result<BTreeSet<String>, String> {
        let sigs = self
            .rpc
            .get_signatures_for_address(&self.key.pubkey())
            .map_err(|e| format!("cannot read this wallet's history, so not paying: {e}"))?;
        if sigs.len() >= SCAN_LIMIT {
            // One page is all this reads. Past it the oldest payments fall out of view and a
            // leaf could be paid a second time, so say so loudly rather than pay from a
            // half-read history.
            return Err(format!(
                "this wallet has {} or more transactions and the paid set is read one page at a                  time — refusing to pay from a history that may be missing its oldest entries.                  The scan needs paging before this wallet is used further.",
                sigs.len()
            ));
        }
        let mut paid = paid_from_memos(
            sigs.iter().filter(|s| s.err.is_none()).filter_map(|s| s.memo.as_deref()),
        );
        // Plus whatever this process paid while the index was catching up.
        if let Ok(recent) = self.just_paid.lock() {
            paid.extend(recent.iter().cloned());
        }
        Ok(paid)
    }

    /// One transaction: the transfers, and the memo that says what they were for.
    ///
    /// The memo is not decoration. It is the entire record — nothing else, anywhere, says this
    /// leaf was paid. A payout whose memo failed to attach would be money out with no way to know
    /// it, so the memo is an instruction in the same transaction rather than a separate step.
    pub fn pay(&self, leaf: &str, author: &str, split: Split) -> Result<Paid, String> {
        let to = Pubkey::from_str(author).map_err(|e| format!("author {author}: {e}"))?;
        let mut ixs = vec![system_instruction::transfer(&self.key.pubkey(), &to, split.author)];
        if let (Some(platform), true) = (self.platform, split.platform > 0) {
            ixs.push(system_instruction::transfer(&self.key.pubkey(), &platform, split.platform));
        }
        ixs.push(Instruction {
            program_id: self.memo_program,
            accounts: vec![],
            data: memo(leaf).into_bytes(),
        });

        let bh = self.rpc.get_latest_blockhash().map_err(|e| e.to_string())?;
        let msg = Message::new_with_blockhash(&ixs, Some(&self.key.pubkey()), &bh);
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_sign(&[&self.key], bh).map_err(|e| e.to_string())?;
        let sig = self
            .rpc
            .send_and_confirm_transaction(&tx)
            .map_err(|e| format!("the payout did not land: {e}"))?;

        // Recorded before it is reported, so the next decision sees it even if the RPC's index
        // has not caught up yet.
        if let Ok(mut recent) = self.just_paid.lock() {
            recent.insert(leaf.to_lowercase());
        }
        Ok(Paid {
            leaf: leaf.to_string(),
            author: author.to_string(),
            author_lamports: split.author,
            platform_lamports: if self.platform.is_some() { split.platform } else { 0 },
            signature: sig.to_string(),
        })
    }
}

fn num_env(name: &str) -> u64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

/// A wallet key inside the repository is refused, both ends, exactly as the author key is.
fn refuse_inside_repo(key: &std::path::Path) -> Result<(), String> {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(repo) = repo.canonicalize() else { return Ok(()) };
    let asked = key.canonicalize().unwrap_or_else(|_| key.to_path_buf());
    let through = key
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(key.file_name().unwrap_or_default()))
        .unwrap_or_else(|| key.to_path_buf());
    for c in [asked, through] {
        if c.starts_with(&repo) {
            return Err(format!(
                "{} is inside this repository. A wallet that pays people is one `git add -A` \
                 from being published; keep it somewhere the repository cannot reach.",
                c.display()
            ));
        }
    }
    Ok(())
}
