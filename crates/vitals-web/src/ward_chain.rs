//! Reading the ward off the chain.
//!
//! Everything `/api/ward` publishes is derived here, and the rule the endpoint makes to its
//! readers is the rule this module has to keep: **no number the server keeps for itself.** The
//! patients come from patient accounts, the shifts from anchored leaves in transaction history,
//! and the keys from the signers of those leaves. Nothing increments a counter, so nothing can
//! drift — and when the chain cannot be read, the answer is [`crate::ward::ward_unavailable`] and
//! never a zero, because a zero looks exactly like a ward nobody came to.
//!
//! **Why history at all, when the patient account already counts her shifts.** It does, and the
//! count would be cheaper — but a count cannot say *who*. The one claim the ward makes about
//! strangers is that the record says which key did what, so the signers have to be read, and once
//! they are read the shifts may as well be counted from the same place rather than from a second
//! source that could disagree with it.
//!
//! History is walked once. Each patient's read stops at the newest signature already seen
//! ([`Seen::until`]), so a ward that has been open for a month costs the same per tick as one
//! opened this morning, and the cache lives wherever [`crate::store::Store`] lives — Firestore on
//! Cloud Run, where it survives the restarts a serverless host does without telling anyone.

use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use solana_rpc_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_rpc_client_api::{
    client_error::Error as RpcError,
    config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    filter::RpcFilterType,
};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::{AccountMeta, Instruction as SolInstruction},
    message::Message,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    system_program,
    transaction::Transaction,
};
use solana_transaction_status_client_types::UiTransactionEncoding;
use std::str::FromStr;
use vitals_program::{
    commitment_pda, patient_pda, tree_pda, Instruction, PatientAccount, RecordWire, PATIENT_LEN,
    SEED_ACCOUNT,
};

use crate::ward::{PatientOnChain, ShiftOnChain};

/// Eternal's program, which this sprint never reads and never upgrades (CWF_PLAN.md ruling 7).
///
/// Named here so that pointing the ward at it is refused with a sentence rather than answered with
/// a census of somebody else's entry. The failure it prevents is not a crash: it is a ward board
/// full of plausible numbers about the wrong program.
pub const ETERNAL_PROGRAM_ID: &str = "535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG";

/// One patient account as the census reads it.
///
/// `None` for anything that is not a patient — a truncated account, another of this program's
/// account types that happens to be nearby, a byte layout that drifted from the program's. Failing
/// to a `None` rather than to a default is the whole point: a default would be counted.
pub fn decode_patient(data: &[u8]) -> Option<PatientOnChain> {
    if data.len() < PATIENT_LEN {
        return None;
    }
    let p = PatientAccount::deserialize(&mut &data[..]).ok()?;
    Some(PatientOnChain {
        patient_id: p.patient_id,
        state: p.state,
        shifts: p.shifts,
        admitted_slot: p.admitted_slot,
        closed_slot: p.closed_slot,
        lease_holder: p.lease_holder,
        lease_until_slot: p.lease_until_slot,
    })
}

/// What `AnchorShift` names, in the program's own order: operator, player, account, tree,
/// commitment, patient, system.
pub const ANCHOR_SHIFT_ACCOUNTS: usize = 7;

/// The anchored shift in one instruction, if that is what it is.
///
/// `program` is the program the instruction actually called and `ours` is the ward's; they are
/// separate arguments because a transaction carries instructions to other programs and their bytes
/// can decode as anything. Only [`Instruction::AnchorShift`] is a shift: taking the head is a
/// lease, not work, and a ward that counted leases would publish shifts nobody played.
///
/// The signer is the **player**, account index 1 in the program's own order for this instruction
/// (operator, player, account, tree, commitment, patient, system). The operator at index 0 pays
/// and signs too, and crediting it would attribute every stranger's shift on the ward to us. An
/// instruction that names too few accounts yields `None` — a shift with a guessed signer is worse
/// than no shift.
pub fn shift_in(
    program: &Pubkey,
    ours: &Pubkey,
    data: &[u8],
    accounts: &[Pubkey],
    slot: u64,
) -> Option<ShiftOnChain> {
    if program != ours {
        return None;
    }
    let (patient_id, run_hash) = match Instruction::deserialize(&mut &data[..]).ok()? {
        Instruction::AnchorShift { patient_id, record, .. } => (patient_id, record.run_hash),
        _ => return None,
    };
    // The program takes exactly these seven, and it is the account list rather than the data that
    // says an instruction is shaped like an anchor. An instruction carrying AnchorShift's bytes
    // over a shorter list is one the program refused, so it anchored nothing — and reading a
    // signer out of it would credit whatever key happened to sit at index 1.
    if accounts.len() < ANCHOR_SHIFT_ACCOUNTS {
        return None;
    }
    let player = accounts.get(1)?;
    Some(ShiftOnChain { patient_id, signer: player.to_bytes(), slot, run_hash })
}

/// One shift and the signature it was read from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeenShift {
    pub signature: String,
    pub shift: ShiftOnChain,
}

/// One patient's transaction history, as far as it has been read.
///
/// Two failure modes, both silent, and this type exists for them: reading a signature twice
/// invents a shift that nobody played, and missing one loses a stranger's work off the census.
/// So a signature is the identity of a shift, [`Seen::absorb`] is idempotent, and the next read
/// starts where the last one stopped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Seen {
    /// The newest signature read so far — `until` for the next `getSignaturesForAddress`.
    until: Option<String>,
    /// The slot that signature landed in. Kept so "newest" is decided by the chain's own ordering
    /// rather than by the order pages happened to arrive in.
    #[serde(default)]
    until_slot: u64,
    shifts: Vec<SeenShift>,
}

impl Seen {
    /// Where the next read stops. `None` on a patient nobody has read yet — walk her whole history.
    pub fn until(&self) -> Option<String> {
        self.until.clone()
    }

    /// Walk a page of her history from the oldest entry forward, stopping at the first one that
    /// cannot be read.
    ///
    /// `page` is newest-first, as `getSignaturesForAddress` answers. Returns what was read, the
    /// cursor to stop at next time, and the trouble that stopped it.
    ///
    /// **Stopping rather than skipping is the whole point.** A skipped entry would take the cursor
    /// past it, and the shift underneath — anchored, paid for, on chain — would never be read
    /// again: absent from the census for ever, silently. Stopping costs one re-read of a handful
    /// of transactions and loses nothing.
    pub fn walk(
        page: Vec<(String, u64)>,
        budget: &Budget,
        mut read: impl FnMut(&str, u64) -> Result<Vec<crate::ward::ShiftOnChain>, String>,
    ) -> (Vec<crate::ward::ShiftOnChain>, Option<(String, u64)>, Option<String>) {
        let mut got = Vec::new();
        let mut cursor = None;
        for (read_so_far, (sig, slot)) in page.into_iter().rev().enumerate() {
            // Asked before the round trip, not after: the budget is there to stop the ward paying
            // for one more, and a check that runs afterwards has already paid for it.
            if let Some(why) = budget.spent(read_so_far, std::time::Instant::now()) {
                return (got, cursor, Some(why));
            }
            match read(&sig, slot) {
                Ok(shifts) => {
                    got.extend(shifts);
                    // Past entries that held no shift as well: taking the head is a transaction
                    // too, and a cursor made only of shifts re-fetches every lease for ever.
                    cursor = Some((sig, slot));
                }
                Err(e) => {
                    return (got, cursor, Some(format!("stopped at {sig}: {e}")));
                }
            }
        }
        (got, cursor, None)
    }

    /// The transaction that carried a given shift, if this cache read it.
    ///
    /// A leaf is a hash and a hash is not an address: the thing a stranger can open in an explorer
    /// and see for themselves is the transaction that anchored it. Kept here already, because a
    /// signature is how this cache tells one shift from another.
    pub fn signature_of(&self, run_hash: &[u8; 32]) -> Option<String> {
        self.shifts
            .iter()
            .find(|s| &s.shift.run_hash == run_hash)
            .map(|s| s.signature.clone())
    }

    pub fn shifts(&self) -> Vec<ShiftOnChain> {
        self.shifts.iter().map(|s| s.shift).collect()
    }

    /// Take a page of history, newest first, and keep what is new. Returns how many were new.
    ///
    /// Idempotent by signature, because every reason this is called twice is ordinary: a retry, a
    /// restart, two instances ticking at once, a page that overlaps the last one.
    pub fn absorb(
        &mut self,
        page: Vec<(crate::ward::ShiftOnChain, String)>,
        cursor: Option<(String, u64)>,
    ) -> usize {
        let mut added = 0;
        for (shift, signature) in page {
            if self.shifts.iter().any(|s| s.signature == signature && s.shift == shift) {
                continue;
            }
            self.shifts.push(SeenShift { signature, shift });
            added += 1;
        }
        self.shifts.sort_by_key(|s| s.shift.slot);
        // The cursor is the walk's, not this function's. Deriving it from the shifts would put it
        // past an entry the walk deliberately stopped before.
        if let Some((sig, slot)) = cursor {
            if slot >= self.until_slot || self.until.is_none() {
                self.until_slot = slot;
                self.until = Some(sig);
            }
        }
        added
    }
}

/// Walk a page of a patient's history, oldest first, stopping where it cannot read.
///
/// The free-standing name for [`Seen::walk`], which is what the reader calls and what the test
/// drives with a page carrying a null.
pub fn walk_history(
    page: Vec<(String, u64)>,
    budget: &Budget,
    read: impl FnMut(&str, u64) -> Result<Vec<crate::ward::ShiftOnChain>, String>,
) -> (Vec<crate::ward::ShiftOnChain>, Option<(String, u64)>, Option<String>) {
    Seen::walk(page, budget, read)
}

/// The chain, as the ward reads it.
pub struct WardChain {
    rpc: RpcClient,
    program_id: Pubkey,
    /// Patient PDAs are seeded on the operator, so the ward can only find its own patients.
    operator: Pubkey,
    /// Pays. Never plays. `None` on a read-only deployment, where the census still answers and
    /// nothing can be signed — which is a perfectly good ward host to put in front of a judge.
    relay: Option<Keypair>,
    source: String,
}

impl WardChain {
    /// `Err` with a sentence a stranger can act on, never a silent `None`.
    ///
    /// Every failure here ends up on `/api/ward` as `readable: false` with this text beside it,
    /// so "misconfigured" and "RPC is down" must not read the same.
    pub fn connect() -> Result<WardChain, String> {
        let url = std::env::var("VITALS_RPC")
            .map_err(|_| "VITALS_RPC is not set: the ward has no chain to read".to_string())?;
        let id = std::env::var("VITALS_PROGRAM_ID")
            .map_err(|_| "VITALS_PROGRAM_ID is not set: the ward has no program to read".to_string())?;
        if id == ETERNAL_PROGRAM_ID {
            return Err("this is Eternal's program id, not the ward's — the sprint never reads or \
                        upgrades Eternal (CWF_PLAN.md ruling 7)".into());
        }
        let program_id = Pubkey::from_str(&id).map_err(|_| format!("VITALS_PROGRAM_ID is not a pubkey: {id}"))?;
        // The relay is also the operator: it admits the patients, so their addresses are seeded
        // on its key. `VITALS_OPERATOR` overrides that for a read-only host — one that publishes
        // somebody else's ward without holding a key at all — and is otherwise never needed.
        let relay = std::env::var("VITALS_KEYPAIR")
            .ok()
            .and_then(|p| read_keypair_file(p).ok());
        let operator = match std::env::var("VITALS_OPERATOR") {
            Ok(o) => Pubkey::from_str(&o).map_err(|_| format!("VITALS_OPERATOR is not a pubkey: {o}"))?,
            Err(_) => relay
                .as_ref()
                .map(|k| k.pubkey())
                .ok_or("no relay keypair and no VITALS_OPERATOR: patient accounts are seeded on \
                        the operator, so without one of the two the ward cannot address its own \
                        patients")?,
        };
        let cluster = cluster_of(&url);
        Ok(WardChain {
            rpc: RpcClient::new_with_commitment(url, CommitmentConfig::confirmed()),
            program_id,
            operator,
            relay,
            source: format!("{cluster}:{program_id}"),
        })
    }

    /// The program these patients live on.
    pub fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    /// Which chain and which program — the half of every figure that says what it is a figure *of*.
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn slot(&self) -> Result<u64, String> {
        self.rpc.get_slot().map_err(why)
    }

    /// When the chain says a slot's block was produced, in unix seconds.
    ///
    /// The one honest bridge between a chain's clock and a person's. `Err` for a slot that was
    /// skipped or has been pruned out of the ledger — which is a slot the board shows no time for
    /// rather than one it dates by arithmetic.
    pub fn block_time(&self, slot: u64) -> Result<i64, String> {
        self.rpc.get_block_time(slot).map_err(why)
    }

    pub fn patient_pda(&self, patient_id: u64) -> Pubkey {
        patient_pda(&self.program_id, &self.operator, patient_id).0
    }

    /// Every patient this ward ever admitted, from the accounts themselves.
    ///
    /// Filtered by size rather than by a discriminator because this program has none: accounts are
    /// plain borsh, so length is what distinguishes a patient from a tree or a claim. Anything of
    /// the right length that does not decode, or that belongs to another operator, is dropped
    /// rather than guessed at.
    pub fn patients(&self) -> Result<Vec<PatientOnChain>, String> {
        let config = RpcProgramAccountsConfig {
            filters: Some(vec![RpcFilterType::DataSize(PATIENT_LEN as u64)]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..RpcAccountInfoConfig::default()
            },
            ..RpcProgramAccountsConfig::default()
        };
        let accounts = self
            .rpc
            .get_program_accounts_with_config(&self.program_id, config)
            .map_err(why)?;

        let mut out: Vec<PatientOnChain> = accounts
            .iter()
            .filter(|(key, acct)| {
                decode_patient(&acct.data)
                    .is_some_and(|p| self.patient_pda(p.patient_id) == *key)
            })
            .filter_map(|(_, acct)| decode_patient(&acct.data))
            .collect();
        out.sort_by_key(|p| p.patient_id);
        Ok(out)
    }

    /// Read whatever has happened to one patient since the last look, into her cache.
    ///
    /// Returns how many shifts were new. A failed transaction is not a shift: the program refused
    /// it, so nothing was anchored, and counting it would publish work the chain says did not
    /// happen — which is precisely the refusal the ward is proud of.
    pub fn refresh(
        &self,
        patient_id: u64,
        seen: &mut Seen,
        store: &crate::store::Store,
        budget: &Budget,
    ) -> Result<Reading, String> {
        let pda = self.patient_pda(patient_id);
        let until = seen.until().and_then(|s| Signature::from_str(&s).ok());
        let sigs = self
            .rpc
            .get_signatures_for_address_with_config(
                &pda,
                GetConfirmedSignaturesForAddress2Config {
                    until,
                    commitment: Some(CommitmentConfig::confirmed()),
                    ..GetConfirmedSignaturesForAddress2Config::default()
                },
            )
            .map_err(why)?;

        // Every slot this answer names, dated by the answer that named it. The listing carries a
        // block time beside each signature, and these are exactly the slots a receipt has to put a
        // date on: her admission, every take, every anchor. Kept before the filter, because a
        // refused transaction still happened at a time, and the slot it happened in is one a later
        // read would otherwise pay a round trip for.
        learn_slot_times(
            store,
            &sigs.iter().map(|s| (s.slot, s.block_time)).collect::<Vec<_>>(),
        );

        // A failed transaction is not a shift: the program refused it, so nothing was anchored,
        // and counting it would publish work the chain says did not happen. Dropped here rather
        // than in the walk, because a refusal is a thing we read successfully.
        let page: Vec<(String, u64)> = sigs
            .into_iter()
            .filter(|s| s.err.is_none())
            .map(|s| (s.signature, s.slot))
            .collect();

        let mut signatures: std::collections::BTreeMap<u64, String> = Default::default();
        let (shifts, cursor, trouble) = Seen::walk(page, budget, |sig, slot| {
            let parsed = Signature::from_str(sig).map_err(|e| format!("{sig} is not a signature: {e}"))?;
            let tx = self
                .rpc
                .get_transaction(&parsed, UiTransactionEncoding::Base64)
                .map_err(why)?;
            // The same fact from the other side: a listing that came back without a time for this
            // slot may still be answered by the transaction itself.
            learn_slot_times(store, &[(tx.slot, tx.block_time)]);
            signatures.insert(slot, sig.to_string());
            let Some(decoded) = tx.transaction.transaction.decode() else {
                // Read, and not a transaction we can decode. Not a failure of the walk: it held no
                // shift, and the cursor may pass it.
                return Ok(Vec::new());
            };
            let keys = decoded.message.static_account_keys().to_vec();
            let mut here = Vec::new();
            for ix in decoded.message.instructions() {
                let Some(program) = keys.get(ix.program_id_index as usize) else { continue };
                let accounts: Vec<Pubkey> = ix
                    .accounts
                    .iter()
                    .filter_map(|i| keys.get(*i as usize).copied())
                    .collect();
                if accounts.len() != ix.accounts.len() {
                    // An index we cannot resolve — a lookup table we do not use. Skip rather than
                    // shift the account order and credit the wrong key.
                    continue;
                }
                if let Some(shift) = shift_in(program, &self.program_id, &ix.data, &accounts, tx.slot) {
                    here.push(shift);
                }
            }
            Ok(here)
        });

        let named: Vec<(crate::ward::ShiftOnChain, String)> = shifts
            .into_iter()
            .map(|sh| {
                let sig = signatures.get(&sh.slot).cloned().unwrap_or_default();
                (sh, sig)
            })
            .collect();
        let added = seen.absorb(named, cursor);

        // A page that stopped early is not an error: what was read is kept, the cursor stops at
        // it, and the next read begins there. The ward stays readable and loses nothing.
        if let Some(why) = &trouble {
            eprintln!("ward       patient {patient_id}'s history was read as far as it could be — {why}");
        }
        // Kept either way; `stopped` is what says whether anything may be decided on it.
        Ok(Reading { added, stopped: trouble })
    }
}

use solana_account_decoder_client_types::UiAccountEncoding;

