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

pub const SEED_TREE: &[u8] = b"tree";
pub const SEED_CLAIM: &[u8] = b"claim";
pub const SEED_PROGRESS: &[u8] = b"prog";

/// How many proven attempts one claim can carry. Bounded so the account is fixed-size and the
/// recomputation cost is bounded; a player with more history claims in batches.
pub const CLAIM_CAPACITY: usize = 16;

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
}

impl RecordWire {
    fn decode(&self) -> Result<AttemptRecord, ProgramError> {
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
            outcome: Outcome::from_u8(self.outcome).ok_or(VitalsError::BadRecord)?,
            harm_count: self.harm_count,
        })
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum Instruction {
    AnchorReplay { tree_id: u64, record: RecordWire },
    ProveAttempt { tree_id: u64, record: RecordWire, index: u64, path: Vec<[u8; 32]> },
    ClaimProgress { tree_id: u64, specialty: u8, claimed: u8 },
}

/// Trees are addressed by id rather than being one global tree.
///
/// A fixed-depth tree holds 4,096 leaves, so a live deployment rolls to a new id when one fills
/// — per cohort, per season, per scenario, whatever the operator chooses. It also means a
/// verifier can retire a tree without touching the anchored history in it.
pub fn tree_seeds(tree_id: u64) -> [u8; 8] {
    tree_id.to_le_bytes()
}

// ── accounts ────────────────────────────────────────────────────────────────

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy)]
pub struct TreeAccount {
    pub root: [u8; 32],
    pub next_index: u64,
    pub filled: [[u8; 32]; DEPTH],
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
    pub score: u32,
    pub max: u32,
    pub difficulty: u8,
    pub exam_mode: bool,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ClaimAccount {
    pub player: [u8; 32],
    pub count: u8,
    pub attempts: Vec<ProvenAttempt>,
}

pub const CLAIM_LEN: usize = 32 + 1 + 4 + CLAIM_CAPACITY * (32 + 32 + 4 + 4 + 1 + 1);

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
}

impl From<VitalsError> for ProgramError {
    fn from(e: VitalsError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

entrypoint!(process_instruction);

pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match Instruction::try_from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)? {
        Instruction::AnchorReplay { tree_id, record } => {
            anchor_replay(program_id, accounts, tree_id, record)
        }
        Instruction::ProveAttempt { tree_id, record, index, path } => {
            prove_attempt(program_id, accounts, tree_id, record, index, path)
        }
        Instruction::ClaimProgress { tree_id, specialty, claimed } => {
            claim_progress(program_id, accounts, tree_id, specialty, claimed)
        }
    }
}

// ── 1. anchor ───────────────────────────────────────────────────────────────

fn anchor_replay(program_id: &Pubkey, accounts: &[AccountInfo], tree_id: u64, wire: RecordWire) -> ProgramResult {
    let it = &mut accounts.iter();
    let payer = next_account_info(it)?;
    let tree_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let id = tree_seeds(tree_id);
    let (pda, bump) = Pubkey::find_program_address(&[SEED_TREE, &id], program_id);
    if pda != *tree_ai.key {
        return Err(VitalsError::WrongPda.into());
    }

    if tree_ai.data_is_empty() {
        create_pda(payer, tree_ai, system, program_id, TREE_LEN, &[SEED_TREE, &id, &[bump]])?;
        write(tree_ai, &TreeAccount::empty())?;
    }

    let record = wire.decode()?;
    // Anchoring is not open to bystanders. Without this anyone could fill somebody else's tree
    // with leaves naming other players — they could never claim them, but a full tree is a tree
    // that has to be rolled, and that is a denial of service somebody else pays for.
    if record.player != payer.key.to_bytes() {
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

fn prove_attempt(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    tree_id: u64,
    wire: RecordWire,
    index: u64,
    path: Vec<[u8; 32]>,
) -> ProgramResult {
    let it = &mut accounts.iter();
    let player = next_account_info(it)?;
    let tree_ai = next_account_info(it)?;
    let claim_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !player.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let record = wire.decode()?;
    // The record names its player, so an anchored run cannot be claimed by a bystander.
    if record.player != player.key.to_bytes() {
        return Err(VitalsError::NotYourRun.into());
    }

    let id = tree_seeds(tree_id);
    let (tree_pda, _) = Pubkey::find_program_address(&[SEED_TREE, &id], program_id);
    if tree_pda != *tree_ai.key {
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
        Pubkey::find_program_address(&[SEED_CLAIM, player.key.as_ref(), &id], program_id);
    if claim_pda != *claim_ai.key {
        return Err(VitalsError::WrongPda.into());
    }
    if claim_ai.data_is_empty() {
        create_pda(player, claim_ai, system, program_id, CLAIM_LEN,
            &[SEED_CLAIM, player.key.as_ref(), &id, &[bump]])?;
        write(claim_ai, &ClaimAccount { player: player.key.to_bytes(), count: 0, attempts: Vec::new() })?;
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
    let player = next_account_info(it)?;
    let claim_ai = next_account_info(it)?;
    let progress_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !player.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let claimed = dreyfus_from_u8(claimed)?;

    let id = tree_seeds(tree_id);
    let (claim_pda, _) =
        Pubkey::find_program_address(&[SEED_CLAIM, player.key.as_ref(), &id], program_id);
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
                &[SEED_PROGRESS, player.key.as_ref(), &[specialty]], program_id);
            if pda != *progress_ai.key {
                return Err(VitalsError::WrongPda.into());
            }
            if progress_ai.data_is_empty() {
                create_pda(player, progress_ai, system, program_id, PROGRESS_LEN,
                    &[SEED_PROGRESS, player.key.as_ref(), &[specialty], &[bump]])?;
            }
            write(progress_ai, &Progress {
                player: player.key.to_bytes(),
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
