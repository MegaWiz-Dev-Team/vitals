//! The Vitals program.
//!
//! Three instructions, and between them the whole claim:
//!
//! 1. [`Instruction::AnchorReplay`] — a verifier appends one finished run to the tree.
//! 2. [`Instruction::ProveAttempt`] — a player proves an anchored run is theirs, one per
//!    transaction, because a Merkle path does not fit in a transaction alongside fifteen others.
//! 3. [`Instruction::ClaimProgress`] — the player claims a level, and the program **recomputes
//!    it** from what has been proven and writes only what its own arithmetic agrees with.
//!
//! There is no issuer key anywhere in step 3. The verdict is a pure function of proven attempts,
//! so anyone can predict it and nobody can lean on it.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    clock::Clock,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};
use vitals_progress::merkle::{self, Tree, DEPTH};
use vitals_progress::record::{AttemptRecord, Outcome};
use vitals_progress::{adjudicate, Attempt, Difficulty, Dreyfus, Verdict};

pub const SEED_ACCOUNT: &[u8] = b"acct";
pub const SEED_TREE: &[u8] = b"tree";
/// One open commitment per player. Seeded on the player, so a commitment cannot be made on
/// somebody else's behalf and cannot be confused with theirs.
pub const SEED_COMMIT: &[u8] = b"commit";
pub const SEED_CLAIM: &[u8] = b"claim";
pub const SEED_PROGRESS: &[u8] = b"prog";

/// How many proven attempts one claim can carry. Bounded so the account is fixed-size and the
/// recomputation cost is bounded; a player with more history claims in batches.
pub const CLAIM_CAPACITY: usize = 16;

/// How many devices one person can play from.
pub const MAX_AUTHORITIES: usize = 8;
pub const ACCOUNT_LEN: usize = 32 + 4 + MAX_AUTHORITIES * 32;

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq)]
pub struct RecordWire {
    pub player: [u8; 32],
    pub sce_hash: [u8; 32],
    pub case: [u8; 32],
    pub run_hash: [u8; 32],
    /// 0 student · 1 intern · 2 resident
    pub difficulty: u8,
    pub exam_mode: bool,
    /// 0 none · 1 win_discharge · 2 win_icu · 3 death_biphasic · 4 death_arrest
    pub outcome: u8,
    pub harm_count: u16,

    // ── vt02 ────────────────────────────────────────────────────────────────
    // Note what is *not* here: `commitment` and `committed_slot`. Those are read from the
    // commitment account and nowhere else. A field the caller fills in proves nothing — it records
    // what the caller asserted — and leaving them off the wire makes supplying them impossible
    // rather than merely forbidden.
    //
    // The score fields are accepted from the caller, and the two halves earn that differently.
    // `det_score` is re-derivable: `run_hash` binds the tape, so anyone can replay it against the
    // pinned engine and recompute it — a lie is detectable by exactly the mechanism the product
    // is about. `judged_score` is NOT re-derivable; it is meant to be verifier-attested, and the
    // attestation mechanism does not exist yet, so today it is a self-asserted number in a
    // permanent record. Nothing on chain consumes it (claims recompute from the outcome), so it
    // cannot buy progression — but do not build anything on it until attestation exists.
    pub rubric_hash: [u8; 32],
    pub det_score: u16,
    pub det_max: u16,
    pub judged_score: u16,
    pub judged_max: u16,
}