/// The cluster's own name, for the `source` line. Read off the URL, which is the only place it is
/// written down — a hardcoded "devnet" that outlived a config change would be a lie in the one
/// field whose job is to say where a number came from.
fn cluster_of(url: &str) -> &'static str {
    if url.contains("devnet") {
        "devnet"
    } else if url.contains("testnet") {
        "testnet"
    } else if url.contains("mainnet") {
        "mainnet-beta"
    } else if url.contains("127.0.0.1") || url.contains("localhost") {
        "localhost"
    } else {
        "unknown-cluster"
    }
}

/// An RPC failure as a sentence, not a debug dump.
fn why(e: RpcError) -> String {
    e.to_string()
}

/// Where each patient's read-so-far lives in the store.
///
/// Durable by [`crate::store::class_of`]'s default, and that is right: losing it costs a full
/// walk of every patient's history rather than a learner's run, and a cache that the sweep can
/// delete at three in the morning is a cache that makes the census slow at three in the morning.
pub const SHIFT_CACHE: &str = "ward_shifts";

/// Where a slot's block time is kept, by slot. Written once and never again: a block's time is
/// decided when the block is produced and never changes, so there is nothing here to expire.
pub const SLOT_TIMES: &str = "ward_slot_time";

/// How many slots one read will ask the chain to date. The cache makes this a cost paid once per
/// slot for the life of the ward, and the cap is what keeps that first read — or the first after a
/// store is moved — from being one request behind fifty round trips. Whatever is left over is
/// dated by a later read, and until then those rows carry the slot and no time.
const DATE_AT_MOST: usize = 24;

/// Keep every block time the chain has already handed us, and ask for none.
///
/// `getSignaturesForAddress` answers with a block time beside every signature and `getTransaction`
/// carries one too, so every slot a patient's history touches is dated by the call that found it —
/// her admission, every take, every anchor. Filing them at that moment is the difference between a
/// receipt that renders from the store and one that asks the RPC for each slot in turn, at 0.11 to
/// 1.03 s a call (measured against public devnet, 18 ก.ย.), in front of a reader looking at nothing.
///
/// Returns how many were new. Never overwrites: a block's time is decided when the block is
/// produced, so a second answer for the same slot is either the same one or a wrong one. Slot 0 is
/// not a slot — it is what a patient carries when the ward never learned when she was admitted, and
/// dating it would put her admission at the epoch.
pub fn learn_slot_times(store: &crate::store::Store, page: &[(u64, Option<i64>)]) -> usize {
    let mut kept = 0;
    for (slot, time) in page {
        let (Some(t), true) = (time, *slot != 0) else { continue };
        if store.get::<i64>(SLOT_TIMES, &slot.to_string()).is_some() {
            continue;
        }
        if store.put(SLOT_TIMES, &slot.to_string(), t).is_ok() {
            kept += 1;
        }
    }
    kept
}

/// The chain's own time for each of these slots, cached for ever.
///
/// Read from the store first, because a block's time never changes and the ward would otherwise
/// ask the RPC the same question about the same slot for the rest of its life. At most
/// `DATE_AT_MOST` new slots are asked for in one pass: the rest keep their place in the cache
/// queue and are dated by a later read, and until then the rows about them carry the slot and no
/// time. A slot the chain will not date — skipped, or pruned out of the ledger — is not cached
/// and not guessed at.
pub fn slot_times(
    chain: &WardChain,
    store: &crate::store::Store,
    want: &std::collections::BTreeSet<u64>,
) -> std::collections::BTreeMap<u64, i64> {
    let mut out = std::collections::BTreeMap::new();
    let mut asked = 0usize;
    for slot in want {
        if let Some(t) = store.get::<i64>(SLOT_TIMES, &slot.to_string()) {
            out.insert(*slot, t);
            continue;
        }
        if asked >= DATE_AT_MOST {
            continue;
        }
        asked += 1;
        if let Ok(t) = chain.block_time(*slot) {
            let _ = store.put(SLOT_TIMES, &slot.to_string(), &t);
            out.insert(*slot, t);
        }
    }
    out
}

/// Where the last board this ward read is kept, so the next process to start has one to answer
/// with. One row, rewritten each time the chain is read.
pub const BOARD_STORE: &str = "ward_board";

/// The shape of the record in [`BOARD_STORE`].
///
/// A board is a payload built by `ward::ward_payload`, and a build that has changed that shape must
/// not serve one written by a build that had not. Bumped when the payload changes in a way a page
/// would notice; a mismatch is refused rather than parsed hopefully.
pub const BOARD_VERSION: u32 = 1;

/// Keep this board for whoever starts next. `true` when it was kept.
///
/// **An outage is not a board.** `read_ward` answers `ward_unavailable` when the chain cannot be
/// read, and writing that over the last good board would turn one bad minute into a ward that is
/// empty until the next successful read — which on a cold instance is the thing being fixed. So
/// only a readable board is kept, and the revision that wrote it is kept beside it.
pub fn keep_board(store: &crate::store::Store, board: &serde_json::Value, revision: &str) -> bool {
    if board.get("readable").and_then(serde_json::Value::as_bool) != Some(true) {
        return false;
    }
    let row = serde_json::json!({
        "version": BOARD_VERSION,
        "at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "revision": revision,
        "board": board,
    });
    match store.put(BOARD_STORE, "last", &row) {
        Ok(()) => true,
        Err(e) => {
            // Said out loud rather than swallowed by an `is_ok()`. A board that cannot be kept means
            // the next process to start pays for a chain read in front of whoever knocks first, and
            // a silent cache miss is exactly the kind of thing that costs an afternoon to find.
            eprintln!(
                "ward       the board could not be kept ({} bytes to {BOARD_STORE}/last): {e}",
                serde_json::to_string(&row).map(|s| s.len()).unwrap_or(0)
            );
            false
        }
    }
}

/// The last board this ward read, and how long ago it read it.
///
/// `None` when there is none, when its shape is not this build's, or when it is not readable — all
/// three mean the same thing to a caller: there is nothing here to answer with.
pub struct Kept {
    /// How long ago the board was read from the chain.
    pub age: std::time::Duration,
    /// The second it was read, as the chain's own clock would have it. Published rather than the
    /// age, because an age ticks — and a board whose bytes change every second has a new ETag every
    /// second, which is every polling page downloading the whole board for ever.
    pub at_unix: u64,
    /// The revision that read it. Named in the answer, so a slow first request can be attributed to
    /// a deploy rather than guessed at.
    pub revision: String,
    pub board: serde_json::Value,
}

pub fn last_board(store: &crate::store::Store) -> Option<Kept> {
    let row: serde_json::Value = store.get(BOARD_STORE, "last")?;
    if row.get("version").and_then(serde_json::Value::as_u64) != Some(BOARD_VERSION as u64) {
        return None;
    }
    let board = row.get("board")?.clone();
    if board.get("readable").and_then(serde_json::Value::as_bool) != Some(true) {
        return None;
    }
    let at = row.get("at_unix").and_then(serde_json::Value::as_u64)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(Kept {
        age: std::time::Duration::from_secs(now.saturating_sub(at)),
        at_unix: at,
        revision: row
            .get("revision")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        board,
    })
}

/// A week, in slots. `604800 / 0.4`.
///
/// The window the endpoint calls *this week*. In slots rather than in hours because every other
/// figure on the endpoint is in slots, and a week measured two ways is a week that can disagree
/// with itself.
pub const WEEK_SLOTS: u64 = 1_512_000;

/// Every patient's history for the board: from the cache, refreshed only where the chain says it
/// moved, and never a reason to have no board.
pub struct Histories {
    pub shifts: Vec<crate::ward::ShiftOnChain>,
    /// Patients whose chain names a shift this ward has no tape for, and the leaf it stopped at.
    pub lost: std::collections::BTreeMap<u64, String>,
    /// Patients whose history could not be refreshed this read, and why. Their account was read —
    /// that is how they are on the list at all — so their row is right and their chart may be a
    /// shift behind. Said on the row; never a reason to say the ward is unreadable.
    pub unread: std::collections::BTreeMap<u64, String>,
    /// How many cost a signature listing. On a quiet ward this is zero.
    pub listed: usize,
}

/// The per-patient loop of [`read_ward`], behind an injected `refresh` so it can be driven without a
/// validator.
///
/// **A failed listing is one patient's stale chart, not a blacked-out ward.** Staging answered
/// `readable: false` for over an hour on 20 ก.ย. because this loop did `return unavailable(...)` on
/// one 429 — throwing away twenty-six accounts that had just been read correctly in a single
/// `getProgramAccounts`. Now the failure is recorded against the patient and the loop goes on.
///
/// And it asks before it pays: [`needs_listing`] on the chain's own shift count and the tapes on
/// disk, the same as the ticker's pass. A patient nothing has happened to is never listed, so on a
/// quiet ward the board rebuild makes no listings at all and there is nothing to 429.
pub fn histories(
    store: &crate::store::Store,
    patients: &[crate::ward::PatientOnChain],
    mut refresh: impl FnMut(u64, &mut Seen) -> Result<Reading, String>,
) -> Histories {
    let mut h = Histories {
        shifts: Vec::new(),
        lost: Default::default(),
        unread: Default::default(),
        listed: 0,
    };
    for p in patients {
        let key = format!("p{}", p.patient_id);
        let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
        let known = seen.shifts();
        if needs_listing(p.shifts, known.len(), missing_tapes(store, &known).is_empty()) {
            h.listed += 1;
            match refresh(p.patient_id, &mut seen) {
                Ok(read) => {
                    if read.added > 0 {
                        let _ = store.put(SHIFT_CACHE, &key, &seen);
                    }
                    // Read only as far as it got: kept, and said. The shifts past a stop are the
                    // recent ones, so this chart may be behind exactly like a failed one.
                    if let Some(why) = read.stopped {
                        h.unread.insert(p.patient_id, why);
                    }
                }
                Err(e) => {
                    h.unread.insert(p.patient_id, e);
                }
            }
        }
        // Who this ward cannot rebuild, found while it is already holding every patient's shifts:
        // a leaf on chain whose tape is not here. The board says so in the row; nothing is written
        // to the chain about it, because "we lost the tape" is a fact about this ward.
        for s in seen.shifts() {
            let hash = hex32(&s.run_hash);
            if is_shift_hash(&hash) && tape_by_hash(store, &hash).is_none() {
                h.lost.entry(p.patient_id).or_insert(hash);
            }
        }
        h.shifts.extend(seen.shifts());
    }
    h
}

/// The board to serve when there is no fresh one: the last one this host kept, saying so.
///
/// `getProgramAccounts` failing means there is no fresh account for anybody — the other half of
/// the same incident. But [`keep_board`] has kept every readable board this host ever built, so
/// there is almost always one a minute or two old that is far more true than "nothing". It is
/// served `from: store` with its own `kept_at` — the time it was read, never the time it was served,
/// because an age ticks and a ticking field is a new ETag every second — and the reason a fresh one
/// could not be had. The queue is this host's own fact and is current whatever the chain did.
///
/// With nothing kept, unreadable and honest: a board is not invented.
pub fn fall_back(
    kept: Option<Kept>,
    source: &str,
    queue: serde_json::Value,
    why: &str,
) -> serde_json::Value {
    match kept {
        Some(k) => {
            let mut board = k.board;
            board["board"] = crate::ward::board_note(
                crate::ward::Origin::Store,
                k.at_unix,
                Some(k.revision.as_str()).filter(|r| !r.is_empty()),
            );
            board["stale"] = serde_json::json!(format!(
                "served from the last board this host kept, because a fresh one could not be read — {why}"
            ));
            board["queue"] = queue;
            board
        }
        None => {
            let mut v = crate::ward::ward_unavailable(source, why);
            v["queue"] = queue;
            v
        }
    }
}

/// One pass over the whole ward: the patients, whatever is new in their histories, and the
/// payload built from both.
///
/// **A partial read is not published.** If any patient's history cannot be read, this answers
/// [`crate::ward::ward_unavailable`] with the reason rather than a census missing her shifts. The
/// failure this refuses is the quiet one: a number that is merely too small looks exactly like a
/// ward nobody came to, and it would be photographed onto the weekly card and read out loud.
///
/// Whatever *was* read still lands in the store, so the next pass does not re-walk it.
pub fn read_ward(chain: &WardChain, store: &crate::store::Store) -> serde_json::Value {
    // The door and the queue are facts about this host and are known whatever the chain says. A
    // ward that cannot read its chain still has a door and still has people waiting behind it, and
    // a page that branches on the door was getting nothing to branch on.
    let queue = queue_block(store);
    // No fresh board at all: serve the last one this host kept, saying so. Never `unavailable`
    // while a readable board a minute old is sitting in the store.
    let as_of = match chain.slot() {
        Ok(s) => s,
        Err(e) => return fall_back(last_board(store), chain.source(), queue, &e),
    };
    let patients = match chain.patients() {
        Ok(p) => p,
        Err(e) => return fall_back(last_board(store), chain.source(), queue, &e),
    };

    let h = histories(store, &patients, |id, seen| {
        chain.refresh(id, seen, store, &Budget::whole_history())
    });
    let (shifts, lost) = (h.shifts, h.lost);
    let times = slot_times(chain, store, &crate::ward::slots_to_date(&patients, &shifts));
    let rate = seconds_per_slot(chain, store, as_of);
    let mut v = crate::ward::ward_payload(&crate::ward::WardRead {
        seconds_per_slot: rate,
        patients: &patients,
        shifts: &shifts,
        packs: &packs(store),
        cases: &crate::ward_case::all(store),
        unrebuildable: &lost,
        unread: &h.unread,
        since: Some(as_of.saturating_sub(WEEK_SLOTS)),
        as_of_slot: as_of,
        now_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source: chain.source(),
        times: &times,
    });
    // How many patients are waiting, which is the number the factory tops up against and the one
    // a reader of the board uses to tell "nobody is playing" from "nobody is left to play".
    v["queue"] = queue;
    // This process read it, so that is what it says — stamped here, where the board is made, so a
    // board that came from the store instead (see `fall_back`) keeps its own provenance and is
    // never relabelled as fresh by whoever happens to serve it.
    v["board"] = crate::ward::board_note(
        crate::ward::Origin::Chain,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        None,
    );
    // Kept for whoever starts next. Every path that reads the chain successfully comes through
    // here — the ticker's, the refresh behind an answer, and the one request on a ward that has
    // never read at all — so this is the only place that has to remember to do it.
    keep_board(store, &v, &std::env::var("K_REVISION").unwrap_or_default());
    v
}

/// The queue, as the board publishes it: how many are waiting, who they are, and which door they
/// are behind.
///
/// Its own function because every answer `/api/ward` gives carries it — including the ones where
/// the chain could not be read at all, which is exactly when a page most needs to know whether the
/// ward is shut, filling, or open.
pub fn queue_block(store: &crate::store::Store) -> serde_json::Value {
    let door = door_here();
    // A closed ward publishes no queue at all — it is not open, nobody is coming, and a list of
    // people nobody can meet is a promise. Preview and open both publish it: the rows draw the
    // same way, and "waiting" is not a secret on a public ward.
    let queued: Vec<(String, crate::ward::Pack)> = if door == Door::Closed {
        Vec::new()
    } else {
        store.list::<crate::ward::Pack>(QUEUE_STORE)
    };
    let waiting = queue_depth(store);
    let mut block = serde_json::json!({});
    for (key, val) in [
        // Null, never zero, when the store cannot be listed — "nobody is waiting" and "we could
        // not look" are opposite facts and this endpoint has one rule about those.
        ("waiting", serde_json::json!(waiting.as_ref().ok())),
        ("waiting_unknown_because", serde_json::json!(waiting.as_ref().err())),
        ("beds", serde_json::json!(crate::ward::BEDS)),
        // Published, because "the queue is empty" and "the door is shut" look identical from
        // outside and mean opposite things about whether anybody should be doing anything.
        ("door", serde_json::json!(door_here().word())),
        ("filled_by", serde_json::json!("a ticker on the ward host, every minute: a bed frees on \
                                         discharge or death and the next queued patient takes it. \
                                         Nobody on the team touches anything")),
        ("waiting_patients", serde_json::json!(crate::ward::waiting_rows(&queued, &crate::ward_case::all(store)))),
    ] {
        block[key] = val;
    }
    block
}

// ── the shift flow ──────────────────────────────────────────────────────────

/// The player's account PDA — who they are, seeded on the key in their browser.
pub fn account_pda(program_id: &Pubkey, player: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[SEED_ACCOUNT, &player.to_bytes()], program_id).0
}

/// Take the head of a patient's chain for the length of a shift.
///
/// Three accounts and no more: the player, who they are, and the patient the lease is written on.
/// **The relay is not among them.** It pays for the transaction — that is what lets a stranger
/// play without ever buying SOL — and it takes no part in the instruction, because a relay that
/// could take a shift could take one in somebody else's name, and the ward's whole claim is that
/// the record says which key did what. The operator is an argument all the same, because the
/// patient's address is seeded on it — the ward can only reach its own patients.
pub fn take_shift_ix(
    program_id: &Pubkey,
    operator: &Pubkey,
    player: &Pubkey,
    patient_id: u64,
) -> SolInstruction {
    SolInstruction::new_with_borsh(
        *program_id,
        &Instruction::TakeShift { patient_id },
        vec![
            AccountMeta::new_readonly(*player, true),
            AccountMeta::new_readonly(account_pda(program_id, player), false),
            AccountMeta::new(patient_pda(program_id, operator, patient_id).0, false),
        ],
    )
}

/// Take a head back on the ward's own authority.
///
/// Two accounts and no player among them: the operator signs, and the patient is what it signs
/// about. The program reads the operator off the patient herself, so a ward can only free a head on
/// a patient it admitted.
///
/// This is what the heartbeat is for. A stranger who closes the tab used to hold a bed until the
/// lease ran out — nine and a half minutes at the rate devnet is running, on a ward with three
/// beds — and a release they signed in advance could not be held for them: a blockhash here is
/// worth about twenty-six seconds, and the silence worth acting on is longer than that.
pub fn free_shift_ix(program_id: &Pubkey, operator: &Pubkey, patient_id: u64) -> SolInstruction {
    SolInstruction::new_with_borsh(
        *program_id,
        &Instruction::FreeShift { patient_id },
        vec![
            AccountMeta::new_readonly(*operator, true),
            AccountMeta::new(patient_pda(program_id, operator, patient_id).0, false),
        ],
    )
}

