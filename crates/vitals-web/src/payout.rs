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

/// What a payout memo says: which leaf, and what each side received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoRecord {
    pub leaf: String,
    pub author_lamports: u64,
    pub platform_lamports: u64,
}

/// The memo a payout carries.
///
/// The amounts are in it so the day's spend is a sum taken from the chain rather than a number
/// the server keeps — exact even when the rate changes mid-day, and readable by anyone auditing
/// what an author was paid without asking us for a total.
pub fn memo(leaf_hex: &str, author_lamports: u64, platform_lamports: u64) -> String {
    format!("{MEMO_PREFIX}{leaf_hex} {author_lamports} {platform_lamports}")
}

/// The leaf a memo names, if it is one of ours. See [`parse_memo`] for the whole record.
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

/// The whole record a memo carries: the leaf, and what each side was paid.
///
/// Amounts that are missing or unreadable come back as zero rather than failing the parse — an
/// older memo that named only a leaf is still a record that the leaf was paid, and forgetting
/// that would pay it a second time. A leaf that cannot be read is not a record at all.
pub fn parse_memo(memo: &str) -> Option<MemoRecord> {
    let leaf = leaf_from_memo(memo)?;
    let tail: Vec<&str> = memo
        .split("; ")
        .find(|p| p.contains(&leaf))
        .unwrap_or("")
        .split_whitespace()
        .skip_while(|w| *w != leaf.as_str())
        .skip(1)
        .collect();
    let n = |i: usize| tail.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    Some(MemoRecord { leaf, author_lamports: n(0), platform_lamports: n(1) })
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
    instruction::{AccountMeta, Instruction},
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
    /// Signatures already looked up, and what they turned out to be. `None` is a transaction
    /// that touched this wallet and was not one of its payouts — an inbound transfer, say — and
    /// is cached so it is never fetched twice.
    seen: std::sync::Mutex<std::collections::BTreeMap<String, Option<(MemoRecord, i64)>>>,
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

/// What this wallet has paid, taken from the chain.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    /// Every leaf this wallet has paid for.
    pub paid: BTreeSet<String>,
    /// Lamports it has paid out today, Bangkok time — summed from the memos, so it is exact even
    /// if the rate changed during the day, and it needs nothing remembered between restarts.
    pub spent_today: u64,
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
        // Everything that is only a matter of reading the environment is checked first, before
        // any file or network. A typo should be reported as a typo whatever else is also wrong,
        // and none of this costs anything.
        let platform_bps = bps_env("VITALS_PLATFORM_BPS", 1500)?;
        let daily_cap = parsed_env("VITALS_PAYOUT_DAILY_CAP_LAMPORTS", 100_000_000)?;

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
            platform_bps,
            daily_cap,
            allowlist,
            seen: Default::default(),
            just_paid: Default::default(),
        }))
    }

    pub fn address(&self) -> String {
        self.key.pubkey().to_string()
    }

    pub fn balance(&self) -> u64 {
        self.rpc.get_balance(&self.key.pubkey()).unwrap_or(0)
    }

    /// What this wallet has paid, and how much of it today.
    ///
    /// **A memo is only ours if this wallet paid the fee for the transaction carrying it.**
    /// `getSignaturesForAddress` lists everything that touches an address, inbound included, so
    /// anyone could send one lamport here with a memo naming a leaf and that leaf would read as
    /// paid for ever — an author silenced for the price of a lamport, and the day's spend
    /// inflated to the cap for not much more. The leaf is public on the explorer, so the attack
    /// needs nothing secret.
    ///
    /// The memo instruction also carries this key as a required signer, so a payout of ours is
    /// self-describing on chain. That is the writing half; this fee-payer check is the reading
    /// half, and it is the one that has to hold, because nothing stops a stranger writing our
    /// prefix into a memo of their own.
    ///
    /// Each signature is looked up once and remembered — a thousand at startup, then only what
    /// is new.
    pub fn ledger(&self) -> Result<Ledger, String> {
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
        let today = crate::usage::bangkok_day(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        let mut paid = BTreeSet::new();
        let mut spent_today = 0u64;
        for s in sigs.iter().filter(|s| s.err.is_none()) {
            let Some((rec, when)) = self.record_for(&s.signature, s.memo.as_deref(), s.block_time)?
            else {
                continue;
            };
            paid.insert(rec.leaf.clone());
            if when > 0 && crate::usage::bangkok_day(when as u64) == today {
                spent_today = spent_today
                    .saturating_add(rec.author_lamports)
                    .saturating_add(rec.platform_lamports);
            }
        }
        // Plus whatever this process paid while the index was catching up.
        if let Ok(recent) = self.just_paid.lock() {
            paid.extend(recent.iter().cloned());
        }
        Ok(Ledger { paid, spent_today })
    }

    /// Is this signature one of our payouts, and when? Cached both ways.
    fn record_for(
        &self,
        signature: &str,
        memo: Option<&str>,
        block_time: Option<i64>,
    ) -> Result<Option<(MemoRecord, i64)>, String> {
        if let Ok(seen) = self.seen.lock() {
            if let Some(hit) = seen.get(signature) {
                return Ok(hit.clone());
            }
        }
        // Cheap first: no memo of ours in the listing means no lookup at all.
        let verdict = match memo.and_then(parse_memo) {
            None => None,
            Some(rec) => {
                if self.we_paid_the_fee(signature)? {
                    Some((rec, block_time.unwrap_or(0)))
                } else {
                    // Somebody else's memo wearing our prefix. Ignored, and said out loud once,
                    // because it is the shape of an attempt to stop an author being paid.
                    eprintln!(
                        "payout: ignoring a memo this wallet did not pay for — {signature} names \
                         leaf {}",
                        rec.leaf
                    );
                    None
                }
            }
        };
        if let Ok(mut seen) = self.seen.lock() {
            seen.insert(signature.to_string(), verdict.clone());
        }
        Ok(verdict)
    }

    /// Whether this wallet is the fee payer of a transaction — the first account key is, by
    /// definition, and that is a fact the transaction carries rather than one anyone asserts.
    fn we_paid_the_fee(&self, signature: &str) -> Result<bool, String> {
        use solana_sdk::signature::Signature as Sig;
        use solana_transaction_status_client_types::UiTransactionEncoding;
        let sig = Sig::from_str(signature).map_err(|e| format!("{signature}: {e}"))?;
        let tx = self
            .rpc
            .get_transaction(&sig, UiTransactionEncoding::Base64)
            .map_err(|e| format!("cannot read {signature}, so not trusting its memo: {e}"))?;
        Ok(tx
            .transaction
            .transaction
            .decode()
            .and_then(|t| t.message.static_account_keys().first().copied())
            .is_some_and(|payer| payer == self.key.pubkey()))
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
        // This key is a required signer on the memo, so a payout of ours is self-describing on
        // chain rather than only recognisable by who paid the fee. SPL Memo verifies every
        // account handed to it, so nobody else can produce this instruction.
        ixs.push(Instruction {
            program_id: self.memo_program,
            accounts: vec![AccountMeta::new_readonly(self.key.pubkey(), true)],
            data: memo(leaf, split.author, split.platform).into_bytes(),
        });

        let bh = self.rpc.get_latest_blockhash().map_err(|e| e.to_string())?;
        let msg = Message::new_with_blockhash(&ixs, Some(&self.key.pubkey()), &bh);
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_sign(&[&self.key], bh).map_err(|e| e.to_string())?;

        // Held **before** it is sent, and kept whatever happens next.
        //
        // `send_and_confirm_transaction` can time out on a transaction that landed. If the leaf
        // were only recorded on success, that timeout plus the index lag is a window in which the
        // next pass pays it again. Missing a payout is recoverable — the memos say what was paid,
        // so an operator can see a proven leaf with no payment and settle it deliberately.
        // Paying twice is not recoverable at all.
        if let Ok(mut recent) = self.just_paid.lock() {
            recent.insert(leaf.to_lowercase());
        }
        let sig = match self.rpc.send_and_confirm_transaction(&tx) {
            Ok(sig) => sig,
            Err(e) => {
                eprintln!(
                    "payout: outcome unknown for leaf {leaf} — held until restart, and the chain \
                     scan is the record: {e}"
                );
                return Err(format!("the payout's outcome is unknown: {e}"));
            }
        };
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

/// A number, or a refusal. **Never a default standing in for a typo.**
///
/// A mistyped cap that silently becomes the default is a spending limit nobody set, and the
/// operator would have no way to know: the deploy comes up, payouts flow, and the number in the
/// environment is not the number in force.
fn parsed_env(name: &str, default: u64) -> Result<u64, String> {
    match std::env::var(name) {
        Err(_) => Ok(default),
        Ok(v) => v
            .trim()
            .parse()
            .map_err(|_| format!("{name} is {v:?}, which is not a number of lamports")),
    }
}

/// The platform's share, refusing anything that would leave the author with nothing.
fn bps_env(name: &str, default: u32) -> Result<u32, String> {
    let v = match std::env::var(name) {
        Err(_) => return Ok(default),
        Ok(v) => v
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("{name} is {v:?}, which is not basis points"))?,
    };
    if v >= 10_000 {
        return Err(format!(
            "{name} is {v}, which leaves the author nothing. The platform's share is taken from              the rate, so it has to be less than all of it."
        ));
    }
    Ok(v)
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
