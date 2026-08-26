//! The whole loop, end to end, against a local validator.
//!
//! Play three runs of EP1 → anchor each into the tree → prove each with a Merkle path →
//! claim a level and watch the program recompute it.
//!
//! Nothing between the replay and the chain is trusted. The program never sees a tape; it sees
//! a leaf that must prove against a root it built itself.

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
use vitals_progress::merkle;
use vitals_progress::record::AttemptRecord;
use vitals_progress::{Difficulty, Dreyfus};
use vitals_program::{
    ClaimAccount, Instruction, Progress, RecordWire, TreeAccount, SEED_CLAIM, SEED_PROGRESS,
    SEED_ACCOUNT,
};
use vitals_replay::{hex, record_for, replay, sce_hash, Replay, Step};

fn repo() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const SPECIALTY_CARDIO: u8 = 1;

fn tick(s: f64) -> Step { Step::Tick(s) }
fn act(s: &str) -> Step { Step::Do(s.into()) }

fn main() {
    let url = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let program_id = Pubkey::from_str(&std::env::var("VITALS_PROGRAM_ID").expect("set VITALS_PROGRAM_ID"))
        .expect("bad program id");
    let rpc = RpcClient::new_with_commitment(url.clone(), CommitmentConfig::confirmed());
    let player: Keypair = read_keypair_file(std::env::var("VITALS_KEYPAIR").unwrap_or_else(|_| {
        format!("{}/.config/solana/id.json", std::env::var("HOME").unwrap())
    }))
    .expect("keypair");

    // The season, as far as it is authored: EP1 is the reference scenario the conformance
    // vectors were frozen against; EP2–EP5 are mocks from the series bible.
    let episodes: Vec<(&str, &str, Difficulty, Vec<Step>)> = vec![
        ("EP1 · The Last Bite", "conformance/sce-anaphylaxis-ep1.json", Difficulty::Student, vec![
            tick(30.0), act("adrenaline im"), act("oxygen"), act("supine"),
            tick(60.0), act("normal saline bolus"), tick(300.0), act("admit for observation"), tick(600.0)]),
        // The same patient again, played worse. It should not buy any breadth.
        ("EP1 · replayed, stood up", "conformance/sce-anaphylaxis-ep1.json", Difficulty::Student, vec![
            tick(30.0), act("adrenaline im"), tick(30.0), act("let her stand up"), act("oxygen"),
            act("normal saline bolus"), tick(300.0), act("admit for observation"), tick(600.0)]),
        ("EP2 · Time Is Muscle", "demo/scenarios/ep2-stemi.json", Difficulty::Intern, vec![
            tick(20.0), act("ecg"), act("aspirin"), tick(30.0), act("activate the cath lab"),
            tick(300.0), tick(300.0)]),
        ("EP3 · Don't Make Him Cry", "demo/scenarios/ep3-epiglottitis.json", Difficulty::Resident, vec![
            tick(15.0), act("keep him calm"), act("blow-by oxygen"), tick(20.0),
            act("call ent and anaesthesia"), tick(30.0), act("secure the airway"), act("ceftriaxone"),
            tick(300.0), tick(300.0)]),
        ("EP4 · The Masquerader", "demo/scenarios/ep4-pulmonary-embolism.json", Difficulty::Resident, vec![
            tick(20.0), act("wells score"), tick(30.0), act("ctpa"), tick(30.0), act("heparin"),
            tick(300.0), tick(300.0)]),
        ("EP5 · The Night the Stars Fell", "demo/scenarios/ep5-the-night-the-stars-fell.json", Difficulty::Resident, vec![
            tick(10.0), act("triage"), act("tourniquet"), act("needle decompression"),
            tick(20.0), act("transfuse"), tick(30.0), act("damage control"), tick(300.0), tick(300.0)]),
    ];

    println!("cluster   {url}");
    println!("program   {program_id}");
    println!("player    {}", player.pubkey());

    // A fresh tree per run of this demo. A deployment rolls trees when one fills; the demo rolls
    // one every time so each run starts from an empty root and the indices below are the real
    // ones rather than whatever a previous run left behind.
    let tree_id = std::env::var("VITALS_TREE_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| rpc.get_slot().expect("slot"));
    println!("tree      #{tree_id}\n");

    // The person has to exist before anything can be recorded against them. Opening twice is a
    // no-op on chain, so re-running the driver is free.
    let acct = account_pda(&program_id, &player.pubkey()).0;
    send(&rpc, &player, &program_id, Instruction::OpenAccount, vec![acct], true);

    // ── 1. play and anchor ──────────────────────────────────────────────────
    println!("── 1 · play the season, anchor every run ──────────────");
    let mut records = Vec::new();
    let mut leaves = Vec::new();
    for (title, file, difficulty, tape) in &episodes {
        let json = std::fs::read_to_string(repo().join(file)).expect("scenario");
        let sce = sce_hash(&json);

        // Declare the attempt before playing it. The nonce keeps the case hidden from anyone
        // watching the chain; the slot comes back from the account because the program assigned
        // it — the record must carry the same (hash, slot) the program will stamp into the leaf,
        // or the local leaf list forks from the tree and every proof below fails.
        let nonce: [u8; 32] = {
            let mut n = [0u8; 32];
            n[..8].copy_from_slice(&tree_id.to_le_bytes());
            n[8..16].copy_from_slice(&(records.len() as u64).to_le_bytes());
            n
        };
        // Mode 0: the CLI demo plays practice runs. Exam-ness is bound into the commitment.
        let chash = vitals_progress::record::commitment_hash(&sce, &player.pubkey().to_bytes(), &nonce, 0);
        let commit_acct = vitals_program::commitment_pda(&program_id, &player.pubkey().to_bytes()).0;
        send(&rpc, &player, &program_id, Instruction::Commit { hash: chash },
             vec![acct, commit_acct], true);
        let cm: vitals_program::Commitment = fetch(&rpc, &commit_acct).expect("commitment");
        assert!(cm.open && cm.hash == chash, "the commitment on chain is not the one just made");

        let r: Replay = replay(&json, tape).expect("replay");
        // The scenario hash is the case identity: replaying one episode is one case, however
        // many times you do it.
        let rec = record_for(player.pubkey().to_bytes(), sce, sce, *difficulty, false, tape, &r, cm.hash, cm.slot)
            .expect("record");
        println!(
            "  {title:<30} {:<14} harm {}  score {:>3}   leaf {}",
            r.outcome.clone().unwrap_or_else(|| "—".into()),
            r.harm_events.len(),
            rec.score(),
            &hex(&rec.leaf())[..12]
        );
        send(&rpc, &player, &program_id, Instruction::AnchorReplay { tree_id, record: wire(&rec) },
             vec![acct, tree_pda(&program_id, &player.pubkey(), tree_id).0, commit_acct], true);
        leaves.push(rec.leaf());
        records.push(rec);
    }

    let tree: TreeAccount = fetch(&rpc, &tree_pda(&program_id, &player.pubkey(), tree_id).0).expect("tree");
    println!("\n  tree root {}   leaves {}\n", &hex(&tree.root)[..24], tree.next_index);

    // ── 2. prove ────────────────────────────────────────────────────────────
    println!("── 2 · prove every run belongs to this player ─────────");
    for (i, rec) in records.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).expect("path");
        let ok = send(&rpc, &player, &program_id,
            Instruction::ProveAttempt { tree_id, record: wire(rec), index: i as u64, path: path.to_vec(),
                                       commitment: rec.commitment, committed_slot: rec.committed_slot },
            vec![acct, tree_pda(&program_id, &player.pubkey(), tree_id).0, claim_pda(&program_id, &player.pubkey(), tree_id).0], true);
        println!("  index {i}  {}", if ok { "proven" } else { "REJECTED" });
    }

    // A leaf that was never anchored must not prove, or none of this means anything.
    let forged = AttemptRecord { harm_count: 0, outcome: vitals_progress::record::Outcome::WinDischarge, ..records[1] };
    let path = merkle::prove(&leaves, 1).expect("path");
    let ok = send(&rpc, &player, &program_id,
        Instruction::ProveAttempt { tree_id, record: wire(&forged), index: 1, path: path.to_vec(),
                                   commitment: forged.commitment, committed_slot: forged.committed_slot },
        vec![acct, tree_pda(&program_id, &player.pubkey(), tree_id).0, claim_pda(&program_id, &player.pubkey(), tree_id).0], false);
    println!("  forged   {}   (the stood-up run, with its harm scrubbed)",
        if ok { "ACCEPTED — the tree is broken" } else { "rejected" });

    let claim: ClaimAccount = fetch(&rpc, &claim_pda(&program_id, &player.pubkey(), tree_id).0).expect("claim");
    println!("\n  {} attempts proven and counted\n", claim.attempts.len());

    // ── 3. claim ────────────────────────────────────────────────────────────
    println!("── 3 · claim a level ───────────────────────────────────");
    for claimed in [Dreyfus::Expert, Dreyfus::Proficient] {
        let ok = send(&rpc, &player, &program_id,
            Instruction::ClaimProgress { tree_id, specialty: SPECIALTY_CARDIO, claimed: claimed as u8 },
            vec![acct, claim_pda(&program_id, &player.pubkey(), tree_id).0,
                 progress_pda(&program_id, &player.pubkey(), SPECIALTY_CARDIO).0], claimed == Dreyfus::Proficient);
        println!("  claim {:<18} {}", claimed.as_str(), if ok { "GRANTED" } else { "REJECTED" });
    }

    if let Some(p) = fetch::<Progress>(&rpc, &progress_pda(&program_id, &player.pubkey(), SPECIALTY_CARDIO).0) {
        println!("\n  onchain: level {}  ·  {} attempts  ·  {} distinct case(s)  ·  xp {}",
            level_name(p.level), p.attempts_counted, p.distinct_cases, p.xp);
        println!("\n  six runs, five episodes — EP1 was played twice and counted once.");
        println!("  Expert wants eight distinct cases. The season is only five long.");
    }
}