impl RecordWire {
    /// `commitment` and `committed_slot` come from the account, not from `self`.
    ///
    /// They are arguments rather than fields for the reason above: the caller cannot supply them,
    /// so the leaf cannot claim a commitment that was never made.
    fn decode(&self, commitment: [u8; 32], committed_slot: u64) -> Result<AttemptRecord, ProgramError> {
        Ok(AttemptRecord {
            player: self.player,
            sce_hash: self.sce_hash,
            case: self.case,
            run_hash: self.run_hash,
            difficulty: match self.difficulty {
                0 => Difficulty::Student,
                1 => Difficulty::Intern,
                2 => Difficulty::Resident,
                _ => return Err(VitalsError::BadRecord.into()),
            },
            exam_mode: self.exam_mode,
            commitment,
            committed_slot,
            rubric_hash: self.rubric_hash,
            det_score: self.det_score,
            det_max: self.det_max,
            judged_score: self.judged_score,
            judged_max: self.judged_max,
            outcome: Outcome::from_u8(self.outcome).ok_or(VitalsError::BadRecord)?,
            harm_count: self.harm_count,
        })
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum Instruction {
    /// Open a person. The signer becomes the id and the first device.
    OpenAccount,
    /// Let another device act as this person. Signed by a device that already can.
    AddAuthority { device: [u8; 32] },
    /// Stop a device from acting as this person — a lost laptop, a borrowed machine.
    RemoveAuthority { device: [u8; 32] },
    AnchorReplay { tree_id: u64, record: RecordWire },
    /// `commitment` and `committed_slot` are arguments here and read from the account at anchor
    /// time, and the asymmetry is deliberate.
    ///
    /// Nothing validates them when a leaf is created, so anchoring must not trust the caller.
    /// Proving is the opposite: the record has to hash to a leaf that is already in the tree, so a
    /// wrong commitment produces a wrong leaf and the Merkle check rejects it. The proof is the
    /// validation. Requiring the account instead would be worse than useless — it is consumed on
    /// anchor and may be closed for its rent, so an honest prover would have nothing to read.
    ProveAttempt {
        tree_id: u64,
        record: RecordWire,
        index: u64,
        path: Vec<[u8; 32]>,
        commitment: [u8; 32],
        committed_slot: u64,
    },
    ClaimProgress { tree_id: u64, specialty: u8, claimed: u8 },
    /// Declare, before the run, which case is about to be attempted.
    ///
    /// `hash` is `hash(case ‖ player ‖ nonce)`: it names the case without revealing it, so the
    /// commitment can be checked afterwards without telling anyone watching the chain which
    /// station is being attempted.
    Commit { hash: [u8; 32] },
    /// Clear an open commitment that will not be used.
    ///
    /// No lamports move, and none can: the count of commitments ever made lives in this same
    /// account, and the account must stay rent-exempt to keep holding it. The count is the whole
    /// mechanism — without it, a learner could commit five times, play five, anchor the good one
    /// and clear the other four, leaving a chain that says they attempted once. So the account is
    /// permanent, its ~0.0012 SOL is the one-time price of the counter (the relay pays it, and it
    /// is noise next to the per-player rent already paid), and closing only frees the open slot
    /// so a new commitment can be made cleanly.
    CloseCommitment,
}

/// Trees are addressed by id rather than being one global tree.
///
/// A fixed-depth tree holds 4,096 leaves, so a live deployment rolls to a new id when one fills
/// — per cohort, per season, per scenario, whatever the operator chooses. It also means a
/// verifier can retire a tree without touching the anchored history in it.
pub fn tree_seeds(tree_id: u64) -> [u8; 8] {
    tree_id.to_le_bytes()
}

/// Where an operator's anchoring tree lives.
///
/// Seeded on the operator as well as the id, and both halves are load-bearing. `tree_id` is the
/// slot the server started in — a *global* number, so two servers booting in the same ~400 ms
/// window choose the same one and, while the address came from that number alone, addressed the
/// same account. Observed live: two servers on one machine both reported `tree #413`.
///
/// The accident was the smaller half of it. Nothing tied a tree to whoever created it, so any
/// funder could pass any `tree_id` and append a leaf into a stranger's tree — no collision
/// required, just the number, which the server prints. Deriving from the operator's key makes a
/// foreign tree unaddressable rather than merely unlikely to be hit: a different signer computes
/// a different account and cannot reach yours at all.
///
/// The cost, stated plainly: a tree belongs to the key that created it, so rotating the relay key
/// starts a new tree. Old trees stay on chain and old proofs keep verifying against them.
pub fn tree_pda(program_id: &Pubkey, operator: &Pubkey, tree_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[SEED_TREE, operator.as_ref(), &tree_id.to_le_bytes()],
        program_id,
    )
}

// ── accounts ────────────────────────────────────────────────────────────────

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy)]
pub struct TreeAccount {
    pub root: [u8; 32],
    pub next_index: u64,
    pub filled: [[u8; 32]; DEPTH],
}

/// A person, as distinct from a device.
///
/// Identity used to *be* a keypair: the progress account was derived straight from whichever key
/// the browser had made, so a second machine was a second person and clearing site data was
/// death. Here the keys are a list, and the person is the list's owner.
///
/// `id` is the first device's public key. That choice is what makes this free to adopt — the
/// seeds `["prog", id, specialty]` are byte-for-byte what `["prog", device, specialty]` produced
/// before, so every record anchored under the old scheme is already at the right address.
/// An open commitment, plus the count that survives it.
///
/// `started` is the load-bearing field. The claim this whole mechanism makes is *"the chain
/// already knows how many times you started"*, and a commitment that could be closed for its rent
/// would let a learner erase the attempts they did not like — commit five, play five, anchor the
/// good one, close the rest. Rent is refundable; the count is not, so it lives here and only ever
/// goes up.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct Commitment {
    /// hash(case ‖ player ‖ nonce). Zero when no commitment is open.
    pub hash: [u8; 32],
    /// The slot the open commitment was made at.
    pub slot: u64,
    /// How many commitments this player has ever made. Monotonic.
    pub started: u64,
    /// Whether `hash`/`slot` describe an open commitment or a spent one.
    pub open: bool,
}

