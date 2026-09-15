//! The ward, on a real cluster: what it must accept, and what it must refuse.
//!
//! The assertions are written before the program is deployed, and that is deliberate — a proof
//! whose expectations are written after watching the cluster is a description, not a proof.
//!
//! What it does. One operator admits one patient. Key A takes the head, plays a shift, anchors it.
//! Key B, who worked from the state A left, anchors against the head A moved past and is refused.
//! B takes the head properly and anchors on it, and the chain is two shifts long. Then a third key
//! tries to take a head B is holding and is refused. Every refusal is checked by its error code,
//! not by "the transaction failed" — a transaction that fails for the wrong reason would pass a
//! weaker test and prove nothing.
//!
//!   VITALS_PROGRAM_ID=<the ward's id> cargo run -p vitals-cli --bin ward_proof
//!   RPC=https://api.devnet.solana.com  VITALS_KEYPAIR=~/.config/solana/id.json
//!
//! Exit 0 only if every expectation held.

use borsh::BorshDeserialize;
use solana_rpc_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction as SolInstruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;
use vitals_program::{
    patient_pda, tree_pda, commitment_pda, Instruction, PatientAccount, RecordWire, VitalsError,
    SEED_ACCOUNT, PATIENT_OPEN,
};
use vitals_progress::record::{AttemptRecord, Outcome};
use vitals_progress::Difficulty;

const TREE: u64 = 1;

fn env(k: &str, fallback: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| fallback.to_string())
}

fn acct(pid: &Pubkey, id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[SEED_ACCOUNT, &id.to_bytes()], pid).0
}

fn wire(r: &AttemptRecord) -> RecordWire {
    RecordWire {
        player: r.player, sce_hash: r.sce_hash, case: r.case, run_hash: r.run_hash,
        difficulty: r.difficulty as u8, exam_mode: r.exam_mode, outcome: r.outcome as u8,
        harm_count: r.harm_count, rubric_hash: r.rubric_hash,
        det_score: r.det_score, det_max: r.det_max,
        judged_score: r.judged_score, judged_max: r.judged_max,
    }
}

/// Send one instruction and say whether it did what was expected. `want` is `Ok(())` for an
/// expected success or `Err(code)` for the refusal that must happen and no other.
// Eight arguments, and each one is a different thing the cluster needs: who pays, who plays, which
// program, which instruction, which accounts, what to print and what must happen. Bundling them
// would hide the one that matters at each call — the expectation — behind a struct literal.
#[allow(clippy::too_many_arguments)]
fn expect(
    rpc: &RpcClient, funder: &Keypair, player: &Keypair, pid: &Pubkey,
    ix: Instruction, metas: Vec<AccountMeta>, what: &str, want: Result<(), u32>,
) -> bool {
    let ix = SolInstruction { program_id: *pid, accounts: metas, data: borsh::to_vec(&ix).expect("ser") };
    let bh = rpc.get_latest_blockhash().expect("blockhash");
    let mut signers: Vec<&Keypair> = vec![funder];
    if player.pubkey() != funder.pubkey() { signers.push(player); }
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&funder.pubkey()), &signers, bh);
    let got = rpc.send_and_confirm_transaction(&tx);
    match (want, &got) {
        (Ok(()), Ok(_)) => { println!("  ok       {what}"); true }
        (Err(code), Err(e)) => {
            let s = e.to_string();
            if s.contains(&format!("custom program error: {code:#x}")) || s.contains(&format!("Custom({code})")) {
                println!("  refused  {what} — error {code}, as it must");
                true
            } else {
                println!("  WRONG    {what} — refused, but not with error {code}:");
                for l in s.lines().take(6) { println!("             {}", l.trim()); }
                false
            }
        }
        (Ok(()), Err(e)) => {
            println!("  FAILED   {what}");
            for l in e.to_string().lines().take(8) { println!("             {}", l.trim()); }
            false
        }
        (Err(code), Ok(_)) => {
            println!("  ACCEPTED {what} — it had to be refused with error {code} and was not");
            false
        }
    }
}