// ── plumbing ────────────────────────────────────────────────────────────────

fn wire(r: &AttemptRecord) -> RecordWire {
    RecordWire {
        player: r.player,
        sce_hash: r.sce_hash,
        case: r.case,
        run_hash: r.run_hash,
        rubric_hash: r.rubric_hash,
        det_score: r.det_score,
        det_max: r.det_max,
        judged_score: r.judged_score,
        judged_max: r.judged_max,
        difficulty: match r.difficulty {
            Difficulty::Student => 0,
            Difficulty::Intern => 1,
            Difficulty::Resident => 2,
        },
        exam_mode: r.exam_mode,
        outcome: r.outcome as u8,
        harm_count: r.harm_count,
    }
}

/// The person. Its id is the driver's own key, so the claim and progress addresses below are the
/// same ones the device-seeded scheme produced before accounts existed.
fn account_pda(program_id: &Pubkey, id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEED_ACCOUNT, &id.to_bytes()], program_id)
}
/// Deliberately delegates rather than re-deriving.
///
/// This file used to carry its own copy of the seeds. When the tree became operator-scoped, the
/// copy did not, and the driver started reading an account that no longer existed — a duplicated
/// derivation is a second definition of where things live, and the two drift silently.
fn tree_pda(program_id: &Pubkey, operator: &Pubkey, tree_id: u64) -> (Pubkey, u8) {
    vitals_program::tree_pda(program_id, operator, tree_id)
}
fn claim_pda(program_id: &Pubkey, player: &Pubkey, tree_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEED_CLAIM, player.as_ref(), &tree_id.to_le_bytes()], program_id)
}
fn progress_pda(program_id: &Pubkey, player: &Pubkey, specialty: u8) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEED_PROGRESS, player.as_ref(), &[specialty]], program_id)
}