pub const COMMITMENT_LEN: usize = 32 + 8 + 8 + 1;

pub fn commitment_pda(program_id: &Pubkey, player: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEED_COMMIT, player], program_id)
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct Account {
    pub id: [u8; 32],
    /// Every key allowed to act as this person. Never empty — removing the last one would leave a
    /// record nobody can ever claim again.
    pub authorities: Vec<[u8; 32]>,
}

impl Account {
    pub fn allows(&self, key: &Pubkey) -> bool {
        let k = key.to_bytes();
        self.authorities.contains(&k)
    }
}

pub const TREE_LEN: usize = 32 + 8 + 32 * DEPTH;

impl TreeAccount {
    fn to_tree(self) -> Tree {
        Tree { root: self.root, next_index: self.next_index, filled: self.filled }
    }
    fn from_tree(t: Tree) -> Self {
        TreeAccount { root: t.root, next_index: t.next_index, filled: t.filled }
    }
    fn empty() -> Self {
        Self::from_tree(Tree::new())
    }
}

/// One proven attempt, as the claim buffer holds it. The leaf is kept so a run proven twice
/// cannot be counted twice.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct ProvenAttempt {
    pub leaf: [u8; 32],
    pub case: [u8; 32],
    /// The outcome score (out of `max`) — what `summarize`/level is computed from.
    pub score: u32,
    pub max: u32,
    /// The deterministic rubric score (out of `det_max`) — what a star is measured on. Kept in the
    /// buffer, not only the leaf, so the star tally is re-derivable from the claim account alone.
    pub det_score: u16,
    pub det_max: u16,
    pub difficulty: u8,
    pub exam_mode: bool,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ClaimAccount {
    pub player: [u8; 32],
    pub count: u8,
    pub attempts: Vec<ProvenAttempt>,
}

pub const CLAIM_LEN: usize = 32 + 1 + 4 + CLAIM_CAPACITY * (32 + 32 + 4 + 4 + 2 + 2 + 1 + 1);

#[derive(BorshSerialize, BorshDeserialize, Debug, Default)]
pub struct Progress {
    pub player: [u8; 32],
    pub specialty: u8,
    /// The level the program computed. Never the level that was claimed.
    pub level: u8,
    pub distinct_cases: u32,
    pub attempts_counted: u32,
    pub xp: i64,
}

pub const PROGRESS_LEN: usize = 32 + 1 + 1 + 4 + 4 + 8;

// ── errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum VitalsError {
    /// The claim did not survive recomputation. A success of the design, not a bug.
    ClaimNotEarned = 0,
    BadRecord = 1,
    WrongPda = 2,
    NoAttempts = 3,
    /// The leaf is not in the tree, or not at the index claimed.
    ProofFailed = 4,
    ClaimFull = 5,
    /// A run already counted toward this claim.
    DuplicateAttempt = 6,
    TreeFull = 7,
    /// The record names a different player than the signer.
    NotYourRun = 8,
    /// The account is at the right address but is not ours.
    WrongOwner = 9,
    /// The signer is not one of this account's devices.
    NotAuthorized = 10,
    AuthoritiesFull = 11,
    AlreadyAuthorized = 12,
    /// Removing the last device would strand the record forever.
    LastAuthority = 13,
    NoAccount = 14,
    /// Anchoring without having declared the attempt first.
    NoCommitment = 15,
}

impl From<VitalsError> for ProgramError {
    fn from(e: VitalsError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

entrypoint!(process_instruction);

pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match Instruction::try_from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)? {
        Instruction::OpenAccount => open_account(program_id, accounts),
        Instruction::AddAuthority { device } => authority(program_id, accounts, device, true),
        Instruction::RemoveAuthority { device } => authority(program_id, accounts, device, false),
        Instruction::AnchorReplay { tree_id, record } => {
            anchor_replay(program_id, accounts, tree_id, record)
        }
        Instruction::ProveAttempt { tree_id, record, index, path, commitment, committed_slot } => {
            prove_attempt(program_id, accounts, tree_id, record, index, path, commitment, committed_slot)
        }
        Instruction::Commit { hash } => commit(program_id, accounts, hash),
        Instruction::CloseCommitment => close_commitment(program_id, accounts),
        Instruction::ClaimProgress { tree_id, specialty, claimed } => {
            claim_progress(program_id, accounts, tree_id, specialty, claimed)
        }
    }
}

