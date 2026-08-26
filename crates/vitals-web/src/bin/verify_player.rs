//! Independent on-chain verification of a player's level — the reproducible half of the thesis.
//!
//! `VERIFICATION.md` (attached to the demo footage) points a reviewer here: clone the public repo
//! and run this, and you re-derive a player's level yourself, from devnet, without trusting whoever
//! recorded the video. It is a **keyless read** — no signer, no key material — and it runs the
//! program's own `summarize`/`dreyfus`/`adjudicate`, not a re-implementation, so the number it
//! prints is the number a claim would compute this instant.
//!
//! Defaults are the values under verification in this submission; override to check any player:
//!   `cargo run -p vitals-web --bin verify_player --release -- <PLAYER_PUBKEY> [TREE_ID]`
use borsh::BorshDeserialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use vitals_program::{
    commitment_pda, ClaimAccount, Commitment, Progress, SEED_CLAIM, SEED_PROGRESS,
};
use vitals_progress::{adjudicate, summarize, Attempt, Difficulty, Dreyfus, Verdict};

/// The deployed program on devnet.
const PROGRAM: &str = "535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG";
/// The player under verification in this submission (a pubkey is public; no identity is implied).
const PLAYER: &str = "9iTTpzAHhVqsWSJU37rZuNU92sRSeBjjJ1LMLHRCPSFv";
/// The demo server's Merkle tree these runs anchored to.
const TREE_ID: u64 = 487_877_348;
const RPC: &str = "https://api.devnet.solana.com";

fn main() {
    let mut args = std::env::args().skip(1);
    let player = args.next().unwrap_or_else(|| PLAYER.to_string());
    let tree_id: u64 = args
        .next()
        .map_or(TREE_ID, |s| s.parse().expect("TREE_ID must be a u64"));

    let program = Pubkey::from_str(PROGRAM).expect("PROGRAM pubkey");
    let id = Pubkey::from_str(&player).expect("PLAYER pubkey");
    let rpc = RpcClient::new_with_commitment(RPC.to_string(), CommitmentConfig::confirmed());
    println!("verifying {id} · tree #{tree_id} · program {program} · {RPC}\n");

    // started — the honesty counter the commit-reveal design exists to make undeniable.
    let (cpda, _) = commitment_pda(&program, &id.to_bytes());
    let started = rpc
        .get_account_data(&cpda)
        .ok()
        .and_then(|d| Commitment::deserialize(&mut &d[..]).ok())
        .map(|c| c.started);
    match started {
        Some(n) => println!("started (commitments ever made) : {n}"),
        None => println!("started (commitments ever made) : none"),
    }

    // stored Progress — the snapshot the last successful claim wrote.
    let (ppda, _) = Pubkey::find_program_address(&[SEED_PROGRESS, id.as_ref(), &[1u8]], &program);
    match rpc
        .get_account_data(&ppda)
        .ok()
        .and_then(|d| Progress::deserialize(&mut &d[..]).ok())
    {
        Some(p) => println!(
            "stored Progress (last claim)    : level {} · {} attempts · {} distinct · xp {}",
            p.level, p.attempts_counted, p.distinct_cases, p.xp
        ),
        None => println!("stored Progress (last claim)    : none claimed yet"),
    }

    // The live claim buffer — the exact inputs a NEW claim would adjudicate over.
    let (clpda, _) =
        Pubkey::find_program_address(&[SEED_CLAIM, id.as_ref(), &tree_id.to_le_bytes()], &program);
    let Some(data) = rpc.get_account_data(&clpda).ok() else {
        println!("\nno ClaimAccount at {clpda} — nothing proven on this tree");
        return;
    };
    let claim = ClaimAccount::deserialize(&mut &data[..]).expect("ClaimAccount decode");
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

    println!("\nlive ClaimAccount: {} proven attempts", attempts.len());
    for (i, a) in claim.attempts.iter().enumerate() {
        let c = &a.case;
        let tag = format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]);
        println!(
            "  #{i} case {tag}… outcome {}/{} · det {}/{} diff {} exam {}",
            a.score, a.max, a.det_score, a.det_max, a.difficulty, a.exam_mode
        );
    }
    let stars = vitals_progress::stars(&attempts, vitals_progress::STAR_PASS_BPS);
    println!(
        "\nstars (distinct exam cases cleared at det >= {}%): {stars}",
        vitals_progress::STAR_PASS_BPS / 100
    );

    let s = summarize(&attempts);
    println!(
        "\nsummary: distinct {} · hard {} · avg {}bps · computed = {}",
        s.distinct_cases,
        s.hard_cases,
        s.avg_bps,
        s.dreyfus.as_str()
    );
    for want in [Dreyfus::Competent, Dreyfus::Proficient] {
        let line = match adjudicate(want, &attempts) {
            Verdict::Granted(l) => format!("GRANTED → {}", l.as_str()),
            Verdict::Rejected { claimed, computed } => {
                format!(
                    "REJECTED (claimed {}, computed {})",
                    claimed.as_str(),
                    computed.as_str()
                )
            }
        };
        println!("  claim {:<11}: {line}", want.as_str());
    }
}