/// Put the head down without anchoring anything.
///
/// The same three accounts taking it names, and the same absence: the relay pays for the
/// transaction and is not in it. While a lease stands only its holder may release it — the program
/// checks that — and once it has expired anybody may clear it, which is what stops a stranger who
/// closed their laptop from holding a bed until somebody with the right key comes back, who is
/// nobody.
///
/// **Nothing is recorded by this.** The shift's tape is discarded and her chart is untouched: the
/// next person gets her exactly as this one found her. That is the honest meaning of walking away,
/// and it is why the page says it in those words.
pub fn release_shift_ix(
    program_id: &Pubkey,
    operator: &Pubkey,
    player: &Pubkey,
    patient_id: u64,
) -> SolInstruction {
    SolInstruction::new_with_borsh(
        *program_id,
        &Instruction::ReleaseShift { patient_id },
        vec![
            AccountMeta::new_readonly(*player, true),
            AccountMeta::new_readonly(account_pda(program_id, player), false),
            AccountMeta::new(patient_pda(program_id, operator, patient_id).0, false),
        ],
    )
}

/// Anchor the shift that was just played onto the head it extends.
///
/// The one instruction both keys appear in, and each has exactly one job. The relay signs at
/// index 0 because rent for the leaf comes out of it; the player signs at index 1 and is not
/// writable, because signing is not spending and nothing here debits a stranger. `prev_head` is
/// the claim the program checks: name a head the patient has moved past and the transaction is
/// refused with `StaleHead` rather than quietly overwriting somebody's work.
#[allow(clippy::too_many_arguments)]
pub fn anchor_shift_ix(
    program_id: &Pubkey,
    operator: &Pubkey,
    player: &Pubkey,
    patient_id: u64,
    tree_id: u64,
    record: RecordWire,
    prev_head: [u8; 32],
) -> SolInstruction {
    SolInstruction::new_with_borsh(
        *program_id,
        &Instruction::AnchorShift { tree_id, patient_id, record, prev_head },
        vec![
            AccountMeta::new(*operator, true),
            AccountMeta::new_readonly(*player, true),
            AccountMeta::new(account_pda(program_id, player), false),
            AccountMeta::new(tree_pda(program_id, operator, tree_id).0, false),
            AccountMeta::new(commitment_pda(program_id, &player.to_bytes()).0, false),
            AccountMeta::new(patient_pda(program_id, operator, patient_id).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    )
}

/// A transaction the relay has signed and the player has not.
///
/// The bytes in [`Pending::message`] are exactly what the player's key must sign — the same bytes
/// the relay signed. The server cannot produce that signature, and that is the entire point: a
/// ward whose operator could complete a shift would have a record of what its operator asserted,
/// not of what strangers did.
///
/// (`chain.rs` holds the same shape for the Eternal flow. It lives inside the binary and speaks to
/// a different program with a different instruction set; this one is in the library because the
/// ward's wiring is tested from outside, and the two are kept apart deliberately — merging them
/// would put Eternal's anchoring one refactor away from a sprint change, which ruling 7 forbids.)
pub struct Pending {
    tx: Transaction,
    slot: usize,
}

impl Pending {
    /// The bytes the player's key must sign.
    pub fn message(&self) -> Vec<u8> {
        self.tx.message_data()
    }

    /// Drop the player's signature into its slot and hand back a transaction that will verify.
    ///
    /// The verification is not a formality: it is what turns "somebody sent us 64 bytes" into "the
    /// key that plays this shift signed for it", and a signature from any other key is refused
    /// here rather than by the cluster a second later.
    pub fn signed(self, sig: &[u8; 64]) -> Result<Transaction, String> {
        let mut tx = self.tx;
        tx.signatures[self.slot] = Signature::from(*sig);
        tx.verify()
            .map_err(|_| "that signature does not match this transaction".to_string())?;
        Ok(tx)
    }
}

/// Build the half-signed transaction: relay as fee payer, one empty slot for the player.
///
/// A free function taking the blockhash rather than a method reaching for one, so the part that
/// decides who signs what can be tested without a cluster. [`WardChain::prepare`] is the same
/// thing with a fresh blockhash from the RPC.
pub fn prepare_for(
    relay: &Keypair,
    ix: SolInstruction,
    player: &Pubkey,
    blockhash: Hash,
) -> Result<Pending, String> {
    let msg = Message::new(&[ix], Some(&relay.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    let slot = tx
        .message
        .account_keys
        .iter()
        .position(|k| k == player)
        .ok_or("this instruction does not name the player, so there is nothing for them to sign")?;
    tx.try_partial_sign(&[relay], blockhash)
        .map_err(|e| format!("the relay could not sign: {e}"))?;
    Ok(Pending { tx, slot })
}

impl WardChain {
    /// The commitment this player declared, as the chain recorded it.
    ///
    /// Read back rather than assumed: the slot was assigned on chain and the record anchored later
    /// must carry the same one, or the leaf the server builds is not the leaf the program checks.
    pub fn commitment(&self, player: &Pubkey) -> Option<vitals_program::Commitment> {
        let pda = commitment_pda(&self.program_id, &player.to_bytes()).0;
        let data = self.rpc.get_account_data(&pda).ok()?;
        borsh::BorshDeserialize::deserialize(&mut &data[..]).ok()
    }

    /// Does this key already have an account here? A stranger's first shift begins with one.
    pub fn has_account(&self, player: &Pubkey) -> bool {
        self.rpc.get_account_data(&account_pda(&self.program_id, player)).is_ok()
    }

    /// The key that pays, if this host holds one. The ward board shows it so a stranger can check
    /// the balance that is funding their shift rather than take our word that one exists.
    pub fn relay_pubkey(&self) -> Option<String> {
        self.relay.as_ref().map(|k| k.pubkey().to_string())
    }

    /// The operator these patients are seeded on.
    pub fn operator(&self) -> Pubkey {
        self.operator
    }

    /// One patient's account as it stands: her head, her lease, whether she is still open.
    ///
    /// `Ok(None)` is a patient who was never admitted, which is a different thing from a chain
    /// that could not be read — and the two must not collapse into one answer, because the first
    /// is a bad patient id and the second is an outage.
    pub fn patient(&self, patient_id: u64) -> Result<Option<PatientAccount>, String> {
        match self.rpc.get_account_data(&self.patient_pda(patient_id)) {
            Ok(data) => PatientAccount::deserialize(&mut &data[..])
                .map(Some)
                .map_err(|e| format!("patient {patient_id} does not decode: {e}")),
            // Not found is the ordinary answer for a patient who does not exist yet, and the RPC
            // reports it as an error like any other. Everything else is an outage.
            Err(e) if e.to_string().contains("AccountNotFound") => Ok(None),
            Err(e) => Err(why(e)),
        }
    }

    /// Half-sign an instruction for a player to finish in their browser.
    pub fn prepare(&self, ix: SolInstruction, player: &Pubkey) -> Result<Pending, String> {
        let relay = self
            .relay
            .as_ref()
            .ok_or("this host holds no relay key, so it can read the ward but not pay for a shift")?;
        let blockhash = self.rpc.get_latest_blockhash().map_err(why)?;
        prepare_for(relay, ix, player, blockhash)
    }

    /// Take the head of a patient's chain — prepared here, signed in the browser.
    pub fn take_shift(&self, player: &Pubkey, patient_id: u64) -> Result<Pending, String> {
        self.prepare(take_shift_ix(&self.program_id, &self.operator, player, patient_id), player)
    }

    /// Take a head back, because the page holding it has stopped beating.
    ///
    /// Signed and sent by the ward itself — the relay is the operator here, which is true of
    /// nothing a player does. `Err` with the program's own words if this host is not the operator
    /// the patient was admitted by, which is a read-only deployment publishing somebody else's
    /// ward and must not be able to move anything on it.
    pub fn free_shift(&self, patient_id: u64) -> Result<String, String> {
        self.now(free_shift_ix(&self.program_id, &self.operator, patient_id))
    }

    /// Anchor the shift that was played, onto the head it claims to extend.
    pub fn anchor_shift(
        &self,
        player: &Pubkey,
        patient_id: u64,
        tree_id: u64,
        record: RecordWire,
        prev_head: [u8; 32],
    ) -> Result<Pending, String> {
        self.prepare(
            anchor_shift_ix(&self.program_id, &self.operator, player, patient_id, tree_id, record, prev_head),
            player,
        )
    }

    /// Send a transaction the player has finished signing, and wait for it to land.
    ///
    /// Confirmed rather than fire-and-forget, because the next thing that happens is a browser
    /// re-deriving her chart from the chain: telling a stranger their shift landed and then
    /// showing them a chart without it is worse than telling them it failed.
    pub fn submit(&self, tx: &Transaction) -> Result<String, String> {
        self.rpc
            .send_and_confirm_transaction(tx)
            .map(|s| s.to_string())
            .map_err(why)
    }

    /// Close a patient the ward finished while nobody was in the room.
    ///
    /// **The ticker does this, never a stranger** (founder, 16 ก.ย.). The alternative was to let
    /// the next person who opens her page discover the death and anchor it, and that is exactly
    /// what must not happen: the board would go on offering a corpse, and the record would put her
    /// death under the key of somebody who arrived after it.
    ///
    /// So the ward takes her head itself and anchors a shift with an **empty tape** and the idle
    /// span. Everything about that record says what happened — the operator's key signed it, no
    /// steps were played, and the outcome is the one the engine reached on its own. A reader who
    /// replays it gets the same death from the same nothing.
    ///
    /// Four instructions, the same four a stranger signs, because the program has one way in and a
    /// shortcut for the operator would be a second door into her chart. The host's own key is both
    /// funder and device here, which is the one thing that differs and the reason this is the only
    /// place it happens.
    pub fn close_unattended(
        &self,
        store: &crate::store::Store,
        patient_id: u64,
        sce_json: &str,
        difficulty: vitals_progress::Difficulty,
        replay: &vitals_replay::Replay,
        prev_head: [u8; 32],
    ) -> Result<String, String> {
        use solana_sdk::signature::Signer;
        let relay = self
            .relay
            .as_ref()
            .ok_or("this host holds no relay key, so it cannot close a patient")?;
        let me = relay.pubkey();

        // A key that has closed a patient here before already has an account; the program says so
        // rather than making a second one, and "already" is not a failure.
        if !self.has_account(&me) {
            if let Err(e) = self.now(open_account_ix(&self.program_id, &self.operator, &me)) {
                if !e.to_lowercase().contains("already") {
                    return Err(format!("the ward has no account of its own to close this patient with: {e}"));
                }
            }
        }
        self.now(take_shift_ix(&self.program_id, &self.operator, &me, patient_id))
            .map_err(|e| format!("the ward could not take the head to close this patient: {e}"))?;

        // Declared before it is anchored, like every other shift. There is nothing to hide in a
        // shift nobody played, and the point is that the program's one path is the path.
        let nonce = solana_sdk::signature::Keypair::new().pubkey().to_bytes();
        let sce = vitals_replay::sce_hash(sce_json);
        let hash = vitals_progress::record::commitment_hash(&sce, &me.to_bytes(), &nonce, 0);
        self.now(commit_ix(&self.program_id, &self.operator, &me, hash))
            .map_err(|e| format!("the ward could not declare the closing shift: {e}"))?;
        let slot = self
            .commitment(&me)
            .map(|c| c.slot)
            .ok_or("the declaration did not land, so there is nothing to anchor against")?;

        let rec = vitals_replay::record_for(
            me.to_bytes(), sce, sce, difficulty, false, &[], replay, hash, slot,
        )?;
        // The tape first, as everywhere else that writes to this chain: no steps, because nobody
        // did anything to her, filed under the record's own hash. Without it the death is on chain
        // and cannot be shown — which is what happened to the first two the ticker closed.
        keep_for_anchor(store, patient_id, &rec, &[])?;
        self.now(anchor_shift_ix(
            &self.program_id, &self.operator, &me, patient_id, WARD_TREE, wire(&rec), prev_head,
        ))
        .map_err(|e| format!("the closing shift would not anchor: {e}"))
    }

    /// One instruction, signed here and sent now. The host's key is funder and device both, which
    /// is true of nothing else on this ward.
    fn now(&self, ix: SolInstruction) -> Result<String, String> {
        use solana_sdk::signature::Signer;
        let relay = self.relay.as_ref().ok_or("this host holds no relay key")?;
        let blockhash = self.rpc.get_latest_blockhash().map_err(why)?;
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&relay.pubkey()), &[relay], blockhash);
        self.submit(&tx)
    }

    /// Release a patient onto the ward. The operator's own instruction: no player, no lease.
    ///
    /// The whole signature is ours, so this is the one place the ward acts rather than pays — and
    /// it is deliberately the only one. Admitting is a thing an operator does; treating is not.
    pub fn admit(&self, patient_id: u64, scenario_hash: [u8; 32]) -> Result<String, String> {
        let relay = self
            .relay
            .as_ref()
            .ok_or("this host holds no relay key, so it cannot admit a patient")?;
        let ix = SolInstruction::new_with_borsh(
            self.program_id,
            &Instruction::AdmitPatient { patient_id, scenario_hash },
            vec![
                AccountMeta::new(self.operator, true),
                AccountMeta::new(self.patient_pda(patient_id), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
        );
        let blockhash = self.rpc.get_latest_blockhash().map_err(why)?;
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&relay.pubkey()), &[relay], blockhash);
        self.submit(&tx)
    }
}

/// Where a patient's pack lives once the factory has queued her.
/// How long a slot is taking on this chain, in seconds, measured between two block times.
///
/// Two lookups a few thousand slots apart, both cached for ever after the first read, so this
/// costs one extra RPC call a deploy and nothing after that. `None` when either block cannot be
/// dated — and then nothing on the ward says a number of minutes, because the only other way to
/// get one is to multiply by a rate the chain is not keeping to.
///
/// Five thousand slots is the window: long enough that a few skipped slots do not move it, short
/// enough that it is this chain today rather than this chain last week. Devnet answered 0.166 s
/// over 1,000, 5,000, 20,000 and 60,000 slots on 17 ก.ย. — the window is not delicate.
pub const RATE_WINDOW_SLOTS: u64 = 5_000;

pub fn seconds_per_slot(chain: &WardChain, store: &crate::store::Store, now_slot: u64) -> Option<f64> {
    let then = now_slot.checked_sub(RATE_WINDOW_SLOTS)?;
    let times = slot_times(chain, store, &[then, now_slot].into_iter().collect());
    let (a, b) = (times.get(&then)?, times.get(&now_slot)?);
    let span = (b - a) as f64;
    (span > 0.0).then(|| span / RATE_WINDOW_SLOTS as f64)
}

/// The chain's time for one slot, asked and kept. For the paths that hold a chain.
pub fn dater<'a>(
    chain: &'a WardChain,
    store: &'a crate::store::Store,
) -> impl Fn(u64) -> Option<i64> + 'a {
    move |slot| {
        if slot == 0 {
            return None;
        }
        slot_times(chain, store, &[slot].into_iter().collect()).get(&slot).copied()
    }
}

/// The chain's time for one slot if this ward has already asked for it. For the paths that have a
/// store and no chain — a session restored at boot, a shift being reduced while the RPC is down.
///
/// `None` is "not known here", and a span with an unknown end does not advance the patient at all.
/// That is the conservative direction on purpose: a gap counted as zero leaves her as the last
/// shift left her, and the next read — the ticker's, a minute later, with the block times cached by
/// then — advances her properly. A gap *guessed* at would write a deterioration nobody can check.
pub fn cached_dater(store: &crate::store::Store) -> impl Fn(u64) -> Option<i64> + '_ {
    move |slot| (slot != 0).then(|| store.get::<i64>(SLOT_TIMES, &slot.to_string())).flatten()
}

/// The store's dates, and the ward's own clock for the slot the chain is on *now*.
///
/// Every slot in a patient's history is dated by the call that found it, and exactly one slot never
/// is: the one she is being opened at. No transaction happened in it, so the only way to get it
/// from the chain is `getBlockTime` — one round trip with a reader waiting on it, to be told the
/// time, which this ward already knows.
///
/// So `now_slot` is answered from the clock. It is not a guess about the chain: `now_slot` was read
/// from the chain a moment ago, and what `getBlockTime` would say about it is "about now". The
/// error is the age of that read plus whatever the cluster's clock is behind by — seconds, against
/// idle spans measured in hours — and it is in the honest direction: a span that would otherwise be
/// counted as **zero**, leaving a patient exactly as the last shift left her however long she has
/// been alone.
pub fn dater_to_now(
    store: &crate::store::Store,
    now_slot: u64,
) -> impl Fn(u64) -> Option<i64> + '_ {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    move |slot| {
        if slot != 0 && slot == now_slot {
            return Some(now);
        }
        (slot != 0).then(|| store.get::<i64>(SLOT_TIMES, &slot.to_string())).flatten()
    }
}

pub const PERSONA_STORE: &str = "ward_pack";

/// The packs the ward knows about, by patient id.
///
/// Empty until the factory has run, and empty is a working ward: every entry on the board is
/// published with a null name and a null country rather than withheld, because a patient whose
/// pack has not arrived is still a patient somebody can treat.
pub fn packs(store: &crate::store::Store) -> std::collections::BTreeMap<u64, crate::ward::Pack> {
    store
        .list::<crate::ward::Pack>(PERSONA_STORE)
        .into_iter()
        .filter_map(|(k, v)| k.trim_start_matches('p').parse::<u64>().ok().map(|id| (id, v)))
        .collect()
}

// ── the factory's door ──────────────────────────────────────────────────────

/// The oldest and youngest a patient may be.
///
/// Not a clinical range — a sanity range. A pack built from a case whose band the factory read
/// wrongly arrives here as an age no person has, and this is where that is cheap to catch.
pub const AGE_RANGE: std::ops::RangeInclusive<u16> = 1..=120;