fn main() {
    let url = env("RPC", "https://api.devnet.solana.com");
    let rpc = RpcClient::new_with_commitment(url.clone(), CommitmentConfig::confirmed());
    let pid = Pubkey::from_str(&std::env::var("VITALS_PROGRAM_ID").expect("set VITALS_PROGRAM_ID"))
        .expect("program id");
    let keyfile = env("VITALS_KEYPAIR", &format!("{}/.config/solana/id.json", env("HOME", "")));
    let operator = read_keypair_file(&keyfile).unwrap_or_else(|e| panic!("{keyfile}: {e}"));
    let op = operator.pubkey();
    println!("── cluster  {url}\n── program  {pid}\n── operator {op}");

    let (a, b, c) = (Keypair::new(), Keypair::new(), Keypair::new());
    let patient_id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let pkey = patient_pda(&pid, &op, patient_id).0;
    let tree = tree_pda(&pid, &op, TREE).0;
    println!("── patient  {patient_id}  ({pkey})");

    let mut ok = true;

    // Everyone needs an account before they can hold anything.
    for (who, name) in [(&a, "A"), (&b, "B"), (&c, "C")] {
        ok &= expect(&rpc, &operator, who, &pid, Instruction::OpenAccount,
            vec![AccountMeta::new(op, true), AccountMeta::new_readonly(who.pubkey(), true),
                 AccountMeta::new(acct(&pid, &who.pubkey()), false),
                 AccountMeta::new_readonly(system_program::id(), false)],
            &format!("{name} opens an account"), Ok(()));
    }

    // ── admission ─────────────────────────────────────────────────────────
    ok &= expect(&rpc, &operator, &operator, &pid,
        Instruction::AdmitPatient { patient_id, scenario_hash: [1; 32] },
        vec![AccountMeta::new(op, true), AccountMeta::new(pkey, false),
             AccountMeta::new_readonly(system_program::id(), false)],
        "the operator admits a patient", Ok(()));

    let p: PatientAccount = rpc.get_account_data(&pkey).ok()
        .and_then(|d| PatientAccount::deserialize(&mut &d[..]).ok())
        .expect("the patient account must exist after admission");
    if p.head != [0; 32] || p.state != PATIENT_OPEN || p.shifts != 0 {
        println!("  WRONG    a new patient's chart is not empty: {p:?}");
        ok = false;
    } else {
        println!("  ok       her chart starts empty");
    }

    // ── A takes the head, plays a shift, anchors it ───────────────────────
    let take = |who: &Keypair| vec![
        AccountMeta::new_readonly(who.pubkey(), true),
        AccountMeta::new_readonly(acct(&pid, &who.pubkey()), false),
        AccountMeta::new(pkey, false),
    ];
    let anchor_metas = |who: &Keypair| vec![
        AccountMeta::new(op, true),
        AccountMeta::new_readonly(who.pubkey(), true),
        AccountMeta::new(acct(&pid, &who.pubkey()), false),
        AccountMeta::new(tree, false),
        AccountMeta::new(commitment_pda(&pid, &who.pubkey().to_bytes()).0, false),
        AccountMeta::new(pkey, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    ok &= expect(&rpc, &operator, &a, &pid, Instruction::TakeShift { patient_id },
                 take(&a), "A takes the head", Ok(()));
    let ra = declared(&rpc, &operator, &a, &pid, 1, &mut ok);
    ok &= expect(&rpc, &operator, &a, &pid,
        Instruction::AnchorShift { tree_id: TREE, patient_id, record: wire(&ra), prev_head: [0; 32] },
        anchor_metas(&a), "A anchors the first shift onto an empty chart", Ok(()));

    let after_a = read_patient(&rpc, &pkey);
    if after_a.shifts != 1 || after_a.head == [0; 32] || after_a.lease_holder != [0; 32] {
        println!("  WRONG    after A: shifts {}, head moved {}, lease cleared {}",
                 after_a.shifts, after_a.head != [0; 32], after_a.lease_holder == [0; 32]);
        ok = false;
    } else {
        println!("  ok       the head moved to A's leaf and the lease came back");
    }

    // ── B worked from the state A left, and says so wrongly ───────────────
    ok &= expect(&rpc, &operator, &b, &pid, Instruction::TakeShift { patient_id },
                 take(&b), "B takes the head", Ok(()));
    let rb = declared(&rpc, &operator, &b, &pid, 2, &mut ok);
    ok &= expect(&rpc, &operator, &b, &pid,
        Instruction::AnchorShift { tree_id: TREE, patient_id, record: wire(&rb), prev_head: [0; 32] },
        anchor_metas(&b), "B anchors against the head the patient moved past",
        Err(VitalsError::StaleHead as u32));

    // ── C cannot walk into a room B is in ─────────────────────────────────
    ok &= expect(&rpc, &operator, &c, &pid, Instruction::TakeShift { patient_id },
                 take(&c), "C takes a head B is holding", Err(VitalsError::LeaseHeld as u32));

    // ── B anchors on the head that is actually there ──────────────────────
    let rb = declared(&rpc, &operator, &b, &pid, 2, &mut ok);
    ok &= expect(&rpc, &operator, &b, &pid,
        Instruction::AnchorShift { tree_id: TREE, patient_id, record: wire(&rb), prev_head: after_a.head },
        anchor_metas(&b), "B anchors on A's head", Ok(()));

    let after_b = read_patient(&rpc, &pkey);
    if after_b.shifts != 2 || after_b.head == after_a.head {
        println!("  WRONG    after B: shifts {}, head moved again {}",
                 after_b.shifts, after_b.head != after_a.head);
        ok = false;
    } else {
        println!("  ok       two strangers, one chart, and the chain is two shifts long");
    }

    if !ok {
        eprintln!("\nproof FAILED");
        std::process::exit(1);
    }
    println!("\nproof passed: the ward accepted the handover and refused the stale head and the held lease");
}

fn read_patient(rpc: &RpcClient, key: &Pubkey) -> PatientAccount {
    rpc.get_account_data(key).ok()
        .and_then(|d| PatientAccount::deserialize(&mut &d[..]).ok())
        .expect("the patient account must be readable")
}

/// Declare an attempt the way a client does, then read back what the chain recorded. The slot is
/// the program's, not ours, and a record built with a different one hashes to a different leaf.
fn declared(
    rpc: &RpcClient, funder: &Keypair, who: &Keypair, pid: &Pubkey, case: u8, ok: &mut bool,
) -> AttemptRecord {
    let cp = commitment_pda(pid, &who.pubkey().to_bytes()).0;
    *ok &= expect(rpc, funder, who, pid, Instruction::Commit { hash: [7; 32] },
        vec![AccountMeta::new(funder.pubkey(), true),
             AccountMeta::new_readonly(who.pubkey(), true),
             AccountMeta::new(acct(pid, &who.pubkey()), false),
             AccountMeta::new(cp, false),
             AccountMeta::new_readonly(system_program::id(), false)],
        "the shift is declared before it is played", Ok(()));
    let c: vitals_program::Commitment = rpc.get_account_data(&cp).ok()
        .and_then(|d| BorshDeserialize::deserialize(&mut &d[..]).ok())
        .expect("the commitment the chain recorded");
    let mut case_bytes = [0u8; 32];
    case_bytes[0] = case;
    AttemptRecord {
        player: who.pubkey().to_bytes(),
        sce_hash: [9; 32], case: case_bytes, run_hash: [3; 32],
        commitment: c.hash, committed_slot: c.slot,
        rubric_hash: [0; 32], det_score: 0, det_max: 0, judged_score: 0, judged_max: 0,
        difficulty: Difficulty::Student, exam_mode: false,
        outcome: Outcome::NoTerminal, harm_count: 0,
    }
}