// ── 0. who you are ──────────────────────────────────────────────────────────

/// Read an account PDA and check the signer is one of its devices.
///
/// Everything downstream is seeded on `account.id` rather than on whoever signed, which is the
/// whole point: the record belongs to the person, and the person is allowed to own more than one
/// machine.
fn authorised<'a>(
    program_id: &Pubkey,
    account_ai: &AccountInfo<'a>,
    signer: &AccountInfo<'a>,
) -> Result<Account, ProgramError> {
    if account_ai.data_is_empty() {
        return Err(VitalsError::NoAccount.into());
    }
    owned_by(account_ai, program_id)?;
    let account = read::<Account>(account_ai)?;
    let (pda, _) = Pubkey::find_program_address(&[SEED_ACCOUNT, &account.id], program_id);
    if pda != *account_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    if !account.allows(signer.key) {
        return Err(VitalsError::NotAuthorized.into());
    }
    Ok(account)
}

fn open_account(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let it = &mut accounts.iter();
    let funder = next_account_info(it)?;
    let device = next_account_info(it)?;
    let account_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !funder.is_signer || !device.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // The id is the opening device's own key, so nobody can open an account in someone else's
    // name and nobody has to be assigned one.
    let id = device.key.to_bytes();
    let (pda, bump) = Pubkey::find_program_address(&[SEED_ACCOUNT, &id], program_id);
    if pda != *account_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    if !account_ai.data_is_empty() {
        // Already open. Opening twice is what a second tab does, not an attack.
        return Ok(());
    }
    create_pda(funder, account_ai, system, program_id, ACCOUNT_LEN, &[SEED_ACCOUNT, &id, &[bump]])?;
    write(account_ai, &Account { id, authorities: vec![id] })?;
    msg!("account opened");
    Ok(())
}

/// Add or remove a device. `add` false removes.
fn authority(program_id: &Pubkey, accounts: &[AccountInfo], device: [u8; 32], add: bool) -> ProgramResult {
    let it = &mut accounts.iter();
    let _funder = next_account_info(it)?;
    let signer = next_account_info(it)?;
    let account_ai = next_account_info(it)?;

    if !signer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut account = authorised(program_id, account_ai, signer)?;

    if add {
        if account.authorities.contains(&device) {
            return Err(VitalsError::AlreadyAuthorized.into());
        }
        if account.authorities.len() >= MAX_AUTHORITIES {
            return Err(VitalsError::AuthoritiesFull.into());
        }
        account.authorities.push(device);
        msg!("device added — {} of {}", account.authorities.len(), MAX_AUTHORITIES);
    } else {
        if account.authorities.len() <= 1 {
            // The record would still exist and nobody could ever claim against it again.
            return Err(VitalsError::LastAuthority.into());
        }
        let before = account.authorities.len();
        account.authorities.retain(|a| *a != device);
        if account.authorities.len() == before {
            return Err(VitalsError::NotAuthorized.into());
        }
        msg!("device removed — {} left", account.authorities.len());
    }
    write(account_ai, &account)
}

// ── 1. anchor ───────────────────────────────────────────────────────────────

/// Declare, before playing, which case is about to be attempted.
///
/// Creates the player's commitment account on first use and increments `started` every time. The
/// counter is what the claim rests on, so it is written before anything can go wrong afterwards.
fn commit(program_id: &Pubkey, accounts: &[AccountInfo], hash: [u8; 32]) -> ProgramResult {
    let it = &mut accounts.iter();
    let funder = next_account_info(it)?;
    let device = next_account_info(it)?;
    let account_ai = next_account_info(it)?;
    let commit_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !funder.is_signer || !device.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let account = authorised(program_id, account_ai, device)?;

    // All-zero is the sentinel for "no commitment open" — in this account and in the record's
    // placeholder era. Accepting it as a real commitment would make a vt02 leaf ambiguous about
    // the one thing the field exists to make unambiguous.
    if hash == [0u8; 32] {
        return Err(VitalsError::BadRecord.into());
    }

    let (pda, bump) = commitment_pda(program_id, &account.id);
    if pda != *commit_ai.key {
        return Err(VitalsError::WrongPda.into());
    }

    let mut c = if commit_ai.data_is_empty() {
        create_pda(funder, commit_ai, system, program_id, COMMITMENT_LEN,
                   &[SEED_COMMIT, account.id.as_ref(), &[bump]])?;
        Commitment { hash: [0; 32], slot: 0, started: 0, open: false }
    } else {
        owned_by(commit_ai, program_id)?;
        read::<Commitment>(commit_ai)?
    };

    // Overwriting an open commitment is allowed — a learner who walks away mid-case should not be
    // locked out — but it still counts. That is the whole point of a monotonic counter: every
    // declaration of intent is on the record, whether or not it turned into an anchored run.
    c.hash = hash;
    c.slot = Clock::get()?.slot;
    c.started = c.started.saturating_add(1);
    c.open = true;
    write(commit_ai, &c)
}

