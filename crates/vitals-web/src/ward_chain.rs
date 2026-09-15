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

    crate::ward::ward_payload(&patients, &shifts, Some(as_of.saturating_sub(WEEK_SLOTS)), as_of, chain.source())
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
