//! The program under test, in a real validator.
//!
//! This is the crate that holds the record and refuses claims, and until now it was the only one
//! with no tests at all — the arithmetic underneath it was covered and the thing that decides
//! was not. Every path that can reject something gets a test here, because a refusal that
//! silently stops working is indistinguishable from a system that never refused anything.

use solana_program_test::{processor, ProgramTest, ProgramTestBanksClientExt};
use solana_sdk::{
    instruction::{AccountMeta, Instruction as SolIx},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
    instruction::InstructionError,
};
use vitals_program::*;
use vitals_progress::merkle;
use vitals_progress::record::{AttemptRecord, Outcome};
use vitals_progress::{Difficulty, Dreyfus};

const TREE: u64 = 7;

fn rec(player: &Pubkey, case: u8, outcome: Outcome, harm: u16, d: Difficulty) -> AttemptRecord {
    let mut c = [0u8; 32];
    c[0] = case;
    AttemptRecord {
        player: player.to_bytes(),
        sce_hash: [9; 32],
        case: c,
        run_hash: [3; 32],
        difficulty: d,
        exam_mode: false,
        outcome,
        harm_count: harm,
    }
}

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

fn pdas(pid: &Pubkey, who: &Pubkey) -> (Pubkey, Pubkey, Pubkey) {
    let id = TREE.to_le_bytes();
    (
        Pubkey::find_program_address(&[SEED_TREE, &id], pid).0,
        Pubkey::find_program_address(&[SEED_CLAIM, who.as_ref(), &id], pid).0,
        Pubkey::find_program_address(&[SEED_PROGRESS, who.as_ref(), &[1u8]], pid).0,
    )
}

/// `funder` has the lamports and pays rent. `player` is whose run it is and needs no balance —
/// it signs, and nothing is ever debited from it. They are the same key only because most of this
/// test does not care; `a_player_with_no_sol_can_still_prove_and_claim` is where they differ.
fn ix(pid: Pubkey, funder: Pubkey, player: Pubkey, extra: &[Pubkey], data: Instruction) -> SolIx {
    let mut metas = vec![
        AccountMeta::new(funder, true),
        AccountMeta::new_readonly(player, true),
    ];
    metas.extend(extra.iter().map(|k| AccountMeta::new(*k, false)));
    metas.push(AccountMeta::new_readonly(system_program::id(), false));
    SolIx { program_id: pid, accounts: metas, data: borsh::to_vec(&data).unwrap() }
}

fn custom(err: &TransactionError) -> Option<u32> {
    match err {
        TransactionError::InstructionError(_, InstructionError::Custom(c)) => Some(*c),
        _ => None,
    }
}

/// Everything in one test: the harness boots a validator per test and that is not cheap, so the
/// happy path and the refusals share one.
#[tokio::test]
async fn anchor_prove_claim_and_every_way_it_can_refuse() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, claim, prog) = pdas(&pid, &me);

    // ── a claim before anything is proven ──────────────────────────────────
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[claim, prog], Instruction::ClaimProgress { tree_id: TREE, specialty: 1, claimed: 2 })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NoAttempts as u32), "an empty claim must not pass");

    // ── anchoring somebody else's run ─────────────────────────────────────
    let theirs = rec(&Pubkey::new_unique(), 1, Outcome::WinDischarge, 0, Difficulty::Student);
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[tree], Instruction::AnchorReplay { tree_id: TREE, record: wire(&theirs) })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NotYourRun as u32), "a bystander must not fill the tree");

    // ── anchor three of my own, one per case ──────────────────────────────
    let mine: Vec<AttemptRecord> = (1..=3)
        .map(|i| rec(&me, i, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();
    let mut leaves = Vec::new();
    for r in &mine {
        let bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, me, me, &[tree], Instruction::AnchorReplay { tree_id: TREE, record: wire(r) })],
                Some(&me), &[&payer], bh,
            ))
            .await
            .expect("anchoring my own run");
        leaves.push(r.leaf());
    }

    // ── a record that was never anchored must not prove ───────────────────
    let forged = AttemptRecord { harm_count: 0, outcome: Outcome::WinIcu, ..mine[0] };
    let path = merkle::prove(&leaves, 0).unwrap();
    let bh1 = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[tree, claim], Instruction::ProveAttempt {
                tree_id: TREE, record: wire(&forged), index: 0, path: path.to_vec() })],
            Some(&me), &[&payer], bh1,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::ProofFailed as u32), "a forged leaf must not prove");

    // ── prove all three ───────────────────────────────────────────────────
    for (i, r) in mine.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).unwrap();
        let bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, me, me, &[tree, claim], Instruction::ProveAttempt {
                    tree_id: TREE, record: wire(r), index: i as u64, path: path.to_vec() })],
                Some(&me), &[&payer], bh,
            ))
            .await
            .unwrap_or_else(|e| panic!("proving index {i}: {e:?}"));
    }

    // ── the same run twice ────────────────────────────────────────────────
    let path = merkle::prove(&leaves, 0).unwrap();
    let bh2 = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[tree, claim], Instruction::ProveAttempt {
                tree_id: TREE, record: wire(&mine[0]), index: 0, path: path.to_vec() })],
            Some(&me), &[&payer], bh2,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::DuplicateAttempt as u32), "one run counts once");

    // ── claim more than the arithmetic allows ─────────────────────────────
    let bh3 = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[claim, prog], Instruction::ClaimProgress {
                tree_id: TREE, specialty: 1, claimed: Dreyfus::Expert as u8 })],
            Some(&me), &[&payer], bh3,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::ClaimNotEarned as u32), "Expert needs eight distinct cases");

    // ── and the claim the evidence does support ───────────────────────────
    let bh4 = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[claim, prog], Instruction::ClaimProgress {
                tree_id: TREE, specialty: 1, claimed: Dreyfus::Competent as u8 })],
            Some(&me), &[&payer], bh4,
        ))
        .await
        .expect("Competent is exactly what three distinct full-mark cases earn");

    let stored: Progress = borsh::BorshDeserialize::deserialize(
        &mut &banks.get_account(prog).await.unwrap().unwrap().data[..],
    )
    .unwrap();
    assert_eq!(stored.level, Dreyfus::Competent as u8, "the stored level is the computed one");
    assert_eq!(stored.distinct_cases, 3);
    assert_eq!(stored.attempts_counted, 3);
}