/// Is this pack a patient the ward can actually serve?
///
/// Every rejection below is something that becomes invisible one step later. The factory runs
/// unattended on another machine and pushes here; after this door the pack is a patient on a
/// board, in front of strangers, with her name on her.
///
/// What is **not** checked, and should be said plainly: that her name suits her country, that her
/// age suits her case's band, and that her sex matches the one the case names. The ward cannot
/// check any of those — the band and the sex live inside the case's own text — so they are the
/// factory's responsibility, and the pool the factory draws from is shaped to make them easy to
/// get right.
pub fn validate_pack(p: &crate::ward::Pack) -> Result<(), String> {
    use crate::ward::CATALOGUE;
    // The season's sixteen are vitals.academy's (founder, 16 ก.ย.). A pack may name a case the
    // ward holds, or name none and let the ward place her; whether it *is* held is asked at the
    // door, where the store is.
    if CATALOGUE.contains(&p.case.as_str()) {
        return Err(format!(
            "{} is one of the season's cases, and the ward plays none of them — cases come \
             through /api/ward/case from the case factory",
            p.case
        ));
    }
    if let Some(level) = &p.difficulty {
        if !crate::ward_case::LEVELS.contains(&level.as_str()) {
            return Err(format!(
                "{level} is not a level this ward offers — {:?}, or leave it out",
                crate::ward_case::LEVELS
            ));
        }
    }
    if !p.persona.country_is_alpha3() {
        return Err(format!(
            "{} is not an ISO 3166-1 alpha-3 country code, and the globe matches on alpha-3",
            p.persona.country
        ));
    }
    if p.persona.name.trim().is_empty() {
        return Err("a patient with no name is a patient nobody can talk about".into());
    }
    if !AGE_RANGE.contains(&p.persona.age) {
        return Err(format!("nobody is {}", p.persona.age));
    }
    // The case's own patient, where the case has one. The ward renames her and moves her country
    // — that is the premise — but it may not change what the case was written about.
    //
    // This reads the season's station files and so answers `None` for every case the factory
    // compiles: a compiled pack carries `patient{age,sex}` and the ward's persona overrides it by
    // ruling. It is kept for the patients still mid-stay on season cases and goes with them.
    if let Some(theirs) = crate::ward::case_patient(&p.case) {
        if p.persona.sex != theirs.sex {
            return Err(format!(
                "{} is written for a patient who is {}, and this pack says {} — the dialogue, the \
                 examination and the differential are all written for it",
                p.case, theirs.sex, p.persona.sex
            ));
        }
        let band = crate::ward::age_band(theirs.age);
        if !band.contains(&p.persona.age) {
            return Err(format!(
                "{} is written about a patient of {}, so a pack for it must be {}–{}, not {}",
                p.case, theirs.age, band.start(), band.end(), p.persona.age
            ));
        }
    }
    for (state, src) in &p.portrait {
        portrait_entry(state, src)?;
    }
    // The endemic claim is not checked here: it is a question about the catalogue — whether the
    // case this pack names is tagged endemic, and for which country — and the catalogue lives in
    // the store. `enqueue` asks it, beside the other two questions about this ward at this moment.
    Ok(())
}

/// Where the ward's portraits are published: one bucket, public read, content-addressed objects.
///
/// Production's bucket even for dev packs, because a portrait carries no data about anybody and
/// one set of sixty faces is enough for both.
pub const PORTRAITS: &str = "https://storage.googleapis.com/vitals-world-portraits";

/// Is this exactly a portrait this ward publishes?
///
/// Not "does it look like a url". The board puts this string in an `img src` on a page strangers
/// open, so the check is equality with one shape: the bucket, a 64-character lower-case hex name,
/// and `.webp`. Everything a near miss could smuggle — another host, another bucket, `http`, a
/// traversal segment, a different extension — fails by not being that.
pub fn is_portrait_url(src: &str) -> bool {
    portrait_size(src).is_some()
}

/// Which of the two shapes this address is, if it is either: `Some(true)` for the 256 px sibling.
///
/// One reader for both, so "is this a portrait" and "which size is it" can never disagree — and
/// the second question is what lets the doors refuse a thumbnail filed as a picture.
pub fn portrait_size(src: &str) -> Option<bool> {
    let name = src.strip_prefix(PORTRAITS).and_then(|r| r.strip_prefix('/'))?;
    let stem = name.strip_suffix(".webp")?;
    let (sha, small) = match stem.strip_suffix("-256") {
        Some(sha) => (sha, true),
        None => (stem, false),
    };
    let hex = sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    hex.then_some(small)
}

/// One portrait entry, read: the state it is a picture of, and whether it is the small sibling.
///
/// `Err` carries the sentence a person can act on, because both doors — the queue's and the
/// portrait door — say the same things for the same reasons and there is no second opinion to
/// keep in step.
pub fn portrait_entry(state: &str, src: &str) -> Result<(String, bool), String> {
    let (at, small) = match state.strip_suffix(crate::ward::SMALL) {
        Some(at) => (at, true),
        None => (state, false),
    };
    if at == "dead" {
        return Err("no picture of a dead patient is made — the board shows the last living \
                    state and says died in words"
            .into());
    }
    if !crate::ward::PORTRAIT_LADDER.contains(&at) {
        return Err(format!(
            "{state} is not a state the engine reports, so nothing would ever draw it. The \
             keys are {:?}, each optionally with {}",
            crate::ward::PORTRAIT_LADDER,
            crate::ward::SMALL
        ));
    }
    match portrait_size(src) {
        None => Err(format!(
            "a portrait must be {PORTRAITS}/<sha256>.webp or the same name with -256 before the \
             extension, not {src} — the board renders this as an image on a page strangers open, \
             so the ward publishes two shapes and refuses every other"
        )),
        Some(is_small) if is_small != small => Err(format!(
            "{state} is filed as the {} and {src} is the {} — a thumbnail under the full-size key \
             draws correctly and defeats the point of having two, so the key and the address have \
             to agree",
            if small { "256 px sibling" } else { "full-size picture" },
            if is_small { "256 px sibling" } else { "full-size picture" }
        )),
        Some(_) => Ok((at.to_string(), small)),
    }
}

/// A pack's address: sha256 over its own fields, in a fixed order.
///
/// Content-addressed so the factory can push the same page of packs twice — a retry, a restart, an
/// overlapping window — and queue each patient once. Hashed field by field rather than over
/// serialised JSON, because a serialiser that reorders keys or changes its spacing would rename
/// every patient in the queue without changing one of them.
pub fn pack_id(p: &crate::ward::Pack) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"vitals.ward.pack.v1\n");
    for field in [
        p.case.as_str(),
        p.persona.name.as_str(),
        p.persona.country.as_str(),
        &p.persona.age.to_string(),
        if p.endemic { "endemic" } else { "drawn" },
    ] {
        h.update(field.as_bytes());
        h.update(b"\n");
    }
    // The portraits are **not** hashed. She is the same patient whether or not the picture of her
    // getting worse has been made yet, and the factory adds those to a pack it has already queued
    // — hashing them would make every addition a different woman and queue her twice.
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The word that opens the ward, and the variable that carries it.
pub const DOOR_ENV: &str = "VITALS_WARD_DOOR";

/// The day the ward opened to players. The founder, 21 ก.ย. 2026, 19:00 ICT — every case
/// provisional. **The only copy of this date in the source**: `tests/opening.rs` holds it to one,
/// and every page shows the sentence the API gives it rather than a day typed into the page. A date
/// that lives in three places is three dates — the one that gets updated and the two that go on
/// saying the old thing.
pub const OPENED_ON: &str = "2026-09-21";

/// What the catalogue is, in one sentence that says both facts at once.
///
/// After the opening two things are true together: no case has passed the clinical advisor, and
/// anyone may play. A page that says only the first reads as "not open"; one that says only the
/// second hides the first. The director's words, no pronoun; the date read from [`OPENED_ON`].
///
/// **The door decides which half is true, and that is the whole point of the argument.** The first
/// version composed this from `OPENED_ON` alone, and on the evening of the opening — the door flip
/// blocked by an expired login — production told every visitor the ward was open for play for two
/// hours while `/api/ward/take` answered 404. A claim built from a constant instead of from the
/// state it describes cannot be wrong, because nothing checks it.
///
/// Shut, the date is **absent** rather than unclaimed: a reader who sees "21 Sep 2026" anywhere on
/// the page concludes the ward is open, whatever the words around it say.
pub fn catalogue_status(door: Door) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut parts = OPENED_ON.split('-');
    let (y, m, d) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let month = m.parse::<usize>().ok().and_then(|n| MONTHS.get(n.wrapping_sub(1))).copied().unwrap_or(m);
    let day = d.trim_start_matches('0');
    match door {
        Door::Open => format!(
            "provisional — under review by our clinical advisor · open for play since {day} {month} {y}"
        ),
        Door::Preview | Door::Closed =>
            "provisional — under review by our clinical advisor · not yet open for play".to_string(),
    }
}

/// The three states of the ward's door.
///
/// **Default closed, and every ambiguity resolves closed.** Production carries this code before the
/// ward is meant to be open, and the factory is an unattended job that pushes the moment it has
/// packs — so the thing that opens a public ward has to be somebody deciding, never a deploy
/// landing or a job waking up. The asymmetry is the argument: a ward that stays shut an hour too
/// long costs an hour, and a ward that opens by accident is strangers treating patients nobody
/// chose to release.
///
/// `Preview` is the founder's ruling of 18 ก.ย. and is a set of permissions rather than a flag: the
/// factory may fill the queue, the board may say who is in it, and nothing may put a hand on a
/// patient. It exists because a shut ward with an empty board proves nothing in the week before the
/// fair — the patients are built and nobody can see them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Door {
    Closed,
    Preview,
    Open,
}

impl Door {
    /// May the factory's doors take packs? Both states that are not shut.
    pub fn takes_packs(self) -> bool {
        matches!(self, Door::Open | Door::Preview)
    }

    /// May the ticker admit from the queue? Only an open ward — a bed filled in preview is a
    /// patient nobody may treat.
    pub fn admits(self) -> bool {
        matches!(self, Door::Open)
    }

    /// May a stranger take a head, declare, anchor, release, or leave one behind them?
    pub fn plays(self) -> bool {
        matches!(self, Door::Open)
    }

    /// The word the board publishes, and the one every page branches on.
    pub fn word(self) -> &'static str {
        match self {
            Door::Closed => "closed",
            Door::Preview => "preview",
            Door::Open => "open",
        }
    }

    /// What a stranger is told when they press something this door does not allow.
    ///
    /// One sentence, and it says what will change rather than what is forbidden. A closed ward has
    /// nothing on it to press, so this is the preview sentence and a spare for the shut case.
    pub fn refusal(self) -> &'static str {
        match self {
            Door::Open => "",
            Door::Preview => "the ward opens soon — nobody plays yet",
            Door::Closed => "this ward is not open",
        }
    }
}

/// Read a door out of the word a deploy set.
pub fn door_from(setting: Option<&str>) -> Door {
    match setting.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("open") => Door::Open,
        Some("preview") => Door::Preview,
        _ => Door::Closed,
    }
}

/// The door, as this process is configured.
pub fn door_here() -> Door {
    door_from(std::env::var(DOOR_ENV).ok().as_deref())
}

/// Is the factory's door open enough to take a pack? Kept as its own name because that is the
/// question both factory doors ask, and they asked it before there were three states.
pub fn door_open_here() -> bool {
    door_here().takes_packs()
}

/// Where packs wait for a bed.
pub const QUEUE_STORE: &str = "ward_queue";

/// What one push from the factory did.
///
/// Four numbers rather than an "ok", because the factory is a job on another machine with nobody
/// watching it: it tops the queue up against `depth`, it learns from `rejected` that it is
/// building patients this ward will not take, and `duplicates` tells it its window overlaps
/// without telling it anything is wrong.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Queued {
    pub queued: usize,
    pub duplicates: usize,
    pub rejected: Vec<String>,
    /// How many are waiting, or **null** when the store could not be listed.
    ///
    /// Null rather than zero, because the factory tops the queue up against this number: a zero it
    /// cannot distinguish from "we could not look" is a factory that rebuilds the pool until it
    /// runs out of people. That is not a hypothetical — it is what the first real tick would have
    /// done.
    pub depth: Option<usize>,
    /// Why the depth is unknown, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_error: Option<String>,
}

/// How many packs are waiting for a bed.
pub fn queue_depth(store: &crate::store::Store) -> Result<usize, String> {
    store.keys(QUEUE_STORE).map(|k| k.len())
}

/// Take a page of packs from the factory, keep the ones this ward can serve, and say what happened.
///
/// Each pack is stored under its own content address, so pushing the same page twice queues each
/// patient once — the property the factory's retries depend on and the one that would otherwise
/// put one woman in two beds.
/// Why this pack may not call itself endemic, if it may not.
///
/// True of a pairing and nothing else: the case this pack names is tagged endemic, and the country
/// it is endemic in is the patient's. Dengue is about mosquitoes and meningococcal disease about
/// the dry season in the belt — which is epidemiology, and is only epidemiology while the case and
/// the country actually go together. An endemic tag nothing backs is the claim the rule exists to
/// prevent.
///
/// A pack that names no case may not claim it at all: the ward picks the case afterwards, and which
/// case it picks is exactly what would make the claim true or false.
fn endemic_claim(pack: &crate::ward::Pack, held: &[crate::ward_case::CaseSummary]) -> Option<String> {
    if pack.case.is_empty() {
        return Some(
            "this pack calls itself endemic and names no case — the ward picks the case for a pack \
             that names none, and which case it picks is what would make the claim true or false"
                .to_string(),
        );
    }
    let case = held.iter().find(|c| c.case_id == pack.case)?;
    let where_it_is = match (case.endemic, case.country.as_deref()) {
        (true, Some(country)) if country == pack.persona.country => return None,
        (true, Some(country)) => format!("it is endemic in {country}"),
        (true, None) => "it is endemic in no country the catalogue names".to_string(),
        (false, _) => "the catalogue does not tag it endemic anywhere".to_string(),
    };
    Some(format!(
        "this pack calls itself endemic for {} and {where_it_is} — an endemic tag nothing backs is \
         the claim the rule exists to prevent. The case is {}",
        pack.persona.country, pack.case
    ))
}

pub fn enqueue(store: &crate::store::Store, packs: Vec<crate::ward::Pack>) -> Queued {
    let mut out = Queued::default();
    let held = crate::ward_case::all(store);
    for pack in packs {
        if let Err(why) = validate_pack(&pack) {
            out.rejected.push(why);
            continue;
        }
        // Whether this ward holds the case she was built for. Asked here rather than in the pack's
        // own shape because it is a question about this ward at this moment: the same pack is good
        // the minute after the compiler sends that case, which is why the packs already waiting
        // are left alone rather than deleted.
        // The rule the ward places by, applied to the case the factory named. It held for the
        // patients the ward placed and not for the ones the factory placed, which is the half the
        // factory uses — and a person on a case written about somebody else is a page telling a
        // stranger they are treating a child while the board beside it says sixty-six.
        if let Some(named) = held.iter().find(|c| c.case_id == pack.case) {
            if let Some(why) = crate::ward_case::contradicts(named, &pack.persona) {
                out.rejected.push(why);
                continue;
            }
        }
        // A pack's endemic claim is a claim about a pairing, and the catalogue is where that
        // pairing lives. It was read from `data/endemic.json` — six country→case pairs written for
        // the season's sixteen, empty today — so the first real factory tick had every endemic
        // patient it built turned away at the door: eighteen of the cases this ward holds are
        // tagged endemic by the compiler that wrote them, and the door could not see one of them.
        if pack.endemic {
            if let Some(why) = endemic_claim(&pack, &held) {
                out.rejected.push(why);
                continue;
            }
        }
        // Out of service. The pack is still in the store and the patients on it still open; what a
        // withdrawn case does not get is anybody new.
        if let Some(gone) = held.iter().find(|c| c.case_id == pack.case && c.withdrawn) {
            out.rejected.push(format!(
                "{} has been withdrawn — it stays for the patients already on it, and nobody new \
                 is put on it. Leave the pack's case empty and let the ward choose one",
                gone.case_id
            ));
            continue;
        }
        if !pack.case.is_empty() && !held.iter().any(|c| c.case_id == pack.case) {
            out.rejected.push(format!(
                "{} is not a case this ward holds — send it through /api/ward/case first, or \
                 leave the pack's case empty and let the ward choose one",
                pack.case
            ));
            continue;
        }
        let id = pack_id(&pack);
        if store.get::<crate::ward::Pack>(QUEUE_STORE, &id).is_some() {
            out.duplicates += 1;
            continue;
        }
        match store.put(QUEUE_STORE, &id, &pack) {
            Ok(()) => out.queued += 1,
            // A write that failed is not a queued patient, and saying so is the difference between
            // a factory that tops the queue up and one that believes it already did.
            Err(e) => out.rejected.push(format!("could not queue {id}: {e}")),
        }
    }
    match queue_depth(store) {
        Ok(n) => out.depth = Some(n),
        Err(e) => out.depth_error = Some(e),
    }
    out
}

/// Which queued pack an empty bed should get.
///
/// The two rules here are the two `/api/ward` publishes, so the board and the behaviour cannot
/// drift apart. **No two beds hold the same case at once**, which is why a queue full of cases
/// already on the ward admits nobody — an empty bed is better than a rule broken quietly. And
/// **every difficulty band is represented when beds allow**: the band with fewest patients on the
/// ward wins, which is what stops three intern cases from being the only thing a student can find
/// at four in the morning.
///
/// Deterministic from the ward's own state, ties broken by the pack's own address. A ward that
/// admitted a different patient on each tick from the same state would be one nobody could
/// reproduce — and reproducing it is how a stranger checks us.
/// Which cases this ward can place a patient on, as the caller already holds them.
///
/// An argument rather than a lookup, so [`choose_next`] stays pure: queue in, choice out, the same
/// answer from the same state, which is how a stranger reproduces the ward's admissions.
pub struct Placeable {
    /// Every case the catalogue holds, withdrawn included. Empty means *not read yet* — see
    /// [`Placeable::may_place`].
    known: std::collections::BTreeSet<String>,
    withdrawn: std::collections::BTreeSet<String>,
}

impl Placeable {
    pub fn of(known: &[String], withdrawn: &[String]) -> Placeable {
        let mut all: std::collections::BTreeSet<String> = known.iter().cloned().collect();
        all.extend(withdrawn.iter().cloned());
        Placeable { known: all, withdrawn: withdrawn.iter().cloned().collect() }
    }

