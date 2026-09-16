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
    let patient_id = match Instruction::deserialize(&mut &data[..]).ok()? {
        Instruction::AnchorShift { patient_id, .. } => patient_id,
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
    Some(ShiftOnChain { patient_id, signer: player.to_bytes(), slot })
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

    pub fn shifts(&self) -> Vec<ShiftOnChain> {
        self.shifts.iter().map(|s| s.shift).collect()
    }

    /// Take a page of history, newest first, and keep what is new. Returns how many were new.
    ///
    /// Idempotent by signature, because every reason this is called twice is ordinary: a retry, a
    /// restart, two instances ticking at once, a page that overlaps the last one.
    pub fn absorb(&mut self, page: Vec<SeenShift>) -> usize {
        let mut added = 0;
        for entry in page {
            if entry.shift.slot > self.until_slot || self.until.is_none() {
                self.until_slot = entry.shift.slot;
                self.until = Some(entry.signature.clone());
            }
            if self.shifts.iter().any(|s| s.signature == entry.signature) {
                continue;
            }
            self.shifts.push(entry);
            added += 1;
        }
        self.shifts.sort_by_key(|s| s.shift.slot);
        added
    }
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
        let until = seen
            .until()
            .and_then(|s| Signature::from_str(&s).ok());
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

        let mut page = Vec::new();
        for s in sigs {
            if s.err.is_some() {
                continue;
            }
            let Ok(sig) = Signature::from_str(&s.signature) else { continue };
            let tx = self
                .rpc
                .get_transaction(&sig, UiTransactionEncoding::Base64)
                .map_err(why)?;
            let slot = tx.slot;
            let Some(decoded) = tx.transaction.transaction.decode() else { continue };
            let keys = decoded.message.static_account_keys().to_vec();
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
                if let Some(shift) = shift_in(program, &self.program_id, &ix.data, &accounts, slot) {
                    page.push(SeenShift { signature: s.signature.clone(), shift });
                }
            }
        }
        Ok(seen.absorb(page))
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
    v["queue"] = serde_json::json!({
        "waiting": queue_depth(store),
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
            "{} is not a case this ward serves — queueing her would put a patient on the board \
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
        if state == "dead" {
            return Err("no picture of a dead patient is made — the board shows her last living \
                        state and says died in words"
                .into());
        }
        if !crate::ward::PORTRAIT_LADDER.contains(&state.as_str()) {
            return Err(format!(
                "{state} is not a state the engine reports, so nothing would ever draw it. The \
                 keys are {:?}",
                crate::ward::PORTRAIT_LADDER
            ));
        }
        if !is_portrait_url(src) {
            return Err(format!(
                "a portrait must be {PORTRAITS}/<sha256>.webp, not {src} — the board renders this \
                 as an image on a page strangers open, so the ward publishes one shape and \
                 refuses every other"
            ));
        }
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
    let Some(name) = src.strip_prefix(PORTRAITS).and_then(|r| r.strip_prefix('/')) else {
        return false;
    };
    let Some(sha) = name.strip_suffix(".webp") else { return false };
    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
    pub depth: usize,
}

/// How many packs are waiting for a bed.
pub fn queue_depth(store: &crate::store::Store) -> usize {
    store.keys(QUEUE_STORE).len()
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
    out.depth = queue_depth(store);
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
    if case.starts_with("osce-") {
        root.join("demo/stations").join(format!("{case}.sce.json"))
    } else {
        root.join("demo/scenarios").join(format!("{case}.json"))
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
    pub depth: usize,
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
    let packs = packs(store);
    let mut on_ward_cases: Vec<String> = patients
        .iter()
        .filter(|p| p.state == OPEN)
        .filter_map(|p| packs.get(&p.patient_id).map(|k| k.case.clone()))
        .collect();

    out.open = patients.iter().filter(|p| p.state == OPEN).count();
    out.depth = queue_depth(store);

    for _ in 0..to_admit(out.open, BEDS, out.depth) {
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
                out.notes.push(format!("{} has no scenario here, so she was dropped: {e}", pack.case));
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
                    "admitting {patient_id} failed, and her pack is spent: {e}"
                ));
                break;
            }
        }
    }

    out.depth = queue_depth(store);
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
        if state == "dead" {
            out.rejected.push(
                "no picture of a dead patient is made — the board shows her last living state"
                    .into(),
            );
            continue;
        }
        if !crate::ward::PORTRAIT_LADDER.contains(&state.as_str()) {
            out.rejected.push(format!("{state} is not a state the engine reports"));
            continue;
        }
        if !is_portrait_url(&src) {
            out.rejected.push(format!("{state}: a portrait must be {PORTRAITS}/<sha256>.webp"));
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
/// **The chain holds `run_hash`, not the tape.** These bytes are the off-chain half, and the
/// honest sentence about them is in CWF_PLAN.md: no server can change her past without every
/// browser noticing, because the hash on chain is of exactly these steps. A tape that went missing
/// would not let us rewrite her — it would stop her being rebuildable at all, which is why the
/// shift receipt offers every tape for download and why a mirror is worth having.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredTape {
    pub patient_id: u64,
    /// Its place in her chain, from zero. The order shifts are replayed in.
    pub index: u32,
    /// The slot the shift's lease was taken at — the end of the gap before it.
    pub taken_slot: u64,
    /// The slot the shift anchored at — the start of the gap after it.
    pub anchored_slot: u64,
    pub steps: Vec<vitals_replay::Step>,
}