/// Clear the open commitment, keeping the count — and keeping the rent, necessarily.
///
/// `started` is deliberately untouched: a learner who could close their way back to zero could
/// commit five times, play five, anchor the flattering one and erase the rest. And because the
/// counter lives in this account, the lamports stay too — refunding them would delete the one
/// number that must survive.
fn close_commitment(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let it = &mut accounts.iter();
    let funder = next_account_info(it)?;
    let device = next_account_info(it)?;
    let account_ai = next_account_info(it)?;
    let commit_ai = next_account_info(it)?;

    if !funder.is_signer || !device.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let account = authorised(program_id, account_ai, device)?;
    let (pda, _) = commitment_pda(program_id, &account.id);
    if pda != *commit_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    owned_by(commit_ai, program_id)?;

    let mut c = read::<Commitment>(commit_ai)?;
    c.hash = [0; 32];
    c.slot = 0;
    c.open = false;
    write(commit_ai, &c)?;

    // The account stays, holding only the count. Reclaiming its lamports entirely would take the
    // count with it, which is the thing that must not be reclaimable.
    Ok(())
}

fn anchor_replay(program_id: &Pubkey, accounts: &[AccountInfo], tree_id: u64, wire: RecordWire) -> ProgramResult {
    let it = &mut accounts.iter();
    let funder = next_account_info(it)?;
    let device = next_account_info(it)?;
    let account_ai = next_account_info(it)?;
    let tree_ai = next_account_info(it)?;
    let commit_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    // Two signers with different jobs. `funder` has the lamports and pays rent; `device` is a
    // machine the player owns and needs no balance at all. Collapsing them into one account is
    // what made every player on a server share the server's identity — and its progress.
    if !funder.is_signer || !device.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let account = authorised(program_id, account_ai, device)?;

    let id = tree_seeds(tree_id);
    let (pda, bump) = tree_pda(program_id, funder.key, tree_id);
    if pda != *tree_ai.key {
        return Err(VitalsError::WrongPda.into());
    }

    if tree_ai.data_is_empty() {
        create_pda(funder, tree_ai, system, program_id, TREE_LEN,
                   &[SEED_TREE, funder.key.as_ref(), &id, &[bump]])?;
        write(tree_ai, &TreeAccount::empty())?;
    }

    // The commitment comes from the account and from nowhere else. This is the reason the wire
    // does not carry it: at anchor time nothing else can check it, so a value the caller supplied
    // would record only what the caller asserted. Consumed here — one commitment, one anchor —
    // so the same declaration cannot cover a second attempt.
    let (cpda, _) = commitment_pda(program_id, &account.id);
    if cpda != *commit_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    if commit_ai.data_is_empty() {
        return Err(VitalsError::NoCommitment.into());
    }
    owned_by(commit_ai, program_id)?;
    let mut c = read::<Commitment>(commit_ai)?;
    if !c.open {
        return Err(VitalsError::NoCommitment.into());
    }
    let (commitment, committed_slot) = (c.hash, c.slot);
    c.open = false;
    c.hash = [0; 32];
    write(commit_ai, &c)?;

    let record = wire.decode(commitment, committed_slot)?;
    // Anchoring is not open to bystanders. Without this anyone could fill somebody else's tree
    // with leaves naming other players — they could never claim them, but a full tree is a tree
    // that has to be rolled, and that is a denial of service somebody else pays for.
    if record.player != account.id {
        return Err(VitalsError::NotYourRun.into());
    }
    let leaf = record.leaf();

    owned_by(tree_ai, program_id)?;
    let mut tree = read::<TreeAccount>(tree_ai)?.to_tree();
    let index = tree.append(leaf).ok_or(VitalsError::TreeFull)?;
    write(tree_ai, &TreeAccount::from_tree(tree))?;

    msg!("anchored leaf at index {} — score {} of {}", index, record.score(), record.max_score());
    Ok(())
}

// ── 2. prove ────────────────────────────────────────────────────────────────