    /// As the ward's catalogue reads right now.
    pub fn here(store: &crate::store::Store) -> Placeable {
        let all = crate::ward_case::all(store);
        Placeable {
            known: all.iter().map(|c| c.case_id.clone()).collect(),
            withdrawn: all.iter().filter(|c| c.withdrawn).map(|c| c.case_id.clone()).collect(),
        }
    }

    /// Whether a patient may be admitted onto this case.
    ///
    /// **An empty catalogue places everybody.** A ward that has not read its cases yet is not a
    /// ward that has lost them, and a fresh instance that refused its whole queue would open to
    /// nobody at all — a cold start is exactly what an opening is. `beds_taken` carries the same
    /// guard for the same reason.
    pub fn may_place(&self, case: &str) -> bool {
        self.known.is_empty() || (self.known.contains(case) && !self.withdrawn.contains(case))
    }
}

pub fn choose_next(
    queue: &[(String, crate::ward::Pack)],
    on_ward_cases: &[String],
    placeable: &Placeable,
) -> Option<String> {
    use crate::ward::difficulty_of;
    let band_load = |band: &str| {
        on_ward_cases.iter().filter(|c| difficulty_of(c) == Some(band)).count()
    };
    queue
        .iter()
        .filter(|(_, p)| !on_ward_cases.iter().any(|c| c == &p.case))
        // The door's rule, on the other door. A pack whose case is withdrawn — or that the
        // catalogue has never heard of — would be admitted and judged caseless on the same tick,
        // leaving an account on chain with no case anybody can open.
        .filter(|(_, p)| placeable.may_place(&p.case))
        .min_by_key(|(id, p)| {
            (difficulty_of(&p.case).map(band_load).unwrap_or(usize::MAX), id.clone())
        })
        .map(|(id, _)| id.clone())
}

/// A patient id nobody has used: the clock, or the next free second after it.
///
/// The id is seeded into her account's address, so a reused one does not collide loudly — it finds
/// the account that already exists and writes a second admission over the first one's chart. Three
/// beds filling in the same second is the ordinary case on a ward that has just opened, which is
/// exactly when this matters.
pub fn next_patient_id(now_unix: u64, taken: &[u64]) -> u64 {
    let mut id = now_unix;
    while taken.contains(&id) {
        id += 1;
    }
    id
}

/// Where a catalogue case's scenario file lives under the scenario root.
///
/// The two shapes the repository and the image both use: stations are `demo/stations/<id>.sce.json`
/// and episodes are `demo/scenarios/<id>.json`. Hard-coded rather than searched, because a search
/// that found the wrong file would admit a patient whose chart is a different disease.
pub fn case_path(root: &std::path::Path, case: &str) -> std::path::PathBuf {
    match case {
        // The episodes are shelved under short ids and stored under long filenames, and both
        // spellings are load-bearing: the id is what every table keys on, the filename is what is
        // on disk. `the_ward_and_the_bay_resolve_a_case_to_the_same_file` holds these against the
        // bay's own resolver, because two functions that disagree here play a different patient
        // under the same name.
        "ep2" => root.join("demo/scenarios/ep2-stemi.json"),
        "ep3" => root.join("demo/scenarios/ep3-epiglottitis.json"),
        "ep4" => root.join("demo/scenarios/ep4-pulmonary-embolism.json"),
        "ep5" => root.join("demo/scenarios/ep5-the-night-the-stars-fell.json"),
        _ => root.join("demo/stations").join(format!("{case}.sce.json")),
    }
}

/// The hash a patient is admitted with: her scenario, exactly as it is on disk.
///
/// This is the one thing on chain that says what her stay began as. It is the scenario's own bytes
/// so that anyone with the file can recompute it, and so that a scenario edited after her
/// admission stops matching — which is the point, not a bug.
pub fn scenario_hash(root: &std::path::Path, case: &str) -> Result<[u8; 32], String> {
    let p = case_path(root, case);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(vitals_replay::sce_hash(&text))
}

/// The empty tape behind a shift the ward closed itself, when the chain's own numbers say so.
///
/// The ticker anchors a closing shift with **no steps** and the idle span before it, so the chain
/// reads *died, nobody on shift*. Everything that run hash was built from is on chain — the
/// scenario, the shifts before it, the slot it landed at — so this re-derives it rather than
/// trusting a signer or a shape: replay the gap, compute the hash an empty tape would have
/// produced, and answer `Some(vec![])` only if it is the hash the chain actually carries.
///
/// `None` for a shift somebody played, whoever signed it.
pub fn closing_tape(
    sce_json: &str,
    before: &[crate::ward::ShiftOnChain],
    tape_of: &dyn Fn(&str) -> Option<Vec<vitals_replay::Step>>,
    admitted_slot: u64,
    this: &crate::ward::ShiftOnChain,
    dated: &dyn Fn(u64) -> Option<i64>,
) -> Option<Vec<vitals_replay::Step>> {
    let earlier: Vec<crate::ward::ShiftOnChain> =
        before.iter().filter(|s| s.slot < this.slot).copied().collect();
    let since = earlier.iter().map(|s| s.slot).max().unwrap_or(admitted_slot).max(admitted_slot);
    let (mut st, _) = resumed(sce_json, &earlier, tape_of, admitted_slot, since, dated).ok()?;
    let gap = match (dated(since), dated(this.slot)) {
        (Some(a), Some(b)) if b > a => (b - a) as f64,
        _ => 0.0,
    };
    let r = vitals_replay::shift(&mut st, &[], gap);
    let would_be = vitals_replay::leaf(&vitals_replay::sce_hash(sce_json), &[], &r);
    (would_be == this.run_hash).then(Vec::new)
}

/// The leaves in this list whose tapes are not here.
///
/// Named as a function of its arguments so the boot, the ticker and the board all ask the same
/// question of the same shifts and cannot answer it differently.
pub fn missing_tapes(
    store: &crate::store::Store,
    shifts: &[crate::ward::ShiftOnChain],
) -> Vec<String> {
    shifts
        .iter()
        .map(|s| hex32(&s.run_hash))
        .filter(|h| is_shift_hash(h) && tape_by_hash(store, h).is_none())
        .collect()
}

/// One named part of a pass, and how long it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub what: &'static str,
    pub ms: u64,
}

/// The parts of a pass, in the order they happened, with whatever they do not account for.
///
/// **The reconciliation is the point, not the list.** Both timing incidents here were a true number
/// nobody could attribute — `boot meter +137.6s` timed 150 lines of unrelated work, and
/// `boot sessions +30.2s` turned out to contain the sweep and the tape repair. Each cost an hour
/// and each looked like a correct instrument. So this one prints `elsewhere` for the remainder: a
/// part added to the pass later and left unnamed appears there instead of silently inflating
/// whichever neighbour it sits beside.
///
/// Under a second reads in milliseconds. "0.1s" and "0.0s" look alike and one is forty times the
/// other, which is the same class of mistake one layer down.
pub fn spans_line(took: std::time::Duration, spans: &[Span]) -> String {
    if spans.is_empty() {
        return String::new();
    }
    let say = |what: &str, ms: u64| {
        if ms >= 1_000 { format!("{what} {:.1}s", ms as f64 / 1000.0) } else { format!("{what} {ms}ms") }
    };
    let mut parts: Vec<String> = spans.iter().map(|s| say(s.what, s.ms)).collect();
    // Rounding dust is not a finding: each span is truncated to whole milliseconds, so a pass of
    // n named parts can be short by up to n milliseconds without anything being unaccounted for.
    let named: u64 = spans.iter().map(|s| s.ms).sum();
    let gap = (took.as_millis() as u64).saturating_sub(named);
    if gap > spans.len() as u64 && gap >= 100 {
        parts.push(say("elsewhere", gap));
    }
    parts.join(" · ")
}

/// Whether this patient's signatures have to be listed at all.
///
/// The listing is what devnet charges for: 00064 spent 99.8 s over 26 patients, median 674 ms and
/// max 10.7 s, with roughly half of them in a multi-second backoff. So the pass asks first whether
/// a listing could tell it anything, using two facts it already has:
///
///   * `chain_shifts` — [`crate::ward::PatientOnChain::shifts`], the chain's own count of her
///     leaves, which arrives in the one `patients()` call the tick makes for everybody anyway;
///   * `tapes_all_present` — a local question about the store, no RPC.
///
/// Nothing is remembered between passes on purpose. An in-memory note of the last signature would
/// be empty on the first pass after a start, and on a service that redeploys every commit the first
/// pass is the one worth making fast.
///
/// Both halves are needed: the count alone misses a tape that left the store while the chain stood
/// still, which is the failure the repair exists for. A cache holding *more* than the chain says
/// exists is listed rather than trusted, because a duplicate could otherwise hide a real leaf
/// behind a count that happens to match.
pub fn needs_listing(chain_shifts: u32, cached_shifts: usize, tapes_all_present: bool) -> bool {
    chain_shifts as usize != cached_shifts || !tapes_all_present
}

/// What `POST /api/ward/tick` answers, given what the gate gave it.
///
/// `None` is the gate held — a pass is already running, in the ticker thread or an earlier
/// request — and the answer is **409, never a wait**: the pass writes to the chain (`reap` anchors
/// closing shifts), so two at once could close one patient twice, and a request that blocked
/// would hold the ward's only request thread. The scheduler tries again next minute.
///
/// `Some` is the pass, and the body is the instrument: the same facts the slow-pass line prints,
/// so Cloud Scheduler's own response log shows every pass's shape without anybody reading
/// container logs. Pure so both answers can be pinned without a race in the test.
pub fn tick_response(ran: Option<Ticked>, took: std::time::Duration) -> (u16, serde_json::Value) {
    match ran {
        None => (409, serde_json::json!({
            "error": "a pass is already running on this ward — the ticker or an earlier request \
                      holds it. Not queued: the pass writes to the chain and two at once could \
                      close the same patient twice. Ask again next minute",
        })),
        Some(t) => (200, serde_json::json!({
            "took_ms": took.as_millis() as u64,
            "patients": t.checked,
            "listed": t.listed,
            "admitted": t.admitted,
            "closed": t.closed,
            "open": t.open,
            "depth": t.depth,
            "pace": t.pace,
            "spans": t.spans,
            "notes": t.notes,
        })),
    }
}

/// A pass worth a line, and the passes that are not.
///
/// Every minute on a ward that is usually quiet, so a line per pass is a line nobody reads.
pub const SLOW_PASS: std::time::Duration = std::time::Duration::from_secs(10);

/// How a pass's time was spread across the patients it walked.
///
/// **The median against the max, never a mean, and that is the whole point of the type.** 00063's
/// first ticker pass took 100.1 s over the same 26 patients that boot's repair had walked in 38.3 s
/// four minutes earlier — same chain, same data, same code. Two candidate causes, with opposite
/// fixes:
///
///   * Cloud Run allocates CPU only while a request is being processed, so a background thread on
///     an idle instance is throttled. Then every patient costs the same slowed amount, and the
///     answer is to stop doing the work between requests — not to do more of it at once.
///   * devnet rate-limits the signature listings and the retries back off. Then most patients are
///     quick and a few are very slow, and parallel listings with a retry is exactly the fix.
///
/// A mean cannot tell those apart: 3.8 s is the average of 26 patients at 3.8 s *and* of 24 at
/// 0.4 s plus two at 44 s. `median_ms` beside `max_ms` separates them on sight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct Pace {
    pub patients: usize,
    pub median_ms: u64,
    pub max_ms: u64,
}

/// The shape of a pass, from what each patient cost, in the order the chain listed them.
pub fn pace(each_ms: &[u64]) -> Option<Pace> {
    if each_ms.is_empty() {
    return None;
    }
    let mut sorted = each_ms.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    // Even counts take the two middle values averaged, which is the convention a reader expects;
    // the figure is a diagnostic and the arithmetic is stated here so nobody has to infer it.
    let median_ms = if n % 2 == 1 { sorted[n / 2] } else { (sorted[n / 2 - 1] + sorted[n / 2]) / 2 };
    Some(Pace { patients: n, median_ms, max_ms: sorted[n - 1] })
}

/// What to say about a pass, or nothing.
///
/// Silent below [`SLOW_PASS`], and silent over no patients however long the pass took: a
/// per-patient figure over nobody is a lie, and that time went to the sweep or the refill.
pub fn slow_pass_note(
    took: std::time::Duration,
    pace: Option<Pace>,
    spans: &[Span],
) -> Option<String> {
    if took < SLOW_PASS {
        return None;
    }
    let p = pace?;
    let mut said = format!(
        "slow pass · {:.1}s over {} patients checked · median {}ms · max {}ms each",
        took.as_secs_f64(),
        p.patients,
        p.median_ms,
        p.max_ms
    );
    // Two different questions, one line. The per-patient shape answers "is one patient expensive
    // or are they all" — even is a throttled thread, lumpy is a rate-limited listing. The spans
    // answer "which part of the pass", which on 00065 was the question that mattered: the shape
    // accounted for three seconds of a pass that took twenty-eight.
    let parts = spans_line(took, spans);
    if !parts.is_empty() {
        said.push_str(" · ");
        said.push_str(&parts);
    }
    Some(said)
}

/// Put back every tape the chain names and this ward has lost, from what the server still holds.
///
/// Called at boot **before any session is dropped** — a ward session is restored by rebuilding its
/// patient from the chain, so the session holding the missing tape is exactly the one that fails
/// to restore, and the loop that noticed used to delete it. Called again each tick, for whatever
/// has gone missing since.
pub fn repair_tapes(
    chain: &WardChain,
    store: &crate::store::Store,
    root: &std::path::Path,
    patients: &[crate::ward::PatientOnChain],
    held: &[(String, Vec<vitals_replay::Step>)],
) -> Repaired {
    let mut out = Repaired::default();
    let packs_now = packs(store);
    // **Made once for the whole pass, and the two legs then mean different things — deliberately.**
    // `until` is an absolute instant, so sharing one budget makes the deadline *pass-wide*: the pass
    // is bounded at ten seconds however many patients need listing. `entries` is counted from zero
    // for each patient, so it stays a per-patient cap. A deadline given to each patient separately
    // would bound nobody — twenty-six patients at ten seconds each is four minutes, which is the
    // thing being fixed.
    //
    // A patient who gets no time this pass keeps the cache she had, is flagged incomplete, and is
    // first in line next pass: the ones ahead of her are up to date by then and `needs_listing`
    // skips them before the budget is consulted at all. So the turn rotates without anybody
    // tracking whose turn it is.
    let budget = Budget::pass();
    for p in patients {
        let began = std::time::Instant::now();
        let (notes, listed, seen, read) =
            repair_one(chain, store, root, p, held, &packs_now, &budget);
        out.notes.extend(notes);
        out.listed += usize::from(listed);
        out.cached.seen.insert(p.patient_id, seen);
        if !read {
            out.cached.unread.insert(p.patient_id);
        }
        // Every patient walked, including the ones that were skipped without a listing. What costs
        // the time is the listing, and a shape drawn only from the patients that needed work would
        // report a fast pass — but a skipped patient's own few milliseconds belong in the shape
        // too, because they are what the skip is worth.
        out.each_ms.push(began.elapsed().as_millis() as u64);
    }
    out
}

/// What a pass did, and what it cost per patient.
#[derive(Debug, Default)]
pub struct Repaired {
    pub notes: Vec<String>,
    /// What the rest of the pass reads instead of asking the chain again.
    pub cached: Cached,
    /// How many of them actually cost a signature listing — the number the rate limit is charged
    /// against, and the one that says whether [`needs_listing`] is earning its keep.
    pub listed: usize,
    /// One entry per patient walked, in the order the chain listed them. Read through [`pace`].
    pub each_ms: Vec<u64>,
}

/// What one patient's history may cost one pass.
///
/// **Two legs, and a count alone would not do.** Unthrottled a `get_transaction` is about 40 ms;
/// throttled it is about a second. So an entry count is 8 s on a good day and 200 s on a bad one,
/// and the bad day is the one this exists for — 00067 spent 51 s inside one history. The clock is
/// what bounds the pass. The entry count is the belt to its braces: it keeps one absurd history
/// from queueing behind a clock that has not run out yet.
///
/// Stopping is not a failure and shares the path that a failure takes — what was read is kept, the
/// cursor stops at it, the next pass resumes there, and nothing is decided on a part of a history.
/// That path was already there; a budget is what reaches it deliberately rather than by accident.
pub struct Budget {
    /// Transactions this pass may read for one patient.
    pub entries: usize,
    /// When this patient's turn is over, or `None` for a caller that must read to the end.
    pub until: Option<std::time::Instant>,
}

impl Budget {
    /// The ticker's pass: bounded so a long history costs several passes and holds the ward for
    /// none of them. Ten seconds because the pass runs every sixty and answers nobody while it is
    /// working; two hundred entries because at a shift a minute nothing real comes close.
    pub fn pass() -> Budget {
        Budget {
            entries: 200,
            until: Some(std::time::Instant::now() + std::time::Duration::from_secs(10)),
        }
    }

    /// For a caller that needs the whole history and will wait for it.
    pub fn whole_history() -> Budget {
        Budget { entries: usize::MAX, until: None }
    }

    /// Why this patient's turn is over, or `None` to keep reading. Pure: `now` is given, not read.
    pub fn spent(&self, read_so_far: usize, now: std::time::Instant) -> Option<String> {
        if read_so_far >= self.entries {
            return Some(format!(
                "this pass's {} transactions for one history are spent — the rest is the next \
                 pass's",
                self.entries
            ));
        }
        if self.until.is_some_and(|end| now >= end) {
            return Some(format!(
                "this pass's time for one history ran out after {read_so_far} transactions — the \
                 chain is answering slowly, and the rest is the next pass's"
            ));
        }
        None
    }
}

/// What one read of a patient's history managed.
///
/// `refresh` used to return `Result<usize, String>`, so a history read to its end and one that
/// stopped a third of the way through both came back as `Ok` and no caller could honour the
/// distinction the walk's own comment makes. The shifts past a stop are the *recent* ones, so a
/// partial history is precisely a history that makes a patient look idle — and `reap` ends a stay
/// on that judgement with a chain write.
pub struct Reading {
    /// Shifts new to the cache this read.
    pub added: usize,
    /// Why the walk stopped early, or `None` when the history was read to its end.
    pub stopped: Option<String>,
}