/// Rebuild the patient as she is now: every shift before this one, and the time since.
///
/// One loop, and every step of it is something a stranger can repeat from the chain and the tapes:
/// the first shift runs from the scenario's start, each later shift runs after the idle time its
/// own gap bought ([`vitals_replay::idle_seconds`]), and the time since the last anchor is applied
/// last so the patient a shift opens on is the patient at *this* slot rather than at the moment
/// the previous stranger left.
///
/// `now_slot` is the chain's clock, not ours. Everything here is a pure function of (tapes, slot
/// numbers), which is what lets a browser derive the same patient we did.
pub fn resumed(
    sce_json: &str,
    tapes: &[StoredTape],
    now_slot: u64,
) -> Result<(vitals_sce::runtime::SceState, usize), String> {
    let mut ordered: Vec<&StoredTape> = tapes.iter().collect();
    ordered.sort_by_key(|t| t.index);

    let first = ordered.first().map(|t| t.steps.clone()).unwrap_or_default();
    let (mut st, _) = vitals_replay::resume(sce_json, &first)?;

    let mut last_anchor = ordered.first().map(|t| t.anchored_slot).unwrap_or(now_slot);
    for tape in ordered.iter().skip(1) {
        let gap = tape.taken_slot.saturating_sub(last_anchor);
        vitals_replay::shift(&mut st, &tape.steps, gap);
        last_anchor = tape.anchored_slot;
    }

    // What has happened to her since the last stranger left: nothing anybody did, and time.
    vitals_replay::pass_idle(&mut st, vitals_replay::idle_seconds(now_slot.saturating_sub(last_anchor)));
    Ok((st, ordered.len()))
}

/// Every tape this patient has, in the order they were played.
pub fn tapes_of(store: &crate::store::Store, patient_id: u64) -> Vec<StoredTape> {
    let mut out: Vec<StoredTape> = store
        .list::<StoredTape>(TAPE_STORE)
        .into_iter()
        .map(|(_, t)| t)
        .filter(|t| t.patient_id == patient_id)
        .collect();
    out.sort_by_key(|t| t.index);
    out
}

/// Keep a finished shift's tape. Keyed by patient and index, so replaying a shift cannot append a
/// second copy of it — the same failure the queue's content addressing prevents, one layer down.
pub fn keep_tape(store: &crate::store::Store, tape: &StoredTape) -> Result<(), String> {
    store
        .put(TAPE_STORE, &format!("p{}s{}", tape.patient_id, tape.index), tape)
        .map_err(|e| format!("her tape could not be kept, so the shift is unrebuildable: {e}"))
}
