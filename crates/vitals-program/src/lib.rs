//! The Vitals program.
//!
//! One instruction, and the whole thesis lives inside it: a player claims a Dreyfus level, the
//! program **recomputes that level from the attempts** using the same arithmetic the game uses,
//! and writes the claim only if its own maths agrees. There is no issuer key here and no
//! authority to trust — a false claim simply fails.
//!
//! **Demo shape.** Attempts arrive inside the instruction. In the real protocol they arrive as
//! merkle proofs against the anchored reveal tree, so the program verifies that each attempt was
//! actually anchored before counting it. That substitution changes where the attempts come from;
//! it does not change the adjudication below, which is the part worth demonstrating first.

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
use vitals_progress::{adjudicate, Attempt, Difficulty, Dreyfus, Verdict};

pub const SEED_PROGRESS: &[u8] = b"prog";

/// One attempt as it crosses the instruction boundary.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct AttemptWire {
    pub case: [u8; 32],
    pub score: u32,
    pub max: u32,
    /// 0 student · 1 intern · 2 resident
    pub difficulty: u8,
    pub exam_mode: bool,
}

impl AttemptWire {
    fn to_attempt(&self) -> Result<Attempt, ProgramError> {
        let difficulty = match self.difficulty {
            0 => Difficulty::Student,
            1 => Difficulty::Intern,
            2 => Difficulty::Resident,
            _ => return Err(VitalsError::BadDifficulty.into()),
        };
        Ok(Attempt {
            case: self.case,
            score: self.score,
            max: self.max,
            difficulty,
            exam_mode: self.exam_mode,
        })
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum Instruction {
    /// Claim a Dreyfus level for one specialty, and present the attempts that justify it.
    ClaimProgress { specialty: u8, claimed: u8, attempts: Vec<AttemptWire> },
}

/// What the program stores per (player, specialty). Fixed size, so the PDA is allocated once.
#[derive(BorshSerialize, BorshDeserialize, Debug, Default)]
pub struct Progress {
    pub player: [u8; 32],
    pub specialty: u8,
    /// The level the program itself computed. Never the level that was claimed.
    pub level: u8,
    pub distinct_cases: u32,
    pub xp: i64,
}

pub const PROGRESS_LEN: usize = 32 + 1 + 1 + 4 + 8;

#[derive(Debug, Clone, Copy)]
pub enum VitalsError {
    /// The claim did not survive recomputation. This is a success of the design, not a bug.
    ClaimNotEarned = 0,
    BadDifficulty = 1,
    WrongPda = 2,
    NoAttempts = 3,
}

impl From<VitalsError> for ProgramError {
    fn from(e: VitalsError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    match Instruction::try_from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)? {
        Instruction::ClaimProgress { specialty, claimed, attempts } => {
            claim_progress(program_id, accounts, specialty, claimed, attempts)
        }
    }
}

fn claim_progress(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    specialty: u8,
    claimed: u8,
    wire: Vec<AttemptWire>,
) -> ProgramResult {
    let it = &mut accounts.iter();
    let player = next_account_info(it)?;
    let progress_ai = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !player.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if wire.is_empty() {
        return Err(VitalsError::NoAttempts.into());
    }

    let claimed = dreyfus_from_u8(claimed)?;

    // Rebuild the attempts, then let the shared arithmetic decide. Note what is NOT here: no
    // authority check, no oracle, no signature from us. The verdict is a pure function of the
    // attempts, so anyone can predict it and nobody can lean on it.
    let mut attempts: Vec<Attempt> = Vec::with_capacity(wire.len());
    for w in &wire {
        attempts.push(w.to_attempt()?);
    }

    let summary = vitals_progress::summarize(&attempts);
    match adjudicate(claimed, &attempts) {
        Verdict::Rejected { claimed, computed } => {
            msg!(
                "claim rejected: claimed {}, computed {} from {} attempts ({} distinct, avg {}bps)",
                claimed.as_str(),
                computed.as_str(),
                summary.attempts,
                summary.distinct_cases,
                summary.avg_bps
            );
            Err(VitalsError::ClaimNotEarned.into())
        }
        Verdict::Granted(level) => {
            msg!(
                "claim granted: {} ({} distinct cases, {} hard, avg {}bps, xp {})",
                level.as_str(),
                summary.distinct_cases,
                summary.hard_cases,
                summary.avg_bps,
                summary.xp
            );

            let seeds: &[&[u8]] = &[SEED_PROGRESS, player.key.as_ref(), &[specialty]];
            let (pda, bump) = Pubkey::find_program_address(seeds, program_id);
            if pda != *progress_ai.key {
                return Err(VitalsError::WrongPda.into());
            }

            if progress_ai.data_is_empty() {
                let rent = Rent::get()?.minimum_balance(PROGRESS_LEN);
                invoke_signed(
                    &system_instruction::create_account(
                        player.key,
                        &pda,
                        rent,
                        PROGRESS_LEN as u64,
                        program_id,
                    ),
                    &[player.clone(), progress_ai.clone(), system.clone()],
                    &[&[SEED_PROGRESS, player.key.as_ref(), &[specialty], &[bump]]],
                )?;
            }

            let state = Progress {
                player: player.key.to_bytes(),
                specialty,
                level: level as u8,
                distinct_cases: summary.distinct_cases,
                xp: summary.xp,
            };
            let mut out = &mut progress_ai.data.borrow_mut()[..];
            state.serialize(&mut out).map_err(|_| ProgramError::AccountDataTooSmall)?;
            Ok(())
        }
    }
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