impl Reading {
    /// Whether a decision that writes may be made on this history.
    ///
    /// Separate from whether it is worth keeping, which it always is: the progress cost one RPC
    /// round trip per signature, and throwing it away means the next pass walks the same
    /// transactions from the same place again.
    pub fn whole(&self) -> bool {
        self.stopped.is_none()
    }
}

/// Whether this pass may end a stay on what it read about her.
///
/// No entry at all is a refusal too: absence of a history is not evidence of an idle one.
pub fn may_close(cached: &Cached, patient_id: u64) -> bool {
    cached.seen.contains_key(&patient_id) && !cached.unread.contains(&patient_id)
}

/// The shift caches the repair leaves behind, and which of them it could not confirm.
///
/// **One type rather than two parameters, because the two facts are only safe together.** `seen`
/// alone invites a caller to read an entry the chain refused to confirm, and `reap` ends a stay
/// with a chain write — the first version of this refactor passed them separately and dropped
/// `reap`'s refusal without noticing. Kept as a pair, a caller that has the cache also has the
/// warning about it.
#[derive(Debug, Default)]
pub struct Cached {
    /// Every patient, current by fetch for the ones that were listed and current by inference for
    /// the ones that were skipped — the skip's own premise is that the chain's leaf count already
    /// matches the cache.
    pub seen: std::collections::BTreeMap<u64, Seen>,
    /// The patients whose listing failed. Their `seen` entry is whatever was on disk, which answers
    /// a local question (is a tape missing under a leaf we already know?) and must not answer one
    /// that writes.
    pub unread: std::collections::BTreeSet<u64>,
}

/// One patient's repair — the unit the pass's cost is measured in.
///
/// Its own function rather than a block inside the loop because the timing is per patient, and a
/// measurement whose boundary is not also a boundary in the code drifts from what it claims.
fn repair_one(
    chain: &WardChain,
    store: &crate::store::Store,
    root: &std::path::Path,
    p: &crate::ward::PatientOnChain,
    held: &[(String, Vec<vitals_replay::Step>)],
    packs_now: &std::collections::BTreeMap<u64, crate::ward::Pack>,
    budget: &Budget,
) -> (Vec<String>, bool, Seen, bool) {
    let mut notes = Vec::new();
    let key = format!("p{}", p.patient_id);
    let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
    // Ask before paying. `p.shifts` came free with the patient, and the tape check is local; the
    // listing below is the one that waits on devnet's mood.
    let known = seen.shifts();
    if !needs_listing(p.shifts, known.len(), missing_tapes(store, &known).is_empty()) {
        return (notes, false, seen, true);
    }
    // Three outcomes, and the two that are not a clean read are different from each other.
    //
    //   * **Failed** — nothing was read, so nothing is persisted and nothing may be decided.
    //   * **Stopped short** — some of her history was read and the cursor advanced. That progress
    //     cost one round trip per signature and is kept, or the next pass walks the same
    //     transactions from the same place again, for ever. But the shifts past the stop are the
    //     *recent* ones, so she is exactly the patient an incomplete reading makes look idle, and
    //     `reap` must not end her stay on it.
    //   * **Whole** — persisted, and safe to decide on.
    //
    // Persistence and trust are separate decisions. Conflating them is what the old code did in
    // both directions at once: it persisted a partial history *and* trusted it.
    let whole = match chain.refresh(p.patient_id, &mut seen, store, budget) {
        Err(e) => {
            notes.push(format!("patient {}'s history could not be read: {e}", p.patient_id));
            return (notes, true, seen, false);
        }
        Ok(reading) => {
            if let Some(why) = &reading.stopped {
                notes.push(format!(
                    "patient {}: history read only as far as it could be ({why}) — what was \
                     read is kept, and no stay is ended on part of a history",
                    p.patient_id
                ));
            }
            reading.whole()
        }
    };
    let _ = store.put(SHIFT_CACHE, &key, &seen);
    let shifts = seen.shifts();
    let missing = missing_tapes(store, &shifts);
    if missing.is_empty() {
        return (notes, true, seen, whole);
    }
    // Her case, for the closing-shift recovery. Without a pack there is no scenario to replay
    // against and nothing can be re-derived — which is itself worth one line, not ten.
    let sce = packs_now
        .get(&p.patient_id)
        .and_then(|k| {
            crate::ward_case::sce_of(store, &k.case)
                .or_else(|| std::fs::read_to_string(case_path(root, &k.case)).ok())
        });

    let (mut back, mut gone) = (Vec::new(), Vec::new());
    for hash in missing {
        let found = recover_tape(store, p.patient_id, &hash, held).or_else(|| {
            // The ward's own closing shift: no steps, and the chain's numbers prove it.
            let sce = sce.as_deref()?;
            let this = shifts.iter().find(|s| hex32(&s.run_hash) == hash)?;
            let tape = closing_tape(sce, &shifts, &|h| tape_by_hash(store, h), p.admitted_slot,
                                    this, &dater(chain, store))?;
            keep_tape(store, &StoredTape {
                patient_id: p.patient_id,
                run_hash: hash.clone(),
                steps: tape.clone(),
            })
            .ok()?;
            Some(tape)
        });
        match found {
            Some(_) => back.push(hash),
            None => gone.push(hash),
        }
    }
    // One line per patient per pass. The boot printed the same sentence ten times for one old
    // test patient, which is ten times less readable than saying it once with the count.
    // Named, not counted. A tape put back is a leaf on chain that had nothing under it until
    // this pass — the ward is saying it nearly lost the only record of what somebody did, and
    // "2 repaired" does not let anybody check which two or go and look at them.
    if !back.is_empty() {
        notes.push(format!(
            "patient {}: {} missing tape(s) put back — {}",
            p.patient_id,
            back.len(),
            back.join(", ")
        ));
    }
    if !gone.is_empty() {
        notes.push(format!(
            "patient {}: {} shift(s) the chain names have no tape here — {}. That chart cannot \
             be rebuilt, so the patient is off the board rather than in a bed nobody can take",
            p.patient_id,
            gone.len(),
            gone.join(", ")
        ));
    }
    (notes, true, seen, whole)
}

/// Which open patients this ward cannot rebuild, and the leaf each one stopped at.
///
/// The same question `read_ward` answers for the board, asked by the ticker so a bed nobody can
/// take is refilled rather than held. Cheap: the shift cache is already on disk and the tapes are
/// looked up by hash.
fn lost_tapes(
    store: &crate::store::Store,
    patients: &[crate::ward::PatientOnChain],
    cached: &Cached,
) -> std::collections::BTreeMap<u64, String> {
    let mut lost = std::collections::BTreeMap::new();
    for p in patients.iter().filter(|p| p.state == crate::ward::OPEN) {
        // The repair's own cache for her, not another listing. It ran moments ago over every
        // patient and this is what it ended holding.
        let Some(seen) = cached.seen.get(&p.patient_id) else { continue };
        for s in seen.shifts() {
            let hash = hex32(&s.run_hash);
            if is_shift_hash(&hash) && tape_by_hash(store, &hash).is_none() {
                lost.entry(p.patient_id).or_insert(hash);
            }
        }
    }
    lost
}

/// Close every open patient the engine has already finished.
///
/// Read, decide, write — and every step says why it stopped when it stops. A patient the ward
/// cannot rebuild is left alone and named in the notes rather than closed on a chart nobody can
/// check: an unreadable chart is a reason to say so, never a reason to end a stay.
fn reap(
    chain: &WardChain,
    store: &crate::store::Store,
    root: &std::path::Path,
    patients: &[crate::ward::PatientOnChain],
    packs_now: &std::collections::BTreeMap<u64, crate::ward::Pack>,
    cached: &Cached,
    out: &mut Ticked,
) {
    use crate::ward::OPEN;
    let now_slot = match chain.slot() {
        Ok(s) => s,
        Err(e) => {
            out.notes.push(format!("the slot could not be read, so nobody was closed: {e}"));
            return;
        }
    };

    for p in patients.iter().filter(|p| p.state == OPEN) {
        // Somebody is in the room with her. Whatever the engine has reached, it is theirs to find
        // and theirs to anchor — the ward does not close a patient out from under a shift.
        if p.lease_holder != [0; 32] && now_slot < p.lease_until_slot {
            continue;
        }
        let Some(pack) = packs_now.get(&p.patient_id) else { continue };
        // The ward's own catalogue first, the season's files for the three mid-stay patients —
        // the same order `ward_sce` uses on the request side, so the ticker and the bedside can
        // never be replaying two different scenarios for one patient.
        let from_door = crate::ward_case::sce_of(store, &pack.case);
        let Some(sce) = from_door.or_else(|| std::fs::read_to_string(case_path(root, &pack.case)).ok()) else {
            out.notes.push(format!(
                "patient {} plays {}, which is not on this host, so the chart cannot be read",
                p.patient_id, pack.case
            ));
            continue;
        };

        // The repair's cache again, not a third listing. A patient it could not read is one it
        // has no entry for, and closing a stay on a history nobody could read is the thing this
        // function refuses to do anywhere else — so she is left alone and named.
        // A stay is ended with a chain write, so it is ended only on a history this pass actually
        // read. The repair already said so in the notes; nothing is added here for the same patient.
        if !may_close(cached, p.patient_id) {
            continue;
        }
        let Some(seen) = cached.seen.get(&p.patient_id) else { continue };

        let found = died_unattended(
            &sce,
            &seen.shifts(),
            &|h| tape_by_hash(store, h),
            p.admitted_slot,
            now_slot,
            &dater(chain, store),
        );
        let closing = match found {
            Ok(Some(u)) => u,
            Ok(None) => continue,
            Err(e) => {
                out.notes.push(format!("patient {} could not be rebuilt and was left as found: {e}", p.patient_id));
                continue;
            }
        };

        // The head she died on, read now rather than remembered: the program refuses an anchor
        // that does not extend the head it was told, and that refusal is the mechanic.
        let head = match chain.patient(p.patient_id) {
            Ok(Some(a)) => a.head,
            Ok(None) => continue,
            Err(e) => {
                out.notes.push(format!("patient {}'s account could not be read: {e}", p.patient_id));
                continue;
            }
        };
        let difficulty = match crate::ward::difficulty_of(&pack.case) {
            Some("resident") => vitals_progress::Difficulty::Resident,
            Some("intern") => vitals_progress::Difficulty::Intern,
            _ => vitals_progress::Difficulty::Student,
        };
        match chain.close_unattended(store, p.patient_id, &sce, difficulty, &closing.replay, head) {
            Ok(sig) => {
                out.closed.push(p.patient_id);
                out.notes.push(format!(
                    "patient {} died with nobody on shift — {} after {} slots alone, closed by the \
                     ward — {sig}",
                    p.patient_id, closing.outcome, closing.idle_slots
                ));
            }
            Err(e) => out.notes.push(format!(
                "patient {} is finished and would not close: {e}. That bed stays on the board until \
                 the next tick, which is the honest state — the chain has not been told yet",
                p.patient_id
            )),
        }
    }
}

/// What one minute of the ward doing its own work came to.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Ticked {
    /// The patients released this tick.
    pub admitted: Vec<u64>,
    /// Anything a person would want to know: a refusal, a failed admission, a chain that could
    /// not be read. Never swallowed — a ward that stops admitting in silence looks exactly like a
    /// ward nobody is playing.
    pub notes: Vec<String>,
    pub open: usize,
    /// How many are waiting, or null when the store could not be listed.
    pub depth: Option<usize>,
    /// The patients the ward closed this tick because time alone had finished them.
    pub closed: Vec<u64>,
    /// How the pass's time was spread across those patients, or None if it walked nobody.
    ///
    /// The line printed from this is what tells a throttled thread from a rate-limited listing,
    /// and [`Pace`] says why it is a median and a max rather than an average.
    pub pace: Option<Pace>,
    /// Each named part of the pass and its duration, in the order they happened. Read through
    /// [`spans_line`], which also prints whatever they do not account for.
    pub spans: Vec<Span>,
    /// How many of them cost a signature listing, which is what devnet rate-limits. The gap
    /// between this and `checked` is what [`needs_listing`] saved.
    pub listed: usize,
    /// How many patients this pass walked.
    ///
    /// The repair's cost is *per patient* — 38.3 s over 26 of them on staging 00062, one chain
    /// signature listing each — so the line that reports how long the pass took is unreadable
    /// without it. A reader who sees only the seconds cannot tell a slow ward from a full one.
    pub checked: usize,
}

/// One tick: free beds, and fill them from the queue.
///
/// This is ruling 11 — the refill is an event, not a person noticing. It reads the chain for who
/// is still open, works out how many beds are free, and admits that many from the queue. Nothing
/// here decides anything the endpoint does not publish: the bed count, the no-repeat rule and the
/// band balance are the same ones a stranger reads in `policy`.
///
/// **The order of the three writes is deliberate.** A pack leaves the queue first, is written
/// under the patient id second, and is admitted on chain last. Any crash then costs at most one
/// queued patient, which is invisible and which the factory tops back up. The order that would
/// have been kinder to the queue — admit first — costs the other thing instead: the same woman
/// admitted twice, in two beds, under one name, on a board a judge is looking at.
pub fn tick(
    chain: &WardChain,
    store: &crate::store::Store,
    root: &std::path::Path,
    now_unix: u64,
    held: &[(String, Vec<vitals_replay::Step>)],
) -> Ticked {
    use crate::ward::{arrival_due, arrival_minutes, to_admit, BEDS, OPEN};
    let mut out = Ticked::default();
    // Every part of the pass is timed and named, and `spans_line` reconciles them against the
    // whole — the rule the two boot-marker incidents cost an hour each to learn.
    let mut at = std::time::Instant::now();
    let mut span = |out: &mut Ticked, what: &'static str| {
        out.spans.push(Span { what, ms: at.elapsed().as_millis() as u64 });
        at = std::time::Instant::now();
    };

    let patients = match chain.patients() {
        Ok(p) => p,
        Err(e) => {
            out.notes.push(format!("the chain could not be read, so nobody was admitted: {e}"));
            return out;
        }
    };
    span(&mut out, "patients");
    let mut taken: Vec<u64> = patients.iter().map(|p| p.patient_id).collect();
    let packs_now = packs(store);
    span(&mut out, "packs");
    let mut on_ward_cases: Vec<String> = patients
        .iter()
        .filter(|p| p.state == OPEN)
        .filter_map(|p| packs_now.get(&p.patient_id).map(|k| k.case.clone()))
        .collect();

    // ── the patients nobody came back to ────────────────────────────────────────────────────
    //
    // The idle cap went on 16 ก.ย., so time alone can end a stay, and the founder's ruling is that
    // the ward writes that down rather than the next stranger: she is closed here, with an empty
    // tape and the idle span, and the chain then reads *died, nobody on shift*.
    //
    // Her bed is **not** refilled this tick. `out.open` below is counted from the patients read at
    // the top of this function, where she is still open — so the board carries her last state and
    // the sentence for one minute before the queue takes the bed. That minute is the only time
    // anybody sees that somebody died there, and it is the ward the founder chose.
    // Before anything else: a leaf on chain whose tape this ward has lost. She cannot be opened,
    // rebuilt or closed until it is back, so the repair runs ahead of the reaping that needs it.
    out.checked = patients.len();
    let repaired = repair_tapes(chain, store, root, &patients, held);
    out.pace = pace(&repaired.each_ms);
    out.listed = repaired.listed;
    out.notes.extend(repaired.notes);
    span(&mut out, "repair");
    // Asked once, after the repair has had its go: the tapes it put back are not missing any more,
    // and the beds it could not save are the ones this tick must give up.
    let lost = lost_tapes(store, &patients, &repaired.cached);
    span(&mut out, "lost");
    reap(chain, store, root, &patients, &packs_now, &repaired.cached, &mut out);
    span(&mut out, "reap");

    // Beds, not chain rows: a patient the ward cannot describe holds none (`beds_taken`), so she
    // blocks no admission. Three test patients that reached the chain outside the queue wedged
    // staging shut against a full queue on 16 ก.ย., and this is the rule that unwedges it. The
    // catalogue is passed for the same reason it is passed to the board: a patient whose case this
    // ward no longer holds cannot be opened by anybody, so her bed goes back into service and this
    // tick admits into it.
    out.open = crate::ward::beds_taken(
        &patients, &packs_now, &lost, &crate::ward_case::all(store));
    span(&mut out, "beds");
    let depth = match queue_depth(store) {
        Ok(n) => {
            out.depth = Some(n);
            n
        }
        Err(e) => {
            // Nobody is admitted against a queue we could not read. Admitting from a list we
            // cannot see is how one pack becomes two patients.
            out.notes.push(format!("the queue could not be read, so nobody was admitted: {e}"));
            return out;
        }
    };

    // Nobody is admitted unless the door is open. In preview the queue grows, the board shows who
    // is in it, and the beds stay empty — a patient admitted then is a patient nobody may treat,
    // sitting on a bed with a chain account and a lease nobody can take.
    let door = door_here();
    if !door.admits() {
        if depth > 0 {
            out.notes.push(format!(
                "the door is {} — {depth} waiting, nobody admitted", door.word()
            ));
        }
        return out;
    }

    // ── whether one is due on the clock ────────────────────────────────────────────────
    //
    // The last admission, from the chain: the newest `admitted_slot` among the patients this pass
    // already read, dated by the same dater the board uses. Nothing is stored and nothing is
    // counted here — the answer is a fact about the chain, and a reader with the program id gets
    // the same one.
    let dater = cached_dater(store);
    let last_admission = patients
        .iter()
        .map(|p| p.admitted_slot)
        .max()
        .and_then(&dater);
    let due = arrival_due(last_admission, now_unix as i64, arrival_minutes());
    if due {
        out.notes.push(format!(
            "an arrival is due — the last was {} minutes ago and the ward admits one every {}",
            last_admission.map(|t| (now_unix as i64 - t) / 60).unwrap_or(0),
            arrival_minutes()
        ));
    }

    for _ in 0..to_admit(out.open, BEDS, depth, due) {
        let queue = store.list::<crate::ward::Pack>(QUEUE_STORE);
        let Some(id) = choose_next(&queue, &on_ward_cases, &Placeable::here(store)) else {
            out.notes.push(
                "a bed is free and every queued patient has a case already on the ward — no two \
                 beds hold the same case at once, so the bed waits for the factory"
                    .into(),
            );
            break;
        };
        let Some((_, queued)) = queue.iter().find(|(k, _)| k == &id) else { break };
        let mut pack = queued.clone();

        // ── which case she is admitted onto ─────────────────────────────────────────────────
        //
        // The ward's own catalogue, never `demo/**`: the season's sixteen belong to
        // vitals.academy (founder, 16 ก.ย.). Her pack's `case` is taken as a request — the patient
        // factory will name the case it built her for — and the ward falls back to a case for her
        // country, then to any it holds. Nothing it holds means nobody is admitted, which is an
        // empty bed rather than a patient on a case that does not exist here.
        //
        // The season ids still resolve for the three patients mid-stay on 16 ก.ย. and for nobody
        // else: `sce_of` answers only for cases that came through the door.
        let catalogue = crate::ward_case::all(store);
        let chosen = crate::ward_case::choose_case(
            &catalogue,
            (!pack.case.is_empty()).then_some(pack.case.as_str()),
            &pack.persona,
            pack.difficulty.as_deref(),
        );
        let sce_json = match &chosen {
            Some(c) => crate::ward_case::sce_of(store, &c.case_id),
            None => None,
        };
        let hash = match (&chosen, &sce_json) {
            (Some(c), Some(json)) => {
                pack.case = c.case_id.clone();
                vitals_replay::sce_hash(json)
            }
            _ => {
                out.notes.push(format!(
                    "no case in this ward's catalogue is written about somebody like {} ({}, {}, \
                     {}), so the bed waits rather than opening a case about somebody else. \
                     The case factory fills it through /api/ward/case",
                    pack.persona.name, pack.persona.country, pack.persona.age, pack.persona.sex
                ));
                break;
            }
        };

        let patient_id = next_patient_id(now_unix, &taken);
        store.del(QUEUE_STORE, &id);
        if let Err(e) = store.put(PERSONA_STORE, &format!("p{patient_id}"), &pack) {
            out.notes.push(format!("patient {patient_id}'s pack could not be stored: {e}"));
        }
        match chain.admit(patient_id, hash) {
            Ok(sig) => {
                out.admitted.push(patient_id);
                out.notes.push(format!("admitted {patient_id} with {} — {sig}", pack.case));
                taken.push(patient_id);
                on_ward_cases.push(pack.case.clone());
                out.open += 1;
            }
            Err(e) => {
                out.notes.push(format!(
                    "admitting {patient_id} failed, and that pack is spent: {e}"
                ));
                break;
            }
        }
    }

    span(&mut out, "admissions");

    // Read again at the end: the tick just took packs out of it, and the number the factory tops
    // up against should be the one after this tick rather than before it.
    match queue_depth(store) {
        Ok(n) => out.depth = Some(n),
        Err(e) => out.notes.push(format!("the queue's depth could not be read: {e}")),
    }
    span(&mut out, "queue");
    out
}