// Eight arguments: the standard four every handler takes, plus the four facts a proof is — the
// record, where it sits, the path to it, and the declaration it answers. Bundling them would
// invent a struct that exists for one call from one dispatcher.
#[allow(clippy::too_many_arguments)]
fn prove_attempt(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    tree_id: u64,
    wire: RecordWire,
    index: u64,
    path: Vec<[u8; 32]>,
    commitment: [u8; 32],
    committed_slot: u64,
) -> ProgramResult {
    let it = &mut accounts.iter();
    let funder = next_account_info(it)?;
    let device = next_account_info(it)?;
    let account_ai = next_account_info(it)?;
    let tree_ai = next_account_info(it)?;
    let claim_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !funder.is_signer || !device.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let account = authorised(program_id, account_ai, device)?;

    let record = wire.decode(commitment, committed_slot)?;
    // The record names its person, so an anchored run cannot be claimed by a bystander — and can
    // be claimed from any machine that person plays on.
    if record.player != account.id {
        return Err(VitalsError::NotYourRun.into());
    }

    let id = tree_seeds(tree_id);
    let (tree_key, _) = tree_pda(program_id, funder.key, tree_id);
    if tree_key != *tree_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    owned_by(tree_ai, program_id)?;
    let tree = read::<TreeAccount>(tree_ai)?;

    let leaf = record.leaf();
    let recomputed = merkle::root_from_proof(leaf, index, &path).ok_or(VitalsError::ProofFailed)?;
    if recomputed != tree.root {
        msg!("proof failed: leaf is not in the tree at index {}", index);
        return Err(VitalsError::ProofFailed.into());
    }

    // The claim buffer is per tree, so proofs from a retired tree cannot be mixed into a claim
    // against a live one.
    let (claim_pda, bump) =
        Pubkey::find_program_address(&[SEED_CLAIM, account.id.as_ref(), &id], program_id);
    if claim_pda != *claim_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    if claim_ai.data_is_empty() {
        create_pda(funder, claim_ai, system, program_id, CLAIM_LEN,
            &[SEED_CLAIM, account.id.as_ref(), &id, &[bump]])?;
        write(claim_ai, &ClaimAccount { player: account.id, count: 0, attempts: Vec::new() })?;
    }

    owned_by(claim_ai, program_id)?;
    let mut claim = read::<ClaimAccount>(claim_ai)?;
    if claim.attempts.iter().any(|a| a.leaf == leaf) {
        return Err(VitalsError::DuplicateAttempt.into());
    }
    if claim.attempts.len() >= CLAIM_CAPACITY {
        return Err(VitalsError::ClaimFull.into());
    }

    claim.attempts.push(ProvenAttempt {
        leaf,
        case: record.case,
        score: record.score(),
        max: record.max_score(),
        // The det score already arrived in the record to rebuild the leaf — persist it here rather
        // than discard it, so the star tally can read it from the claim buffer.
        det_score: record.det_score,
        det_max: record.det_max,
        difficulty: wire.difficulty,
        exam_mode: record.exam_mode,
    });
    claim.count = claim.attempts.len() as u8;
    write(claim_ai, &claim)?;

    msg!("proved attempt {} of {} — score {}", claim.count, CLAIM_CAPACITY, record.score());
    Ok(())
}

// ── 3. claim ────────────────────────────────────────────────────────────────