/// Returns whether the transaction succeeded. `expect_ok` only controls how loudly a surprise is
/// reported — a rejection we predicted is not a failure of the demo, it is the demo.
fn send(
    rpc: &RpcClient,
    player: &Keypair,
    program_id: &Pubkey,
    ix: Instruction,
    extra: Vec<Pubkey>,
    expect_ok: bool,
) -> bool {
    // The driver funds its own runs, so it is both accounts. The web server is where they differ:
    // there the relay pays and the player's key never leaves the browser.
    let mut metas = vec![
        AccountMeta::new(player.pubkey(), true),
        AccountMeta::new_readonly(player.pubkey(), true),
    ];
    metas.extend(extra.into_iter().map(|k| AccountMeta::new(k, false)));
    metas.push(AccountMeta::new_readonly(system_program::id(), false));

    let ix = SolInstruction { program_id: *program_id, accounts: metas, data: borsh::to_vec(&ix).expect("ser") };
    let blockhash = rpc.get_latest_blockhash().expect("blockhash");
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&player.pubkey()), &[player], blockhash);

    match rpc.send_and_confirm_transaction(&tx) {
        Ok(_) => true,
        Err(e) => {
            if expect_ok {
                println!("      unexpected failure:");
                for l in e.to_string().lines().take(24) { println!("        {}", l.trim()); }
            } else if let Some(l) = program_log_line(&e.to_string()) {
                println!("      {l}");
            }
            false
        }
    }
}