/// What one portrait push did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Filled {
    pub added: usize,
    /// States she already had. Not an error: the factory retrying is the ordinary case.
    pub kept: usize,
    pub rejected: Vec<String>,
    /// Every state she has a picture for after this push.
    pub states: Vec<String>,
}

/// Replace pictures on a pack that is still waiting for a bed.
///
/// **Replace, not add** — and only here. While she is in the queue nobody has seen her, so a face
/// that came out wrong can simply be fixed. The moment she is admitted, [`fill_portraits`] is the
/// only door and its rule is add-only: a face the board has shown is one strangers have been
/// treating, and changing it underneath them is exactly what that rule exists to prevent.
///
/// Her address does not move when a portrait does. Portraits are deliberately outside the content
/// hash, so fixing a face is not a different patient queued twice.
pub fn replace_queued_portraits(
    store: &crate::store::Store,
    pack_id: &str,
    portraits: std::collections::BTreeMap<String, String>,
) -> Filled {
    let mut out = Filled::default();
    let Some(mut pack) = store.get::<crate::ward::Pack>(QUEUE_STORE, pack_id) else {
        out.rejected.push(format!(
            "no pack {pack_id} is waiting — that patient may be in a bed already, and an \
             admitted patient's faces are added through their own door and never replaced"
        ));
        return out;
    };

    for (state, src) in portraits {
        // One reader, three doors. A queued face may still be replaced — she is nobody's patient
        // yet — but it is held to the same two shapes as one that is already in a bed.
        if let Err(why) = portrait_entry(&state, &src) {
            out.rejected.push(why);
            continue;
        }
        pack.portrait.insert(state, src);
        out.added += 1;
    }

    if out.added > 0 {
        if let Err(e) = store.put(QUEUE_STORE, pack_id, &pack) {
            out.rejected.push(format!("pack {pack_id} could not be written: {e}"));
            out.added = 0;
        }
    }
    out.states = pack.portrait.keys().cloned().collect();
    out
}

/// Add pictures to a patient the ward has already admitted.
///
/// **Add only.** A portrait already on a patient is one the board may have shown, and a factory
/// that could replace it could change the face of a patient strangers have been treating. A state
/// she already has is `kept`, which is not an error — the factory retrying is the ordinary case.
///
/// Refusals are per entry and nothing half-written lands: a bad key or a bad url takes only itself
/// with it, and the patient's record is written once at the end or not at all.
pub fn fill_portraits(
    store: &crate::store::Store,
    patient_id: u64,
    add: std::collections::BTreeMap<String, String>,
) -> Filled {
    let mut out = Filled::default();
    let key = format!("p{patient_id}");
    let Some(mut pack) = store.get::<crate::ward::Pack>(PERSONA_STORE, &key) else {
        out.rejected.push(format!(
            "no patient {patient_id} has been admitted here, so there is nobody to put a face on"
        ));
        return out;
    };

    for (state, src) in add {
        if pack.portrait.contains_key(&state) {
            out.kept += 1;
            continue;
        }
        // The same reader the queue's door uses, so a face refused there is refused here and for
        // the same stated reason.
        if let Err(why) = portrait_entry(&state, &src) {
            out.rejected.push(why);
            continue;
        }
        pack.portrait.insert(state, src);
        out.added += 1;
    }

    if out.added > 0 {
        if let Err(e) = store.put(PERSONA_STORE, &key, &pack) {
            out.rejected.push(format!("patient {patient_id} could not be written: {e}"));
            out.added = 0;
        }
    }
    out.states = pack.portrait.keys().cloned().collect();
    out
}

// ── the bay, resumed ────────────────────────────────────────────────────────

/// Where a patient's tapes live: one document per shift.
pub const TAPE_STORE: &str = "ward_tape";

/// One shift's tape, kept so the next stranger can be handed the patient it left.
///
/// **The chain holds `run_hash`, not the tape.** These bytes are the off-chain half, and they are
/// addressed by that hash — the chain says which tapes are hers and in what order, and this is
/// only the lookup. A tape that went missing would not let us rewrite her; it would stop her being
/// rebuildable at all, which is why the receipt offers every tape for download.
///
/// No slots on it. When a shift happened is the chain's to say, and a timing we held privately
/// would be one nobody could check.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredTape {
    /// Who it was first kept for. Provenance, not identity — the tape is found by its hash, and
    /// two patients with byte-identical tapes share one record because they are the same bytes.
    pub patient_id: u64,
    /// Hex of the hash the leaf commits to. The key this tape is found by.
    pub run_hash: String,
    pub steps: Vec<vitals_replay::Step>,
}

/// Rebuild the patient as she is now: every anchored shift, in the chain's order, and the time
/// between them.
///
/// **Every input is either on the chain or is bytes the chain committed to.** `shifts` are her
/// AnchorShift transactions in slot order; each tape is found by the `run_hash` in its leaf;
/// `admitted_slot` and `now_slot` are the chain's. Nothing here is a number this server kept to
/// itself, which is what lets a stranger who trusts nobody arrive at the same patient.
///
/// The idle clock runs over three spans, all of them chain arithmetic: admission to the first
/// anchor, anchor to anchor, and the last anchor to now. Anchor-to-anchor over-counts by the
/// length of the shift itself, deliberately — the alternative is a take-slot only our store knows,
/// and a number nobody can check is worth less than one that is slightly generous. The cap bounds
/// it either way.
///
/// `Err` when a tape the chain names cannot be produced. Her chart then **cannot be rebuilt**, and
/// saying so is the only honest answer: replaying what is left would hand somebody a patient who
/// never existed.
pub fn resumed(
    sce_json: &str,
    shifts: &[crate::ward::ShiftOnChain],
    tape_of: &dyn Fn(&str) -> Option<Vec<vitals_replay::Step>>,
    admitted_slot: u64,
    now_slot: u64,
    dated: &dyn Fn(u64) -> Option<i64>,
) -> Result<(vitals_sce::runtime::SceState, usize), String> {
    // How long she was alone between two slots, in real seconds, as the chain dates them. It was
    // the slot count times 0.4 s — and devnet was producing slots at 0.166 s on 17 ก.ย., so every
    // gap was replayed 2.4× longer than it lasted and unattended patients died that much sooner.
    // A span either end of which this ward cannot date advances her by nothing: see `cached_dater`.
    let span = |from: u64, to: u64| -> f64 {
        match (dated(from), dated(to)) {
            (Some(a), Some(b)) if b > a => (b - a) as f64,
            _ => 0.0,
        }
    };
    let mut ordered: Vec<&crate::ward::ShiftOnChain> = shifts.iter().collect();
    ordered.sort_by_key(|s| s.slot);

    let (mut st, _) = vitals_replay::resume(sce_json, &[])?;
    let mut since = admitted_slot;
    for s in &ordered {
        let hash = hex32(&s.run_hash);
        let steps = tape_of(&hash).ok_or_else(|| {
            format!(
                "this chart cannot be rebuilt: the chain says a shift anchored at slot {} with run \
                 hash {hash}, and that tape is not here. Nothing is shown rather than a patient \
                 nobody can check",
                s.slot
            )
        })?;
        vitals_replay::shift(&mut st, &steps, span(since, s.slot));
        since = s.slot;
    }

    // What has happened to her since the last anchor: nothing anybody did, and time.
    vitals_replay::pass_idle(&mut st, vitals_replay::idle_sim_seconds(span(since, now_slot)));
    Ok((st, ordered.len()))
}

/// A death the ward has to write down, and the span it happened in.
///
/// `outcome` is the engine's own word, carried rather than restated. `idle_slots` and `since_slot`
/// are the chain's arithmetic — the last anchor (or her admission) and the gap since — so the
/// record a stranger recomputes is the record we anchored.
pub struct Unattended {
    pub outcome: String,
    pub since_slot: u64,
    pub idle_slots: u64,
    pub replay: vitals_replay::Replay,
    /// Her machine at the moment it stopped — the anchor is built from this, never from a second
    /// replay that might not agree with the first.
    pub state: vitals_sce::runtime::SceState,
}

/// Has the ward finished her while nobody was in the room?
///
/// The founder removed the idle cap on 16 ก.ย., so time alone can now end a stay — and a death
/// nobody records is worse than no death at all: the board would keep offering her, and the next
/// stranger would open a corpse the page still called alive. So the ticker asks this of every open
/// bed, every minute, and closes the ones the engine has already finished.
///
/// Pure and chain-derived: her scenario, her anchored shifts, the tapes those shifts name, the
/// slot she was admitted at and the slot now. `Err` when a tape the chain names cannot be found —
/// her chart cannot be rebuilt, and closing her on a chart nobody can check is the one thing the
/// ward may not do.
pub fn died_unattended(
    sce_json: &str,
    shifts: &[crate::ward::ShiftOnChain],
    tape_of: &dyn Fn(&str) -> Option<Vec<vitals_replay::Step>>,
    admitted_slot: u64,
    now_slot: u64,
    dated: &dyn Fn(u64) -> Option<i64>,
) -> Result<Option<Unattended>, String> {
    // Where her chart stops: the last shift anybody anchored, or her admission if nobody has.
    let since = shifts.iter().map(|s| s.slot).max().unwrap_or(admitted_slot).max(admitted_slot);
    let (mut st, _) = resumed(sce_json, shifts, tape_of, admitted_slot, since, dated)?;
    // The slots stay on the record — they are the chain's own name for the span, and what a
    // stranger re-derives it from. What the engine is handed is what those two blocks say the span
    // lasted in seconds.
    let idle_slots = now_slot.saturating_sub(since);
    let idle_real = match (dated(since), dated(now_slot)) {
        (Some(a), Some(b)) if b > a => (b - a) as f64,
        _ => 0.0,
    };

    // The span itself, as a shift with no steps in it — which is what happened.
    let replay = vitals_replay::shift(&mut st, &[], idle_real);
    let Some(outcome) = replay.outcome.clone() else { return Ok(None) };
    let finished = vitals_progress::record::Outcome::parse(&outcome).is_some_and(|o| {
        matches!(
            o,
            vitals_progress::record::Outcome::DeathArrest
                | vitals_progress::record::Outcome::DeathBiphasic
        )
    });
    if !finished {
        // She reached an ending the ward does not close on its own. A discharge nobody was there
        // to give is not a discharge, and ICU is a transfer: the next stranger continues her.
        return Ok(None);
    }
    Ok(Some(Unattended { outcome, since_slot: since, idle_slots, replay, state: st }))
}

/// A run hash as the tapes are keyed by it.
pub fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// One tape, by the hash its leaf commits to.
///
/// **Addressed by content and by nothing else.** Two patients whose shifts produced byte-identical
/// tapes have the same hash, and that is not a collision to defend against — it is the same bytes,
/// and either patient replaying them arrives where she should. Keying by patient as well would
/// have made one of them unable to find her own tape.
pub fn tape_by_hash(
    store: &crate::store::Store,
    run_hash: &str,
) -> Option<Vec<vitals_replay::Step>> {
    store.get::<StoredTape>(TAPE_STORE, run_hash).map(|t| t.steps)
}

/// Keep a finished shift's tape, addressed by the hash its leaf commits to.
///
/// Content-addressed, so keeping the same tape twice is keeping it once — and so the name it is
/// stored under is the name the chain will call it by.
pub fn keep_tape(store: &crate::store::Store, tape: &StoredTape) -> Result<(), String> {
    store
        .put(TAPE_STORE, &tape.run_hash, tape)
        .map_err(|e| format!("the tape could not be kept, so the shift is unrebuildable: {e}"))
}

/// File the tape under the hash the instruction is about to put on chain.
///
/// **One reduction, one hash.** The hand-over reduces the shift and files the tape under the leaf
/// it computed; the anchor reduces it again to build the record. On 16 ก.ย. those two disagreed —
/// the page's clock kept posting ticks between them — and the chain took the anchor's hash while
/// the store held the hand-over's, leaving a patient nobody could rebuild. So the anchor files it
/// too, under `rec.run_hash` itself rather than under anything computed a second time, and the
/// caller refuses to build the instruction if this fails: a leaf on chain whose tape was never
/// kept is worse than a shift that did not anchor.
pub fn keep_for_anchor(
    store: &crate::store::Store,
    patient_id: u64,
    rec: &vitals_progress::record::AttemptRecord,
    tape: &[vitals_replay::Step],
) -> Result<String, String> {
    let run_hash = hex32(&rec.run_hash);
    keep_tape(store, &StoredTape { patient_id, run_hash: run_hash.clone(), steps: tape.to_vec() })?;
    // And the chain's own name for this shift. The leaf binds the player, the declaration and the
    // tape; the run hash binds only the tape, and two strangers who did the same things share one.
    // It cannot be recomputed later — `RecordWire` carries no commitment, by design — so it is
    // written down here, where the record was built. A failure is not fatal: the receipt is still
    // reachable by run hash, and a leaf nobody can look up is a missing convenience rather than a
    // missing shift.
    let _ = store.put(
        LEAF_INDEX,
        &hex32(&rec.leaf()),
        &serde_json::json!({ "run_hash": run_hash, "patient_id": patient_id }),
    );
    Ok(run_hash)
}

/// Where the leaf of each shift this server anchored is filed, against the tape's own hash.
pub const LEAF_INDEX: &str = "ward_leaf";

/// The tape hash behind a leaf, when this server anchored the shift.
///
/// `None` for a leaf anchored by somebody else's host, or before this index existed — the receipt
/// is then reachable by run hash and by nothing else, which is a smaller address book rather than
/// a wrong answer.
pub fn run_hash_of_leaf(store: &crate::store::Store, leaf: &str) -> Option<String> {
    if !is_shift_hash(leaf) {
        return None;
    }
    store
        .get::<serde_json::Value>(LEAF_INDEX, leaf)?
        .get("run_hash")?
        .as_str()
        .map(str::to_string)
}

/// Find the tape behind a leaf the chain carries and this ward has lost.
///
/// `held` is what the server still has in memory or on disk: each entry a scenario and a tape. A
/// tape that reduces to the missing leaf **is** that shift — the leaf commits to the scenario, the
/// steps and the reduction, so nothing else can produce it — and filing it makes the patient
/// openable again. A tape that reduces to some other leaf is somebody else's shift and is left
/// alone; `None` then, and the board says she cannot be rebuilt rather than this filing a guess
/// under her name.
pub fn recover_tape(
    store: &crate::store::Store,
    patient_id: u64,
    leaf_hex: &str,
    held: &[(String, Vec<vitals_replay::Step>)],
) -> Option<Vec<vitals_replay::Step>> {
    for (sce_json, tape) in held {
        let Ok(r) = vitals_replay::replay(sce_json, tape) else { continue };
        let leaf = vitals_replay::leaf(&vitals_replay::sce_hash(sce_json), tape, &r);
        if hex32(&leaf) != leaf_hex {
            continue;
        }
        keep_tape(store, &StoredTape {
            patient_id,
            run_hash: leaf_hex.to_string(),
            steps: tape.clone(),
        })
        .ok()?;
        return Some(tape.clone());
    }
    None
}

/// The ward's leaf list on chain.
///
/// One tree for the whole ward, seeded on the operator — every shift anybody plays here appends to
/// it, and a proof about one shift is a path through the same tree as everyone else's. A second
/// tree would mean two lists nobody can compare.
pub const WARD_TREE: u64 = 1;

