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
        mut read: impl FnMut(&str, u64) -> Result<Vec<crate::ward::ShiftOnChain>, String>,
    ) -> (Vec<crate::ward::ShiftOnChain>, Option<(String, u64)>, Option<String>) {
        let mut got = Vec::new();
        let mut cursor = None;
        for (sig, slot) in page.into_iter().rev() {
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
    read: impl FnMut(&str, u64) -> Result<Vec<crate::ward::ShiftOnChain>, String>,
) -> (Vec<crate::ward::ShiftOnChain>, Option<(String, u64)>, Option<String>) {
    Seen::walk(page, read)
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
    pub fn refresh(&self, patient_id: u64, seen: &mut Seen) -> Result<usize, String> {
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

        // A failed transaction is not a shift: the program refused it, so nothing was anchored,
        // and counting it would publish work the chain says did not happen. Dropped here rather
        // than in the walk, because a refusal is a thing we read successfully.
        let page: Vec<(String, u64)> = sigs
            .into_iter()
            .filter(|s| s.err.is_none())
            .map(|s| (s.signature, s.slot))
            .collect();

        let mut signatures: std::collections::BTreeMap<u64, String> = Default::default();
        let (shifts, cursor, trouble) = Seen::walk(page, |sig, slot| {
            let parsed = Signature::from_str(sig).map_err(|e| format!("{sig} is not a signature: {e}"))?;
            let tx = self
                .rpc
                .get_transaction(&parsed, UiTransactionEncoding::Base64)
                .map_err(why)?;
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
        if let Some(why) = trouble {
            eprintln!("ward       patient {patient_id}'s history was read as far as it could be — {why}");
        }
        Ok(added)
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

/// A week, in slots. `604800 / 0.4`.
///
/// The window the endpoint calls *this week*. In slots rather than in hours because every other
/// figure on the endpoint is in slots, and a week measured two ways is a week that can disagree
/// with itself.
pub const WEEK_SLOTS: u64 = 1_512_000;

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
    let as_of = match chain.slot() {
        Ok(s) => s,
        Err(e) => return crate::ward::ward_unavailable(chain.source(), &e),
    };
    let patients = match chain.patients() {
        Ok(p) => p,
        Err(e) => return crate::ward::ward_unavailable(chain.source(), &e),
    };

    let mut shifts = Vec::new();
    for p in &patients {
        let key = format!("p{}", p.patient_id);
        let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
        let added = chain.refresh(p.patient_id, &mut seen);
        if matches!(added, Ok(n) if n > 0) {
            let _ = store.put(SHIFT_CACHE, &key, &seen);
        }
        if let Err(e) = added {
            return crate::ward::ward_unavailable(
                chain.source(),
                &format!("patient {}'s history could not be read: {e}", p.patient_id),
            );
        }
        shifts.extend(seen.shifts());
    }

    let mut v = crate::ward::ward_payload(&crate::ward::WardRead {
        patients: &patients,
        shifts: &shifts,
        packs: &packs(store),
        since: Some(as_of.saturating_sub(WEEK_SLOTS)),
        as_of_slot: as_of,
        now_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source: chain.source(),
    });
    // How many patients are waiting, which is the number the factory tops up against and the one
    // a reader of the board uses to tell "nobody is playing" from "nobody is left to play".
    let waiting = queue_depth(store);
    v["queue"] = serde_json::json!({
        // Null, never zero, when the store cannot be listed — "nobody is waiting" and "we could
        // not look" are opposite facts and this endpoint has one rule about those.
        "waiting": waiting.as_ref().ok(),
        "waiting_unknown_because": waiting.as_ref().err(),
        "beds": crate::ward::BEDS,
        // Published, because "the queue is empty" and "the door is shut" look identical from
        // outside and mean opposite things about whether anybody should be doing anything.
        "door": if door_open_here() { "open" } else { "closed" },
        "filled_by": "a ticker on the ward host, every minute: a bed frees on discharge or death \
                      and the next queued patient takes it. Nobody on the team touches anything",
    });
    v
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
    if !CATALOGUE.contains(&p.case.as_str()) {
        return Err(format!(
            "{} is not a case this ward serves — queueing it would put a patient on the board \
             that no shift can open",
            p.case
        ));
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
    if p.endemic {
        let has = crate::ward::endemic()
            .get(&p.persona.country)
            .is_some_and(|cases| cases.iter().any(|c| c == &p.case));
        if !has {
            return Err(format!(
                "this pack calls itself endemic, but the endemic list does not pair {} with {} — \
                 an endemic tag nothing backs is the claim the rule exists to prevent",
                p.persona.country, p.case
            ));
        }
    }
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

/// Is the factory's door open?
///
/// **Default closed, and every ambiguity resolves closed.** Production carries this code before
/// the ward is meant to be open, and the factory is an unattended job that pushes the moment it
/// has packs — so the thing that opens a public ward has to be somebody deciding, never a deploy
/// landing or a job waking up.
///
/// The asymmetry is the whole argument: a ward that stays shut an hour too long costs an hour, and
/// a ward that opens by accident is strangers treating patients nobody chose to release.
pub fn door_is_open(setting: Option<&str>) -> bool {
    setting.is_some_and(|v| v.trim().eq_ignore_ascii_case("open"))
}

/// The door, as this process is configured.
pub fn door_open_here() -> bool {
    door_is_open(std::env::var(DOOR_ENV).ok().as_deref())
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
pub fn enqueue(store: &crate::store::Store, packs: Vec<crate::ward::Pack>) -> Queued {
    let mut out = Queued::default();
    for pack in packs {
        if let Err(why) = validate_pack(&pack) {
            out.rejected.push(why);
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
pub fn choose_next(queue: &[(String, crate::ward::Pack)], on_ward_cases: &[String]) -> Option<String> {
    use crate::ward::difficulty_of;
    let band_load = |band: &str| {
        on_ward_cases.iter().filter(|c| difficulty_of(c) == Some(band)).count()
    };
    queue
        .iter()
        .filter(|(_, p)| !on_ward_cases.iter().any(|c| c == &p.case))
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

/// Put back a tape the chain names and this ward has lost.
///
/// It happened on 16 ก.ย.: an anchor reduced a tape two ticks longer than the one the hand-over
/// had filed, so the chain carried a leaf whose tape was nowhere, and a bed sat on the board that
/// no stranger could take. `keep_for_anchor` is what stops it recurring; this is what repairs the
/// patients it already happened to.
///
/// `held` is every session this server still has. A tape that reduces to the missing leaf **is**
/// that shift — a leaf commits to the scenario, the steps and the reduction together — so filing
/// it is recovery rather than a guess. Nothing that reduces to it means the shift is gone, and the
/// note says so; `read_ward` then marks her unrebuildable on the board rather than offering a bed
/// nobody can take.
fn repair(
    chain: &WardChain,
    store: &crate::store::Store,
    patients: &[crate::ward::PatientOnChain],
    packs_now: &std::collections::BTreeMap<u64, crate::ward::Pack>,
    held: &[(String, Vec<vitals_replay::Step>)],
    out: &mut Ticked,
) {
    use crate::ward::OPEN;
    for p in patients.iter().filter(|p| p.state == OPEN) {
        if !packs_now.contains_key(&p.patient_id) {
            continue;
        }
        let key = format!("p{}", p.patient_id);
        let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
        if chain.refresh(p.patient_id, &mut seen).is_err() {
            continue;
        }
        for shift in seen.shifts() {
            let hash = hex32(&shift.run_hash);
            if tape_by_hash(store, &hash).is_some() {
                continue;
            }
            match recover_tape(store, p.patient_id, &hash, held) {
                Some(t) => out.notes.push(format!(
                    "patient {}: the tape for {hash} was missing and has been put back from a \
                     session still held here — {} steps",
                    p.patient_id,
                    t.len()
                )),
                None => out.notes.push(format!(
                    "patient {}: the chain names a shift at slot {} with run hash {hash} and no \
                     tape here reduces to it. That chart cannot be rebuilt, so the patient is off \
                     the board rather than in a bed nobody can take",
                    p.patient_id, shift.slot
                )),
            }
        }
    }
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
        let path = case_path(root, &pack.case);
        let Ok(sce) = std::fs::read_to_string(&path) else {
            out.notes.push(format!(
                "patient {} plays {}, which is not on this host, so the chart cannot be read",
                p.patient_id, pack.case
            ));
            continue;
        };

        let key = format!("p{}", p.patient_id);
        let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
        if let Err(e) = chain.refresh(p.patient_id, &mut seen) {
            out.notes.push(format!("patient {}'s history could not be read: {e}", p.patient_id));
            continue;
        }
        let _ = store.put(SHIFT_CACHE, &key, &seen);

        let found = died_unattended(
            &sce,
            &seen.shifts(),
            &|h| tape_by_hash(store, h),
            p.admitted_slot,
            now_slot,
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
        match chain.close_unattended(p.patient_id, &sce, difficulty, &closing.replay, head) {
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
    use crate::ward::{to_admit, BEDS, OPEN};
    let mut out = Ticked::default();

    let patients = match chain.patients() {
        Ok(p) => p,
        Err(e) => {
            out.notes.push(format!("the chain could not be read, so nobody was admitted: {e}"));
            return out;
        }
    };
    let mut taken: Vec<u64> = patients.iter().map(|p| p.patient_id).collect();
    let packs_now = packs(store);
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
    repair(chain, store, &patients, &packs_now, held, &mut out);
    reap(chain, store, root, &patients, &packs_now, &mut out);

    // Beds, not chain rows: a patient the ward cannot describe holds none (`beds_taken`), so she
    // blocks no admission. Three test patients that reached the chain outside the queue wedged
    // staging shut against a full queue on 16 ก.ย., and this is the rule that unwedges it.
    out.open = crate::ward::beds_taken(&patients, &packs_now);
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

    for _ in 0..to_admit(out.open, BEDS, depth) {
        let queue = store.list::<crate::ward::Pack>(QUEUE_STORE);
        let Some(id) = choose_next(&queue, &on_ward_cases) else {
            out.notes.push(
                "a bed is free and every queued patient has a case already on the ward — no two \
                 beds hold the same case at once, so the bed waits for the factory"
                    .into(),
            );
            break;
        };
        let Some((_, pack)) = queue.iter().find(|(k, _)| k == &id) else { break };
        let hash = match scenario_hash(root, &pack.case) {
            Ok(h) => h,
            Err(e) => {
                out.notes.push(format!("{} has no scenario here, so that pack was dropped: {e}", pack.case));
                store.del(QUEUE_STORE, &id);
                continue;
            }
        };

        let patient_id = next_patient_id(now_unix, &taken);
        store.del(QUEUE_STORE, &id);
        if let Err(e) = store.put(PERSONA_STORE, &format!("p{patient_id}"), pack) {
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

    // Read again at the end: the tick just took packs out of it, and the number the factory tops
    // up against should be the one after this tick rather than before it.
    match queue_depth(store) {
        Ok(n) => out.depth = Some(n),
        Err(e) => out.notes.push(format!("the queue's depth could not be read: {e}")),
    }
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
) -> Result<(vitals_sce::runtime::SceState, usize), String> {
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
        vitals_replay::shift(&mut st, &steps, s.slot.saturating_sub(since));
        since = s.slot;
    }

    // What has happened to her since the last anchor: nothing anybody did, and time.
    vitals_replay::pass_idle(&mut st, vitals_replay::idle_seconds(now_slot.saturating_sub(since)));
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
) -> Result<Option<Unattended>, String> {
    // Where her chart stops: the last shift anybody anchored, or her admission if nobody has.
    let since = shifts.iter().map(|s| s.slot).max().unwrap_or(admitted_slot).max(admitted_slot);
    let (mut st, _) = resumed(sce_json, shifts, tape_of, admitted_slot, since)?;
    let idle_slots = now_slot.saturating_sub(since);

    // The span itself, as a shift with no steps in it — which is what happened.
    let replay = vitals_replay::shift(&mut st, &[], idle_slots);
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
    Ok(run_hash)
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
    let code = err.split("custom program error: 0x").nth(1)?;
    let code: String = code.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    Some(match u32::from_str_radix(&code, 16).ok()? {
        16 => "this chart moved while you were at the bedside: somebody else anchored a shift on the head \
               you were extending. Your work is still on your tape — open her again and it will be \
               played on the patient as she is now",
        17 => "someone is already in the room with her. A shift is held until it is anchored or \
               its time runs out, and then the head is free for anybody",
        18 => "this patient has left the ward — a stay that ended is not one anybody can add to",
        19 => "the head is not yours to give back: somebody else holds this shift",
        _ => return None,
    })
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
pub fn receipt(
    sce_json: &str,
    rubric_json: Option<&str>,
    shifts: &[crate::ward::ShiftOnChain],
    this: &crate::ward::ShiftOnChain,
    tape_of: &dyn Fn(&str) -> Option<Vec<vitals_replay::Step>>,
    pack: &crate::ward::Pack,
    admitted_slot: u64,
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
    let (mut st, played) = resumed(sce_json, &before, tape_of, admitted_slot, this.slot)?;
    let r = vitals_replay::shift(&mut st, &tape, 0);

    let det = rubric_json.and_then(|rj| vitals_osce::det_for_run(sce_json, &tape, rj).ok());

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
        "judged": serde_json::Value::Null,
        "judged_omitted": "a judged score belongs to a finished case. This is one shift in the \
                           middle of her stay, and a number that cannot mean what a reader assumes \
                           is worse than no number",
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
pub fn find_shift(
    chain: &WardChain,
    store: &crate::store::Store,
    run_hash: &str,
) -> Result<Option<(u64, Vec<crate::ward::ShiftOnChain>, crate::ward::ShiftOnChain)>, String> {
    let patients = chain.patients()?;
    for refreshing in [false, true] {
        for p in &patients {
            let key = format!("p{}", p.patient_id);
            let mut seen: Seen = store.get(SHIFT_CACHE, &key).unwrap_or_default();
            if refreshing && chain.refresh(p.patient_id, &mut seen).is_ok() {
                let _ = store.put(SHIFT_CACHE, &key, &seen);
            }
            let all = seen.shifts();
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