fn fetch<T: BorshDeserialize>(rpc: &RpcClient, key: &Pubkey) -> Option<T> {
    // Prefix decode, for the same reason the program does: a claim buffer is mostly zeros.
    rpc.get_account_data(key).ok().and_then(|d| T::deserialize(&mut &d[..]).ok())
}

fn program_log_line(err: &str) -> Option<String> {
    err.lines()
        .map(str::trim)
        .find(|l| l.starts_with("Program log:") && !l.contains("instruction"))
        .map(|l| l.trim_start_matches("Program log:").trim().to_string())
}

/// The first line of a program error that is worth showing a human.
///
/// Only the tests call it, so it is compiled only for them. Without the gate the binary build
/// reports it as dead code and the test build stops compiling without it — the two clippy runs
/// disagree, and deleting it to satisfy the first one is what broke the second.
#[cfg(test)]
fn first_program_log(err: &str) -> String {
    program_log_line(err).unwrap_or_else(|| err.lines().next().unwrap_or("").to_string())
}

fn level_name(v: u8) -> &'static str {
    match v {
        0 => "Novice", 1 => "Advanced beginner", 2 => "Competent",
        3 => "Proficient", 4 => "Expert", _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The driver is the reference client, so the addresses it computes have to be the same ones
    /// the program derives — otherwise it works by luck on whatever the server also happens to
    /// get wrong.
    #[test]
    fn the_addresses_match_the_program_seeds() {
        let pid = Pubkey::new_from_array([9; 32]);
        let me = Pubkey::new_from_array([4; 32]);
        assert_eq!(
            account_pda(&pid, &me).0,
            Pubkey::find_program_address(&[SEED_ACCOUNT, &me.to_bytes()], &pid).0
        );
        assert_eq!(
            tree_pda(&pid, &me, 7).0,
            vitals_program::tree_pda(&pid, &me, 7).0
        );
        assert_eq!(
            claim_pda(&pid, &me, 7).0,
            Pubkey::find_program_address(&[SEED_CLAIM, me.as_ref(), &7u64.to_le_bytes()], &pid).0
        );
        assert_eq!(
            progress_pda(&pid, &me, 1).0,
            Pubkey::find_program_address(&[SEED_PROGRESS, me.as_ref(), &[1]], &pid).0
        );
    }

    /// Different ids must not collide, or two seasons share a tree.
    #[test]
    fn a_different_tree_id_is_a_different_tree() {
        let pid = Pubkey::new_from_array([9; 32]);
        let me = Pubkey::new_unique();
        assert_ne!(tree_pda(&pid, &me, 1).0, tree_pda(&pid, &me, 2).0);
        // And the operator is load-bearing: same id, different funder, different tree.
        assert_ne!(tree_pda(&pid, &me, 1).0, tree_pda(&pid, &Pubkey::new_unique(), 1).0);
        // And the same id is stable, which is what lets a rerun add to the season it started.
        assert_eq!(tree_pda(&pid, &me, 42).0, tree_pda(&pid, &me, 42).0);
    }

    /// Two players never share a claim buffer. This was once false, and every player on a server
    /// held the same level as a result.
    #[test]
    fn two_players_never_share_a_buffer() {
        let pid = Pubkey::new_from_array([9; 32]);
        let a = Pubkey::new_from_array([1; 32]);
        let b = Pubkey::new_from_array([2; 32]);
        assert_ne!(claim_pda(&pid, &a, 7).0, claim_pda(&pid, &b, 7).0);
        assert_ne!(progress_pda(&pid, &a, 1).0, progress_pda(&pid, &b, 1).0);
        // Nor do two specialties of one player.
        assert_ne!(progress_pda(&pid, &a, 1).0, progress_pda(&pid, &a, 2).0);
    }

    #[test]
    fn a_refusal_reads_as_the_program_speaking() {
        let err = "\
RPC response error -32002: Transaction simulation failed
    Program log: claim rejected: claimed Expert, computed Proficient
    Program abc consumed 1111 of 200000 compute units";
        assert_eq!(first_program_log(err), "claim rejected: claimed Expert, computed Proficient");
    }

    #[test]
    fn a_transport_failure_is_not_dressed_up_as_a_refusal() {
        let err = "error sending request for url (http://127.0.0.1:8899/)";
        assert_eq!(program_log_line(err), None);
        assert!(first_program_log(err).starts_with("error sending request"));
    }

    #[test]
    fn levels_are_named_in_order_and_nothing_beyond_them_is() {
        assert_eq!(
            (0..5).map(level_name).collect::<Vec<_>>(),
            ["Novice", "Advanced beginner", "Competent", "Proficient", "Expert"]
        );
        assert_eq!(level_name(5), "?");
    }

    #[test]
    fn difficulty_survives_the_trip_to_the_wire() {
        use vitals_progress::record::Outcome;
        for (d, n) in [
            (vitals_progress::Difficulty::Student, 0u8),
            (vitals_progress::Difficulty::Intern, 1),
            (vitals_progress::Difficulty::Resident, 2),
        ] {
            let r = AttemptRecord {
                player: [1; 32], sce_hash: [2; 32], case: [3; 32], run_hash: [4; 32],
                difficulty: d, exam_mode: false, outcome: Outcome::WinDischarge, harm_count: 0,
                commitment: [0u8; 32], committed_slot: 0, rubric_hash: [0u8; 32],
                det_score: 0, det_max: 0, judged_score: 0, judged_max: 0,
            };
            assert_eq!(wire(&r).difficulty, n);
        }
    }

    /// The season the driver plays has to be on disk, or it panics halfway through a demo.
    #[test]
    fn every_scenario_the_driver_plays_exists() {
        for f in [
            "conformance/sce-anaphylaxis-ep1.json",
            "demo/scenarios/ep2-stemi.json",
            "demo/scenarios/ep3-epiglottitis.json",
            "demo/scenarios/ep4-pulmonary-embolism.json",
            "demo/scenarios/ep5-the-night-the-stars-fell.json",
        ] {
            let p = repo().join(f);
            assert!(p.exists(), "{} is missing", p.display());
        }
    }

    #[test]
    fn tape_helpers_build_what_they_say() {
        assert_eq!(tick(30.0), Step::Tick(30.0));
        assert_eq!(act("adrenaline im"), Step::Do("adrenaline im".into()));
    }
}