/// An attempt record as the instruction carries it.
pub fn wire(r: &vitals_progress::record::AttemptRecord) -> RecordWire {
    RecordWire {
        player: r.player,
        sce_hash: r.sce_hash,
        case: r.case,
        run_hash: r.run_hash,
        difficulty: r.difficulty as u8,
        exam_mode: r.exam_mode,
        outcome: r.outcome as u8,
        harm_count: r.harm_count,
        rubric_hash: r.rubric_hash,
        det_score: r.det_score,
        det_max: r.det_max,
        judged_score: r.judged_score,
        judged_max: r.judged_max,
    }
}

/// A person's account on the ward's program: the key that plays, opened once.
///
/// The relay pays for the rent and signs for it; the player signs for themselves, and their key is
/// what the account is seeded on. A stranger arrives with no account at all, so this is the first
/// transaction their browser ever signs here.
pub fn open_account_ix(program_id: &Pubkey, operator: &Pubkey, player: &Pubkey) -> SolInstruction {
    SolInstruction::new_with_borsh(
        *program_id,
        &Instruction::OpenAccount,
        vec![
            AccountMeta::new(*operator, true),
            AccountMeta::new_readonly(*player, true),
            AccountMeta::new(account_pda(program_id, player), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    )
}

/// Declare a shift before it is played.
///
/// Commit–reveal, exactly as the bay does it: the hash binds the case and the player before
/// anybody knows how the shift went, and the program stamps the slot. An outcome chosen after the
/// fact cannot be declared retrospectively, which is what makes the record worth anything.
pub fn commit_ix(
    program_id: &Pubkey,
    operator: &Pubkey,
    player: &Pubkey,
    hash: [u8; 32],
) -> SolInstruction {
    SolInstruction::new_with_borsh(
        *program_id,
        &Instruction::Commit { hash },
        vec![
            AccountMeta::new(*operator, true),
            AccountMeta::new_readonly(*player, true),
            AccountMeta::new(account_pda(program_id, player), false),
            AccountMeta::new(commitment_pda(program_id, &player.to_bytes()).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    )
}

/// The ward's own refusals, in words.
///
/// The program says no by a number; a person standing at a bed cannot read `custom program error:
/// 0x10`. Each sentence says what happened to *her* rather than what the program is called,
/// because this is the moment the ward is most worth watching — the chain deciding, in public,
/// against somebody who wanted a different answer.
///
/// `None` for anything that is not one of ours: an outage is not a refusal, and telling a stranger
/// their work was rejected when it was never sent is worse than saying the network failed.
pub fn refusal(err: &str) -> Option<&'static str> {
    Some(match program_code(err)? {
        16 => "this chart moved while you were at the bedside: somebody else anchored a shift on the head \
               you were extending. Your work is still on your tape — open the patient again and it will be \
               played on the chart as it is now",
        17 => "someone is already in the room with this patient. A shift is held until it is anchored or \
               its time runs out, and then the head is free for anybody",
        18 => "this patient has left the ward — a stay that ended is not one anybody can add to",
        19 => "the head is not yours to give back: somebody else holds this shift",
        _ => return None,
    })
}

/// The program's own number, out of whatever the cluster wrapped it in.
fn program_code(err: &str) -> Option<u32> {
    let code = err.split("custom program error: 0x").nth(1)?;
    let code: String = code.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u32::from_str_radix(&code, 16).ok()
}

/// What the ward says when the head moved because *this shift* moved it.
pub const ALREADY_ANCHORED: &str =
    "this shift is already anchored: the chain has it, and the same minutes cannot be filed \
     twice. Nothing was lost by pressing again";

/// The same refusals, read against the head the chain is holding now.
///
/// `StaleHead` has two stories and the ward told one of them for both. "Somebody else anchored a
/// shift on the head you were extending" is true when somebody else did; after a second press of
/// Hand over that somebody else is us, and a stranger was shown a refusal for the shift the chain
/// had just taken from them.
///
/// So: if the head on chain is this shift's own leaf, the shift is anchored and the words say that.
/// If it is anybody else's — or the chain could not be read at the moment of the refusal — the
/// general sentence stands, because a guess here would tell somebody their work is safe when it
/// may not be.
pub fn anchor_refusal(
    err: &str,
    head_now: Option<[u8; 32]>,
    ours: [u8; 32],
) -> Option<&'static str> {
    let said = refusal(err)?;
    if program_code(err) == Some(16) && head_now == Some(ours) {
        return Some(ALREADY_ANCHORED);
    }
    Some(said)
}

/// The words a case wrote beside each of its own intervention ids.
///
/// A tape carries ids — `tx_oxygen`, `ask_chest_abdominal_and_flank_pain` — because that is what
/// the case is keyed by, what the matcher ruled on, and what a verifier re-runs; it is the same run
/// in any language. None of that is a reason to show them to a reader. The case carries the words
/// beside the id and a receipt is built with the case in hand.
///
/// Empty for a case that cannot be read: an unreadable case is not a licence to invent phrases.
pub fn labels_of(sce_json: &str) -> std::collections::BTreeMap<String, String> {
    let Ok(sce) = serde_json::from_str::<serde_json::Value>(sce_json) else {
        return Default::default();
    };
    sce.get("interventions")
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|i| {
                    let id = i.get("id").and_then(serde_json::Value::as_str)?;
                    let label = i.get("label").and_then(serde_json::Value::as_str)?;
                    Some((id.to_string(), without_its_row(label)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The words for one id, or `None` — which means the tape's own text stands. A receipt that invented
/// a phrase for an id nobody wrote would be worse than one that shows the id.
pub fn label_for(sce_json: &str, id: &str) -> Option<String> {
    labels_of(sce_json).remove(id)
}

/// A label without the row it is already under.
///
/// The compiler writes "Ask: Chest, abdominal and flank pain", and a receipt that has just printed
/// ASKED in its own column does not need the label to say it again. The tray strips the same four
/// words for the same reason (`chipText` in `bay.js`, where the rule is written for the page) —
/// and anything else is left exactly as the author wrote it, including "Diagnosis:", which is part
/// of what that label says rather than a row heading.
fn without_its_row(label: &str) -> String {
    let Some((head, rest)) = label.split_once(':') else {
        return label.to_string();
    };
    let row = head.trim().to_ascii_lowercase();
    if matches!(row.as_str(), "ask" | "examine" | "order" | "give") && !rest.trim().is_empty() {
        return rest.trim().to_string();
    }
    label.to_string()
}

/// An id read as the words it already is, for a case that wrote none of its own.
///
/// `tx_source_control` is not a phrase to invent around: it is "source control" with the compiler's
/// row prefix on the front and underscores between. Dropping the prefix and opening the underscores
/// reads it; it does not add anything that was not in it. That is the whole licence — a receipt must
/// never make up a phrase for an id nobody wrote, and must never print `tx_source_control` at a
/// reader either.
///
/// A typed order is not an id and comes back untouched: "oxygen face mask 15 lpm" is what somebody
/// wrote, and what the tape kept.
pub fn id_as_words(id: &str) -> String {
    let body = id
        .split_once('_')
        .filter(|(row, _)| matches!(*row, "ask" | "exam" | "ix" | "tx" | "dx"))
        .map_or(id, |(_, rest)| rest);
    body.replace('_', " ")
}

/// What one shift was, for somebody who never played it.
///
/// Everything here is either on the chain or recomputed in front of the reader from bytes the
/// chain committed to. `rubric` is the case's mark sheet when it has one; without it the
/// deterministic score is absent rather than zero, because zero is a claim and absence is a fact.
///
/// **No judged score.** The judged sixty belongs to a finished case; a shift is a few minutes in
/// the middle of a stay, and a number that cannot mean what a reader assumes is worse than no
/// number. The receipt says that in words rather than leaving a hole.
///
/// **The harm is this shift's own.** `vitals_replay::shift` reports only what this tape added, so
/// a stranger who walked into a patient somebody else hurt is answerable for what they did and
/// nothing else.
// Eight arguments, and every one of them is a different fact this function is not allowed to go
// and find for itself: the case, the mark sheet, her chain, this shift, the tapes, the pack, the
// slot she was admitted at, and how to date a slot. A struct here would be the same eight fields
// with a name, and the callers would fill it in the same order.
#[allow(clippy::too_many_arguments)]
pub fn receipt(
    sce_json: &str,
    rubric_json: Option<&str>,
    shifts: &[crate::ward::ShiftOnChain],
    this: &crate::ward::ShiftOnChain,
    tape_of: &dyn Fn(&str) -> Option<Vec<vitals_replay::Step>>,
    pack: &crate::ward::Pack,
    admitted_slot: u64,
    dated: &dyn Fn(u64) -> Option<i64>,
) -> Result<serde_json::Value, String> {
    let hash = hex32(&this.run_hash);
    let tape = tape_of(&hash).ok_or_else(|| {
        format!("the tape for {hash} is not here, so this shift cannot be shown at all — a receipt \
                 nobody can check is not a receipt")
    })?;

    // The patient as she was when this stranger arrived: every shift the chain anchored before
    // this one, and the idle time between them.
    let before: Vec<crate::ward::ShiftOnChain> =
        shifts.iter().filter(|s| s.slot < this.slot).copied().collect();
    let (mut st, played) = resumed(sce_json, &before, tape_of, admitted_slot, this.slot, dated)?;
    let r = vitals_replay::shift(&mut st, &tape, 0.0);

    let det = rubric_json.and_then(|rj| vitals_osce::det_for_run(sce_json, &tape, rj).ok());
    // The rows of the mark sheet, so a stranger can see what the case paid for and what it did not.
    // The same sheet `/api/marks` opens at the bell, on a shift that is already over.
    let sheet = rubric_json.and_then(|rj| vitals_osce::sheet_for_run(sce_json, &tape, rj).ok());

    // What this stranger actually did, in the order they did it. The tape is the evidence and this
    // is the tape read out: every order with the intervention it resolved to, every question asked,
    // and the beats the case produced in reply. A receipt that says "13 orders · 0 beats" tells a
    // reader the shape of the shift and nothing about it.
    let mut at = 0.0f64;
    let mut timeline: Vec<serde_json::Value> = Vec::new();
    // Read once for the whole tape rather than per row. `said` is the case's own words for what was
    // done; the id stays beside it, because it is what a verifier re-runs and what the marks are
    // keyed by. A step the case wrote no words for keeps the tape's text and nothing is invented.
    let labels = labels_of(sce_json);
    // The case's own words where it wrote them, and the id read as words where it did not. Never
    // the raw id: it is the key the matcher rules on, which is true and unreadable.
    let said = |key: &str| Some(labels.get(key).cloned().unwrap_or_else(|| id_as_words(key)));
    for step in &tape {
        match step {
            vitals_replay::Step::Tick(dt) => at += dt,
            vitals_replay::Step::Do(text) => timeline.push(serde_json::json!({
                "at": at, "kind": "order", "text": text, "said": said(text) })),
            vitals_replay::Step::Act { text, id } => timeline.push(serde_json::json!({
                "at": at, "kind": "order", "text": text, "id": id,
                "said": said(id).or_else(|| said(text)) })),
            vitals_replay::Step::Ask(q) => timeline.push(serde_json::json!({
                "at": at, "kind": "asked", "text": q, "said": said(q) })),
            _ => {}
        }
    }

    // The same bytes can be anchored more than once: a run hash is the hash of the tape, and two
    // strangers who did exactly the same things to the same case produce the same one. Their
    // leaves differ — a leaf carries the player and the commitment — so those are different shifts
    // wearing one address, and a receipt that showed the first and said nothing would read as
    // "this is the shift" when the truth is "this is one of three".
    let sharing = shifts.iter().filter(|s| s.run_hash == this.run_hash).count().saturating_sub(1);

    Ok(serde_json::json!({
        "patient_id": this.patient_id,
        "name": pack.persona.name,
        "case": pack.case,
        "shift": played + 1,
        "run_hash": hash,
        "slot": this.slot,
        "player": bs58(&this.signer),
        "did": {
            "beats": r.beats.len(),
            "steps": r.steps,
            "sim_seconds": r.sim_seconds,
            "harm": r.harm_events,
            "outcome": r.outcome,
        },
        "det": det.map(|(earned, max, _)| serde_json::json!({ "earned": earned, "max": max })),
        // Every row of the sheet, costliest first: what was earned, what was not, and what it was
        // worth. A total with no rows is a mark nobody can learn from.
        "items": sheet.as_ref().map(|(_, d)| d.by_loss().iter().map(|i| serde_json::json!({
            "label": i.label,
            "kind": i.kind,
            "mark": i.mark.as_str(),
            "points": i.points,
            "earned": i.earned_points(),
            // What the row *took*, and what for. Zero for every check but `no_unindicated`, which
            // is the only one that deducts — and the receipt prints it as a deduction rather than
            // as a mark of zero out of zero.
            "penalty": i.penalty,
            "charged": i.charged,
        })).collect::<Vec<_>>()),
        "pass_bps": sheet.as_ref().map(|(r, _)| r.pass_bps),
        "timeline": timeline,
        "status_after": format!("{:?}", st.status),
        "judged": serde_json::Value::Null,
        // One line on the receipt. The reasoning — a judged score belongs to a finished case, and
        // a number that cannot mean what a reader assumes is worse than no number — is in
        // docs/CWF_PLAN.md, where a reader who wants it can find it.
        "judged_omitted": "No AI-judged marks on a mid-stay shift.",
        "also_anchored": sharing,
        "also_anchored_note": (sharing > 0).then(|| format!(
            "{sharing} other shift{} on this ward anchored the same tape — the same bytes, played \
             again. They are different shifts: each leaf carries its own player and its own \
             declaration, and only the tape's hash is shared",
            if sharing == 1 { "" } else { "s" }
        )),
        "tape": format!("/api/tape/{hash}"),
        "derivations": {
            "player": "the key that signed this shift's AnchorShift transaction",
            "did": "this tape replayed on the patient the chain says was in that bed — the beats and the \
                    harm are this shift's own, never what it walked into",
            "det": "the case's rubric, recomputed from the tape by the same code the anchor used. \
                    Absent when the case has no rubric — absent rather than zero, because zero is \
                    a claim",
            "run_hash": "the hash this shift's leaf commits to on chain; the tape below hashes to it",
        },
    }))
}

/// A public key as base58, for a receipt a person reads.
fn bs58(bytes: &[u8; 32]) -> String {
    Pubkey::new_from_array(*bytes).to_string()
}

/// Find the shift a run hash names: which patient, and where in her chain.
///
/// Reads the caches first and refreshes only if the hash is not in them, so the ordinary case —
/// somebody opening a receipt for a shift the board has already seen — costs no chain read at all.
/// A hash that is nowhere is reported as such: it may never have anchored, or it may belong to
/// another ward, and this one does not guess between them.
/// Is this string a hash a shift could actually have?
///
/// Sixty-four hex characters, and **not all of them zero**. All-zero is what an uninitialised
/// record deserialises to: a proof-tool leaf in the shift cache carried one, and `/api/shift/000…0`
/// answered with a receipt for it. A tape does not hash to nothing.
pub fn is_shift_hash(run_hash: &str) -> bool {
    run_hash.len() == 64
        && run_hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && run_hash.bytes().any(|b| b != b'0')
}

/// Whether a shift hash is worth asking the chain about.
///
/// The ward holds the tape for every shift it anchored, kept under the hash the leaf commits to.
/// So a hash with a tape behind it may have landed on chain a moment ago with this ward's own
/// cache not yet caught up — one read of the chain settles that. A hash with no tape here is a
/// hash this ward cannot render a receipt for even if the chain confirms it, and it is answered
/// from the store in microseconds rather than in a minute and a half of RPC round trips that any
/// stranger with a URL bar could start.
pub fn worth_reading_the_chain_for(store: &crate::store::Store, run_hash: &str) -> bool {
    is_shift_hash(run_hash) && tape_by_hash(store, run_hash).is_some()
}

pub fn find_shift(
    chain: &WardChain,
    store: &crate::store::Store,
    run_hash: &str,
) -> Result<Option<(u64, Vec<crate::ward::ShiftOnChain>, crate::ward::ShiftOnChain)>, String> {
    if !is_shift_hash(run_hash) {
        return Ok(None);
    }
    let patients = chain.patients()?;
    // The cached pass always; the refreshing one only for a hash this ward has a tape for. The
    // second pass is twenty-odd RPC round trips, it is reached by any 64-hex string a stranger
    // types, and this server answers requests one at a time — see `worth_reading_the_chain_for`.
    let passes: &[bool] = if worth_reading_the_chain_for(store, run_hash) { &[false, true] } else { &[false] };
    for refreshing in passes.iter().copied() {
        for p in &patients {
            let key = format!("p{}", p.patient_id);
            let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
            if refreshing
                && chain.refresh(p.patient_id, &mut seen, store, &Budget::whole_history()).is_ok()
            {
                let _ = store.put(SHIFT_CACHE, &key, &seen);
            }
            // Skipped rather than matched: a cached row with an empty hash is a row about
            // nothing, and it must not be found by a search for anything.
            let all: Vec<crate::ward::ShiftOnChain> =
                seen.shifts().into_iter().filter(|s| is_shift_hash(&hex32(&s.run_hash))).collect();
            if let Some(this) = all.iter().find(|s| hex32(&s.run_hash) == run_hash) {
                return Ok(Some((p.patient_id, all.clone(), *this)));
            }
        }
    }
    Ok(None)
}

/// The rubric a case is marked against, when it has one.
pub fn rubric_of(root: &std::path::Path, case: &str) -> Option<String> {
    std::fs::read_to_string(root.join("demo/rubrics").join(format!("{case}.json"))).ok()
}

/// The mark sheet for a case, wherever this ward keeps it.
///
/// A compiled case's rubric arrives inside the pack, through the case door, and lives in the store;
/// only the season's cases have a file. Every receipt on this ward read the file and printed "not
/// scored — this case has no rubric", which is a fact about where we looked. The receipt is where a
/// stranger finds out what their shift earned.
pub fn rubric_for(
    store: &crate::store::Store,
    root: &std::path::Path,
    case: &str,
) -> Option<String> {
    store
        .get::<serde_json::Value>(crate::ward_case::CASE_STORE, &crate::ward_case::key_for(case))
        .and_then(|pack| pack.get("rubric").cloned())
        .map(|r| r.to_string())
        .or_else(|| rubric_of(root, case))
}