/// The reason the funder and the player are two accounts.
///
/// A person who has never touched a crypto wallet has no SOL and is not about to acquire some to
/// try a clinical case. The relay pays; the player signs. What must not happen is the shortcut
/// that was here before — the relay signing *as* the player, which quietly gave every player on a
/// server the same identity, the same claim buffer and the same progress account.
#[tokio::test]
async fn a_player_with_no_sol_can_still_prove_and_claim() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, relay, bh) = pt.start().await;

    // Never funded, never airdropped. It does not exist on chain at all.
    let player = Keypair::new();
    let who = player.pubkey();
    assert!(banks.get_account(who).await.unwrap().is_none(), "the player has no account");

    let (tree, claim, prog) = pdas(&pid, &who);
    let records: Vec<AttemptRecord> = (0..3)
        .map(|i| rec(&who, i as u8 + 1, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();

    let mut bh = bh;
    for r in &records {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, relay.pubkey(), who, &[tree], Instruction::AnchorReplay {
                    tree_id: TREE, record: wire(r) })],
                Some(&relay.pubkey()), &[&relay, &player], bh,
            ))
            .await
            .expect("the relay funds the tree, the player owns the run");
    }

    let leaves: Vec<[u8; 32]> = records.iter().map(|r| r.leaf()).collect();
    for (i, r) in records.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).unwrap();
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, relay.pubkey(), who, &[tree, claim], Instruction::ProveAttempt {
                    tree_id: TREE, record: wire(r), index: i as u64, path: path.to_vec() })],
                Some(&relay.pubkey()), &[&relay, &player], bh,
            ))
            .await
            .expect("proving costs the player nothing");
    }

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, relay.pubkey(), who, &[claim, prog], Instruction::ClaimProgress {
                tree_id: TREE, specialty: 1, claimed: Dreyfus::Competent as u8 })],
            Some(&relay.pubkey()), &[&relay, &player], bh,
        ))
        .await
        .expect("and the level lands on the player, not the relay");

    let stored: Progress = borsh::BorshDeserialize::deserialize(
        &mut &banks.get_account(prog).await.unwrap().unwrap().data[..],
    )
    .unwrap();
    assert_eq!(stored.player, who.to_bytes(), "the progress belongs to the player");
    assert_eq!(stored.level, Dreyfus::Competent as u8);
    assert!(banks.get_account(who).await.unwrap().is_none(), "still holds nothing");

    // And the relay cannot promote itself by paying: its own progress PDA was never created.
    let (_, _, relay_prog) = pdas(&pid, &relay.pubkey());
    assert!(banks.get_account(relay_prog).await.unwrap().is_none(),
            "paying for a run must not earn the payer a level");
}

/// A relay must not be able to anchor a run in somebody's name without them.
#[tokio::test]
async fn the_relay_cannot_sign_for_the_player() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, relay, bh) = pt.start().await;
    let victim = Keypair::new().pubkey();
    let (tree, _, _) = pdas(&pid, &victim);
    let r = rec(&victim, 1, Outcome::WinDischarge, 0, Difficulty::Student);

    // The relay names the victim as the player but only signs for itself.
    let mut metas = vec![
        AccountMeta::new(relay.pubkey(), true),
        AccountMeta::new_readonly(victim, false),
        AccountMeta::new(tree, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];
    metas.dedup();
    let ix = SolIx {
        program_id: pid,
        accounts: metas,
        data: borsh::to_vec(&Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) }).unwrap(),
    };
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix], Some(&relay.pubkey()), &[&relay], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert!(
        matches!(e, TransactionError::InstructionError(_, InstructionError::MissingRequiredSignature)),
        "got {e:?} — the player must sign their own run"
    );
}
