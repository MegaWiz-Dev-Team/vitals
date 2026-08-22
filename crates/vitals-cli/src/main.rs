//! The whole loop, end to end, against a local validator.
//!
//! Play three runs of EP1 → anchor each into the tree → prove each with a Merkle path →
//! claim a level and watch the program recompute it.
//!
//! Nothing between the replay and the chain is trusted. The program never sees a tape; it sees
//! a leaf that must prove against a root it built itself.

use borsh::{BorshDeserialize, BorshSerialize};
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
    SEED_TREE,
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

    // ── 1. play and anchor ──────────────────────────────────────────────────
    println!("── 1 · play the season, anchor every run ──────────────");
    let mut records = Vec::new();
    let mut leaves = Vec::new();
    for (title, file, difficulty, tape) in &episodes {
        let json = std::fs::read_to_string(repo().join(file)).expect("scenario");
        let sce = sce_hash(&json);
        let r: Replay = replay(&json, tape).expect("replay");
        // The scenario hash is the case identity: replaying one episode is one case, however
        // many times you do it.
        let rec = record_for(player.pubkey().to_bytes(), sce, sce, *difficulty, false, tape, &r)
            .expect("record");
        println!(
            "  {title:<30} {:<14} harm {}  score {:>3}   leaf {}",
            r.outcome.clone().unwrap_or_else(|| "—".into()),
            r.harm_events.len(),
            rec.score(),
            &hex(&rec.leaf())[..12]
        );
        send(&rpc, &player, &program_id, Instruction::AnchorReplay { tree_id, record: wire(&rec) },
             vec![tree_pda(&program_id, tree_id).0], true);
        leaves.push(rec.leaf());
        records.push(rec);
    }

    let tree: TreeAccount = fetch(&rpc, &tree_pda(&program_id, tree_id).0).expect("tree");
    println!("\n  tree root {}   leaves {}\n", &hex(&tree.root)[..24], tree.next_index);

    // ── 2. prove ────────────────────────────────────────────────────────────
    println!("── 2 · prove every run belongs to this player ─────────");
    for (i, rec) in records.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).expect("path");
        let ok = send(&rpc, &player, &program_id,
            Instruction::ProveAttempt { tree_id, record: wire(rec), index: i as u64, path: path.to_vec() },
            vec![tree_pda(&program_id, tree_id).0, claim_pda(&program_id, &player.pubkey(), tree_id).0], true);
        println!("  index {i}  {}", if ok { "proven" } else { "REJECTED" });
    }

    // A leaf that was never anchored must not prove, or none of this means anything.
    let forged = AttemptRecord { harm_count: 0, outcome: vitals_progress::record::Outcome::WinDischarge, ..records[1] };
    let path = merkle::prove(&leaves, 1).expect("path");
    let ok = send(&rpc, &player, &program_id,
        Instruction::ProveAttempt { tree_id, record: wire(&forged), index: 1, path: path.to_vec() },
        vec![tree_pda(&program_id, tree_id).0, claim_pda(&program_id, &player.pubkey(), tree_id).0], false);
    println!("  forged   {}   (the stood-up run, with its harm scrubbed)",
        if ok { "ACCEPTED — the tree is broken" } else { "rejected" });

    let claim: ClaimAccount = fetch(&rpc, &claim_pda(&program_id, &player.pubkey(), tree_id).0).expect("claim");
    println!("\n  {} attempts proven and counted\n", claim.attempts.len());

    // ── 3. claim ────────────────────────────────────────────────────────────
    println!("── 3 · claim a level ───────────────────────────────────");
    for claimed in [Dreyfus::Expert, Dreyfus::Proficient] {
        let ok = send(&rpc, &player, &program_id,
            Instruction::ClaimProgress { tree_id, specialty: SPECIALTY_CARDIO, claimed: claimed as u8 },
            vec![claim_pda(&program_id, &player.pubkey(), tree_id).0,
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

fn tree_pda(program_id: &Pubkey, tree_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEED_TREE, &tree_id.to_le_bytes()], program_id)
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

fn first_program_log(err: &str) -> String {
    program_log_line(err).unwrap_or_else(|| err.lines().next().unwrap_or("").to_string())
}

fn level_name(v: u8) -> &'static str {
    match v {
        0 => "Novice", 1 => "Advanced beginner", 2 => "Competent",
        3 => "Proficient", 4 => "Expert", _ => "?",
    }
}
