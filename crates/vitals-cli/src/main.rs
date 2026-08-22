//! The Vitals demo, end to end, against a local validator.
//!
//! Three cardiology cases across three tiers — the whole run a player could have. Then two
//! claims against the same evidence:
//!
//!   1. Proficient — which the shipping thresholds do not allow on three distinct cases
//!   2. Competent  — which they do
//!
//! The first transaction fails. That failure is the demonstration: no authority refused it, the
//! program recomputed the level and disagreed with the player. Run it twice, from two machines,
//! against two validators — same evidence, same verdict, every time.

use borsh::BorshDeserialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction as SolInstruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;
use vitals_program::{AttemptWire, Instruction, Progress, SEED_PROGRESS};
use vitals_progress::Dreyfus;

const SPECIALTY_CARDIO: u8 = 1;

fn case_id(name: &str) -> [u8; 32] {
    // Stand-in for the anchored case hash. The real leaf hashes the case content; here we only
    // need distinct, stable identities so `distinct_cases` counts what a player actually played.
    let mut out = [0u8; 32];
    let b = name.as_bytes();
    out[..b.len().min(32)].copy_from_slice(&b[..b.len().min(32)]);
    out
}

fn main() {
    let url = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let program_id = Pubkey::from_str(
        &std::env::var("VITALS_PROGRAM_ID").expect("set VITALS_PROGRAM_ID"),
    )
    .expect("bad program id");

    let rpc = RpcClient::new_with_commitment(url.clone(), CommitmentConfig::confirmed());
    let payer: Keypair = read_keypair_file(
        std::env::var("VITALS_KEYPAIR")
            .unwrap_or_else(|_| format!("{}/.config/solana/id.json", std::env::var("HOME").unwrap())),
    )
    .expect("keypair");

    println!("cluster   {url}");
    println!("program   {program_id}");
    println!("player    {}\n", payer.pubkey());

    // The run: three distinct cardiology cases, one per tier.
    let attempts = vec![
        AttemptWire {
            case: case_id("stable-angina"),
            score: 72,
            max: 100,
            difficulty: 0, // student
            exam_mode: false,
        },
        AttemptWire {
            case: case_id("anterior-stemi"),
            score: 78,
            max: 100,
            difficulty: 1, // intern
            exam_mode: false,
        },
        AttemptWire {
            case: case_id("aortic-dissection"),
            score: 70,
            max: 100,
            difficulty: 2, // resident
            exam_mode: false,
        },
    ];

    let (progress_pda, _) = Pubkey::find_program_address(
        &[SEED_PROGRESS, payer.pubkey().as_ref(), &[SPECIALTY_CARDIO]],
        &program_id,
    );
    println!("progress  {progress_pda}\n");

    submit(&rpc, &payer, &program_id, progress_pda, Dreyfus::Proficient, &attempts);
    submit(&rpc, &payer, &program_id, progress_pda, Dreyfus::Competent, &attempts);

    match rpc.get_account_data(&progress_pda) {
        Ok(data) => {
            let p = Progress::try_from_slice(&data).expect("decode");
            println!("\nonchain progress account");
            println!("  specialty       {}", p.specialty);
            println!("  level           {}", level_name(p.level));
            println!("  distinct cases  {}", p.distinct_cases);
            println!("  xp              {}", p.xp);
            println!("\nthe level stored is the one the program computed, never the one claimed.");
        }
        Err(e) => println!("\nno progress account: {e}"),
    }
}

fn submit(
    rpc: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    progress_pda: Pubkey,
    claimed: Dreyfus,
    attempts: &[AttemptWire],
) {
    println!("── claim: {} ────────────────────────────────", claimed.as_str());

    let ix_data = borsh::to_vec(&Instruction::ClaimProgress {
        specialty: SPECIALTY_CARDIO,
        claimed: claimed as u8,
        attempts: attempts.to_vec(),
    })
    .expect("serialize");

    let ix = SolInstruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(progress_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: ix_data,
    };

    let blockhash = rpc.get_latest_blockhash().expect("blockhash");
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        blockhash,
    );

    match rpc.send_and_confirm_transaction(&tx) {
        Ok(sig) => println!("  GRANTED   {sig}"),
        Err(e) => {
            let s = e.to_string();
            if s.contains("custom program error: 0x0") {
                println!("  REJECTED  the program recomputed the level and disagreed");
            } else {
                println!("  FAILED    {s}");
            }
            if let Some(logs) = program_logs(&s) {
                for l in logs {
                    println!("            {l}");
                }
            }
        }
    }
    println!();
}

/// Pull the `msg!` lines out of the RpcError text so the verdict speaks for itself.
fn program_logs(err: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for line in err.lines() {
        let t = line.trim();
        if t.starts_with("Program log:") {
            out.push(t.trim_start_matches("Program log:").trim().to_string());
        }
    }
    (!out.is_empty()).then_some(out)
}

fn level_name(v: u8) -> &'static str {
    match v {
        0 => "Novice",
        1 => "Advanced beginner",
        2 => "Competent",
        3 => "Proficient",
        4 => "Expert",
        _ => "?",
    }
}