fn claim_progress(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    tree_id: u64,
    specialty: u8,
    claimed: u8,
) -> ProgramResult {
    let it = &mut accounts.iter();
    let funder = next_account_info(it)?;
    let device = next_account_info(it)?;
    let account_ai = next_account_info(it)?;
    let claim_ai = next_account_info(it)?;
    let progress_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !funder.is_signer || !device.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let account = authorised(program_id, account_ai, device)?;
    let claimed = dreyfus_from_u8(claimed)?;

    let id = tree_seeds(tree_id);
    let (claim_pda, _) =
        Pubkey::find_program_address(&[SEED_CLAIM, account.id.as_ref(), &id], program_id);
    if claim_pda != *claim_ai.key || claim_ai.data_is_empty() {
        return Err(VitalsError::NoAttempts.into());
    }
    owned_by(claim_ai, program_id)?;
    let claim = read::<ClaimAccount>(claim_ai)?;
    if claim.attempts.is_empty() {
        return Err(VitalsError::NoAttempts.into());
    }

    // Every attempt here survived a Merkle proof against the tree root. Nothing else is trusted.
    let attempts: Vec<Attempt> = claim
        .attempts
        .iter()
        .map(|a| Attempt {
            case: a.case,
            score: a.score,
            max: a.max,
            det_score: a.det_score,
            det_max: a.det_max,
            difficulty: match a.difficulty {
                1 => Difficulty::Intern,
                2 => Difficulty::Resident,
                _ => Difficulty::Student,
            },
            exam_mode: a.exam_mode,
        })
        .collect();

    let summary = vitals_progress::summarize(&attempts);
    match adjudicate(claimed, &attempts) {
        Verdict::Rejected { claimed, computed } => {
            msg!(
                "claim rejected: claimed {}, computed {} from {} proven attempts ({} distinct, avg {}bps)",
                claimed.as_str(), computed.as_str(), summary.attempts, summary.distinct_cases, summary.avg_bps
            );
            Err(VitalsError::ClaimNotEarned.into())
        }
        Verdict::Granted(level) => {
            msg!(
                "claim granted: {} ({} proven, {} distinct, {} hard, avg {}bps, xp {})",
                level.as_str(), summary.attempts, summary.distinct_cases, summary.hard_cases,
                summary.avg_bps, summary.xp
            );

            let (pda, bump) = Pubkey::find_program_address(
                &[SEED_PROGRESS, account.id.as_ref(), &[specialty]], program_id);
            if pda != *progress_ai.key {
                return Err(VitalsError::WrongPda.into());
            }
            if progress_ai.data_is_empty() {
                create_pda(funder, progress_ai, system, program_id, PROGRESS_LEN,
                    &[SEED_PROGRESS, account.id.as_ref(), &[specialty], &[bump]])?;
            }
            write(progress_ai, &Progress {
                player: account.id,
                specialty,
                level: level as u8,
                distinct_cases: summary.distinct_cases,
                attempts_counted: summary.attempts,
                xp: summary.xp,
            })
        }
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    target: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    program_id: &Pubkey,
    len: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    let rent = Rent::get()?.minimum_balance(len);
    invoke_signed(
        &system_instruction::create_account(payer.key, target.key, rent, len as u64, program_id),
        &[payer.clone(), target.clone(), system.clone()],
        &[seeds],
    )
}

/// Deserialize the prefix of an account.
///
/// Not `try_from_slice`: that insists the whole buffer is consumed, and an account sized for a
/// full claim buffer is mostly trailing zeros while it fills up. Reading a prefix is what we
/// actually mean.
/// Refuse to read an account this program does not own.
///
/// A derived PDA can only have been created by us, so in the current flow this is belt and
/// braces — but "the address is right" and "the data is ours" are different facts, and the second
/// one is the one deserialisation depends on. Every account read goes through here.
fn owned_by(ai: &AccountInfo, program_id: &Pubkey) -> ProgramResult {
    if ai.owner != program_id {
        return Err(VitalsError::WrongOwner.into());
    }
    Ok(())
}

fn read<T: BorshDeserialize>(ai: &AccountInfo) -> Result<T, ProgramError> {
    let data = ai.data.borrow();
    let mut slice: &[u8] = &data;
    T::deserialize(&mut slice).map_err(|_| ProgramError::InvalidAccountData)
}

fn write<T: BorshSerialize>(ai: &AccountInfo, value: &T) -> ProgramResult {
    let mut data = ai.data.borrow_mut();
    // Zero first: Borsh writes a Vec shorter than the last one without clearing the tail, and a
    // stale tail deserialises as garbage on the next read.
    data.fill(0);
    let mut slice = &mut data[..];
    value.serialize(&mut slice).map_err(|_| ProgramError::AccountDataTooSmall)
}

fn dreyfus_from_u8(v: u8) -> Result<Dreyfus, ProgramError> {
    Ok(match v {
        0 => Dreyfus::Novice,
        1 => Dreyfus::AdvancedBeginner,
        2 => Dreyfus::Competent,
        3 => Dreyfus::Proficient,
        4 => Dreyfus::Expert,
        _ => return Err(ProgramError::InvalidInstructionData),
    })
}

#[cfg(test)]
mod tests {
    /// Two servers that start in the same slot must not share a tree.
    ///
    /// `tree_id` is the slot the server started in, and a slot is a global number: two operators
    /// booting within the same ~400 ms window pick the same one. While the address was derived
    /// from that number alone, they addressed the same account and appended into each other's
    /// tree — silently, and permanently, since the tree is the thing every Merkle proof is built
    /// against. Observed live: two servers on one machine both reported `tree #413`.
    ///
    /// This matters because the repository says out loud that it wants to be "a protocol with one
    /// reference client": other people running their own servers is the intended end state, not a
    /// hypothetical.
    #[test]
    fn two_operators_in_one_slot_do_not_share_a_tree() {
        let program = Pubkey::new_unique();
        let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
        let slot = 413;
        assert_ne!(tree_pda(&program, &a, slot).0, tree_pda(&program, &b, slot).0);
    }

    #[test]
    fn one_operator_still_gets_a_tree_per_id() {
        let program = Pubkey::new_unique();
        let me = Pubkey::new_unique();
        assert_ne!(tree_pda(&program, &me, 413).0, tree_pda(&program, &me, 414).0);
    }

    #[test]
    fn the_same_operator_and_id_is_always_the_same_tree() {
        // Resuming after a restart has to find the tree it was filling, or the server starts a
        // fresh one and can no longer prove anything it anchored before the restart.
        let program = Pubkey::new_unique();
        let me = Pubkey::new_unique();
        assert_eq!(tree_pda(&program, &me, 413).0, tree_pda(&program, &me, 413).0);
    }

    use super::*;

    /// The list is what makes a person more than a machine, so membership is the load-bearing
    /// check in the whole account layer.
    #[test]
    fn an_account_allows_exactly_the_keys_on_its_list() {
        let a = Pubkey::new_from_array([1; 32]);
        let b = Pubkey::new_from_array([2; 32]);
        let stranger = Pubkey::new_from_array([3; 32]);
        let acct = Account { id: a.to_bytes(), authorities: vec![a.to_bytes(), b.to_bytes()] };
        assert!(acct.allows(&a));
        assert!(acct.allows(&b));
        assert!(!acct.allows(&stranger));
    }

    #[test]
    fn an_account_with_no_devices_allows_nobody() {
        let acct = Account { id: [1; 32], authorities: vec![] };
        assert!(!acct.allows(&Pubkey::new_from_array([1; 32])),
                "being the id is not the same as being on the list");
    }

    /// The account has to fit the space reserved for it or writing the eighth device fails at
    /// runtime, on chain, for whoever happens to own eight machines.
    #[test]
    fn a_full_account_fits_in_its_allocation() {
        let acct = Account { id: [7; 32], authorities: vec![[9; 32]; MAX_AUTHORITIES] };
        let encoded = borsh::to_vec(&acct).expect("serialise");
        assert!(encoded.len() <= ACCOUNT_LEN,
                "{} bytes into {ACCOUNT_LEN}", encoded.len());
    }

    #[test]
    fn an_account_round_trips_through_borsh() {
        let acct = Account { id: [7; 32], authorities: vec![[1; 32], [2; 32]] };
        let back: Account =
            borsh::BorshDeserialize::deserialize(&mut &borsh::to_vec(&acct).unwrap()[..]).unwrap();
        assert_eq!(back.id, acct.id);
        assert_eq!(back.authorities, acct.authorities);
    }

    /// A partly-filled account is the normal case, and `try_from_slice` refuses trailing bytes —
    /// which is exactly the bug that broke reading claim accounts once already.
    #[test]
    fn a_partly_filled_account_decodes_from_a_full_length_buffer() {
        let acct = Account { id: [7; 32], authorities: vec![[1; 32]] };
        let mut buf = borsh::to_vec(&acct).unwrap();
        buf.resize(ACCOUNT_LEN, 0);
        let back: Account = borsh::BorshDeserialize::deserialize(&mut &buf[..])
            .expect("prefix decode, not try_from_slice");
        assert_eq!(back.authorities.len(), 1);
    }

    #[test]
    fn every_seed_is_distinct() {
        let seeds = [SEED_ACCOUNT, SEED_TREE, SEED_CLAIM, SEED_PROGRESS];
        for (i, a) in seeds.iter().enumerate() {
            for b in &seeds[i + 1..] {
                assert_ne!(a, b, "two kinds of account would share an address");
            }
        }
    }

    /// The property the whole migration rests on: an account whose id is its first device puts
    /// the progress record at the address the device-seeded scheme already used.
    #[test]
    fn moving_to_accounts_does_not_move_the_progress_address() {
        let pid = Pubkey::new_from_array([42; 32]);
        let device = Pubkey::new_from_array([13; 32]);
        let acct = Account { id: device.to_bytes(), authorities: vec![device.to_bytes()] };
        let by_account = Pubkey::find_program_address(&[SEED_PROGRESS, &acct.id, &[1]], &pid).0;
        let by_device = Pubkey::find_program_address(&[SEED_PROGRESS, device.as_ref(), &[1]], &pid).0;
        assert_eq!(by_account, by_device);
    }

    #[test]
    fn error_codes_are_stable() {
        // The web server translates these numbers into sentences for people. Renumbering them
        // silently would make every message wrong rather than missing.
        assert_eq!(VitalsError::ClaimNotEarned as u32, 0);
        assert_eq!(VitalsError::NotYourRun as u32, 8);
        assert_eq!(VitalsError::WrongOwner as u32, 9);
        assert_eq!(VitalsError::NotAuthorized as u32, 10);
        assert_eq!(VitalsError::LastAuthority as u32, 13);
        assert_eq!(VitalsError::NoAccount as u32, 14);
    }
}
