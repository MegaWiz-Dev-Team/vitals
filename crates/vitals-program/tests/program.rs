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
    system_program, system_instruction,
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
        commitment: [7; 32],
        committed_slot: 11,
        rubric_hash: [0u8; 32],
        det_score: 0,
        det_max: 0,
        judged_score: 0,
        judged_max: 0,
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
        rubric_hash: r.rubric_hash,
        det_score: r.det_score,
        det_max: r.det_max,
        judged_score: r.judged_score,
        judged_max: r.judged_max,
    }
}

fn acct(pid: &Pubkey, id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[SEED_ACCOUNT, &id.to_bytes()], pid).0
}

/// The tree belongs to whoever *pays* — that is what stops one operator appending into another's
/// tree — while the claim and the progress belong to whoever *played*. Most of these tests use one
/// key for both, so the two arguments are usually the same; the ones that separate a funding relay
/// from a penniless player are exactly the ones this distinction exists for.
fn pdas(pid: &Pubkey, funder: &Pubkey, who: &Pubkey) -> (Pubkey, Pubkey, Pubkey) {
    let id = TREE.to_le_bytes();
    (
        vitals_program::tree_pda(pid, funder, TREE).0,
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

fn cpda(pid: &Pubkey, who: &Pubkey) -> Pubkey {
    vitals_program::commitment_pda(pid, &who.to_bytes()).0
}

/// Declare an attempt the way a client would: send `Commit`, then read back what the chain
/// actually recorded. The slot is the reason for the read-back — the program assigns it, and the
/// record built for the leaf has to carry the same one, or the local Merkle list forks from the
/// tree and every proof after it fails for a reason that has nothing to do with what a test is
/// trying to show.
async fn committed(
    banks: &mut solana_program_test::BanksClient,
    pid: Pubkey,
    funder: &Keypair,
    device: &Keypair,
    bh: solana_sdk::hash::Hash,
) -> ([u8; 32], u64) {
    let who = device.pubkey();
    let hash = [7u8; 32];
    let mut signers: Vec<&Keypair> = vec![funder];
    if device.pubkey() != funder.pubkey() {
        signers.push(device);
    }
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, funder.pubkey(), who, &[acct(&pid, &who), cpda(&pid, &who)],
                 Instruction::Commit { hash })],
            Some(&funder.pubkey()), &signers, bh,
        ))
        .await
        .expect("commit");
    let data = banks.get_account(cpda(&pid, &who)).await.unwrap().expect("commitment account").data;
    let c: Commitment = borsh::from_slice(&data).expect("commitment layout");
    assert!(c.open && c.hash == hash, "the chain recorded a different commitment");
    (c.hash, c.slot)
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
    let (tree, claim, prog) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
            Some(&me), &[&payer], bh,
        ))
        .await
        .expect("opening an account");

    // ── a claim before anything is proven ──────────────────────────────────
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, claim, prog], Instruction::ClaimProgress { tree_id: TREE, specialty: 1, claimed: 2 })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NoAttempts as u32), "an empty claim must not pass");

    // ── anchoring somebody else's run ─────────────────────────────────────
    // The commitment gate sits before the ownership check, so open one first — otherwise this
    // refusal would be NoCommitment and the test would prove nothing about ownership. The failed
    // transaction rolls back, so the commitment survives for the next stage regardless.
    let bh2 = banks.get_new_latest_blockhash(&bh).await.unwrap();
    committed(&mut banks, pid, &payer, &payer, bh2).await;
    let theirs = rec(&Pubkey::new_unique(), 1, Outcome::WinDischarge, 0, Difficulty::Student);
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
                 Instruction::AnchorReplay { tree_id: TREE, record: wire(&theirs) })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NotYourRun as u32), "a bystander must not fill the tree");

    // ── anchor three of my own, one per case ──────────────────────────────
    let mut mine: Vec<AttemptRecord> = (1..=3)
        .map(|i| rec(&me, i, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();
    // Threaded, not shadowed. `get_new_latest_blockhash(&h)` waits for a hash different from `h`,
    // so asking it repeatedly about the *same* stale `h` can return the same answer every time —
    // and this test later re-sends a transaction it has already sent, expecting the program to
    // refuse it as a duplicate. Reuse the blockhash and the two transactions become byte-identical,
    // the runtime dedupes rather than executing, and the refusal never happens. Under load the
    // ProgramTest tick task is starved, fewer blockhashes appear, and that is exactly when it bit:
    // three failures in four full-workspace runs, and never once when run alone.
    let mut bh = bh;
    let mut leaves = Vec::new();
    for r in &mut mine {
        // One commitment per anchor — each is consumed. The record then carries exactly what the
        // chain recorded, because the leaf the program computes will.
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        let (ch, cs) = committed(&mut banks, pid, &payer, &payer, bh).await;
        r.commitment = ch;
        r.committed_slot = cs;
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
                     Instruction::AnchorReplay { tree_id: TREE, record: wire(r) })],
                Some(&me), &[&payer], bh,
            ))
            .await
            .expect("anchoring my own run");
        leaves.push(r.leaf());
    }

    // ── a record that was never anchored must not prove ───────────────────
    let forged = AttemptRecord { harm_count: 0, outcome: Outcome::WinIcu, ..mine[0] };
    let path = merkle::prove(&leaves, 0).unwrap();
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
                tree_id: TREE, record: wire(&forged), index: 0, path: path.to_vec(),
                commitment: forged.commitment, committed_slot: forged.committed_slot })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::ProofFailed as u32), "a forged leaf must not prove");

    // ── prove all three ───────────────────────────────────────────────────
    for (i, r) in mine.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).unwrap();
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
                    tree_id: TREE, record: wire(r), index: i as u64, path: path.to_vec(),
                commitment: r.commitment, committed_slot: r.committed_slot })],
                Some(&me), &[&payer], bh,
            ))
            .await
            .unwrap_or_else(|e| panic!("proving index {i}: {e:?}"));
    }

    // ── the same run twice ────────────────────────────────────────────────
    let path = merkle::prove(&leaves, 0).unwrap();
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
                tree_id: TREE, record: wire(&mine[0]), index: 0, path: path.to_vec(),
                commitment: mine[0].commitment, committed_slot: mine[0].committed_slot })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::DuplicateAttempt as u32), "one run counts once");

    // ── claim more than the arithmetic allows ─────────────────────────────
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, claim, prog], Instruction::ClaimProgress {
                tree_id: TREE, specialty: 1, claimed: Dreyfus::Expert as u8 })],
            Some(&me), &[&payer], bh,
        ))
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(custom(&e), Some(VitalsError::ClaimNotEarned as u32), "Expert needs eight distinct cases");

    // ── and the claim the evidence does support ───────────────────────────
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, claim, prog], Instruction::ClaimProgress {
                tree_id: TREE, specialty: 1, claimed: Dreyfus::Competent as u8 })],
            Some(&me), &[&payer], bh,
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

    let (tree, claim, prog) = pdas(&pid, &relay.pubkey(), &who);
    let acc = acct(&pid, &who);
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, relay.pubkey(), who, &[acc], Instruction::OpenAccount)],
            Some(&relay.pubkey()), &[&relay, &player], bh,
        ))
        .await
        .expect("the relay opens the account, the player owns it");
    let mut records: Vec<AttemptRecord> = (0..3)
        .map(|i| rec(&who, i as u8 + 1, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();

    let mut bh = bh;
    for r in &mut records {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        let (ch, cs) = committed(&mut banks, pid, &relay, &player, bh).await;
        r.commitment = ch;
        r.committed_slot = cs;
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, relay.pubkey(), who, &[acc, tree, cpda(&pid, &who)],
                     Instruction::AnchorReplay { tree_id: TREE, record: wire(r) })],
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
                &[ix(pid, relay.pubkey(), who, &[acc, tree, claim], Instruction::ProveAttempt {
                    tree_id: TREE, record: wire(r), index: i as u64, path: path.to_vec(),
                commitment: r.commitment, committed_slot: r.committed_slot })],
                Some(&relay.pubkey()), &[&relay, &player], bh,
            ))
            .await
            .expect("proving costs the player nothing");
    }

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, relay.pubkey(), who, &[acc, claim, prog], Instruction::ClaimProgress {
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
    let (_, _, relay_prog) = pdas(&pid, &relay.pubkey(), &relay.pubkey());
    assert!(banks.get_account(relay_prog).await.unwrap().is_none(),
            "paying for a run must not earn the payer a level");
}

/// A relay must not be able to anchor a run in somebody's name without them.
#[tokio::test]
async fn the_relay_cannot_sign_for_the_player() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (banks, relay, bh) = pt.start().await;
    let victim = Keypair::new().pubkey();
    let (tree, _, _) = pdas(&pid, &relay.pubkey(), &victim);
    let acc = acct(&pid, &victim);
    let r = rec(&victim, 1, Outcome::WinDischarge, 0, Difficulty::Student);

    // The relay names the victim as the player but only signs for itself.
    let mut metas = vec![
        AccountMeta::new(relay.pubkey(), true),
        AccountMeta::new_readonly(victim, false),
        AccountMeta::new(acc, false),
        AccountMeta::new(tree, false),
        AccountMeta::new(cpda(&pid, &victim), false),
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

/// The thing this whole layer exists for: play on one machine, see it on another.
#[tokio::test]
async fn a_second_machine_plays_into_the_same_record() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, relay, bh) = pt.start().await;

    let laptop = Keypair::new();          // where they started
    let desktop = Keypair::new();         // where they carried on
    let who = laptop.pubkey();
    let acc = acct(&pid, &who);
    let (tree, claim, prog) = pdas(&pid, &relay.pubkey(), &who);

    // The address of the record is derived from the person, and the person's id is the first
    // device's key — so this is exactly the address the old device-seeded scheme produced.
    // Nothing anchored before this change has to move.
    assert_eq!(
        prog,
        Pubkey::find_program_address(&[SEED_PROGRESS, who.as_ref(), &[1u8]], &pid).0,
        "changing to accounts must not move anybody's progress"
    );

    let mut bh = bh;
    let go = |ixs: Vec<SolIx>, signers: Vec<&Keypair>, bh| {
        Transaction::new_signed_with_payer(&ixs, Some(&relay.pubkey()), &signers, bh)
    };

    banks.process_transaction(go(
        vec![ix(pid, relay.pubkey(), who, &[acc], Instruction::OpenAccount)],
        vec![&relay, &laptop], bh)).await.expect("open");

    // The desktop has a key of its own and no standing at all yet.
    let e = banks
        .process_transaction(go(
            vec![ix(pid, relay.pubkey(), desktop.pubkey(), &[acc, claim, prog],
                    Instruction::ClaimProgress { tree_id: TREE, specialty: 1, claimed: 1 })],
            vec![&relay, &desktop], bh))
        .await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NotAuthorized as u32),
               "an unlinked machine is a stranger");

    // Two cases played on the laptop.
    let mut records: Vec<AttemptRecord> = (0..2)
        .map(|i| rec(&who, i as u8 + 1, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();
    for r in &mut records {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        let (ch, cs) = committed(&mut banks, pid, &relay, &laptop, bh).await;
        r.commitment = ch;
        r.committed_slot = cs;
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks.process_transaction(go(
            vec![ix(pid, relay.pubkey(), who, &[acc, tree, cpda(&pid, &who)],
                    Instruction::AnchorReplay { tree_id: TREE, record: wire(r) })],
            vec![&relay, &laptop], bh)).await.expect("anchor on the laptop");
    }

    // Link the desktop, signed from the laptop.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(go(
        vec![ix(pid, relay.pubkey(), who, &[acc],
                Instruction::AddAuthority { device: desktop.pubkey().to_bytes() })],
        vec![&relay, &laptop], bh)).await.expect("linking the desktop");

    // And now the desktop proves the laptop's runs — same tree, same claim buffer, same person.
    let leaves: Vec<[u8; 32]> = records.iter().map(|r| r.leaf()).collect();
    for (i, r) in records.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).unwrap();
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks.process_transaction(go(
            vec![ix(pid, relay.pubkey(), desktop.pubkey(), &[acc, tree, claim],
                    Instruction::ProveAttempt { tree_id: TREE, record: wire(r),
                                                index: i as u64, path: path.to_vec(),
                commitment: r.commitment, committed_slot: r.committed_slot })],
            vec![&relay, &desktop], bh)).await.expect("the desktop proves the laptop's run");
    }

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(go(
        vec![ix(pid, relay.pubkey(), desktop.pubkey(), &[acc, claim, prog],
                Instruction::ClaimProgress { tree_id: TREE, specialty: 1, claimed: 1 })],
        vec![&relay, &desktop], bh)).await.expect("claiming from the second machine");

    let stored: Progress = borsh::BorshDeserialize::deserialize(
        &mut &banks.get_account(prog).await.unwrap().unwrap().data[..]).unwrap();
    assert_eq!(stored.player, who.to_bytes(), "the record belongs to the person, not the machine");
    assert_eq!(stored.attempts_counted, 2, "both laptop runs count on the desktop");
    assert_eq!(stored.distinct_cases, 2);
}

/// A device you no longer have should stop counting — but the last one cannot go, or the record
/// becomes unreachable forever.
#[tokio::test]
async fn devices_can_be_dropped_but_never_the_last_one() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, relay, bh) = pt.start().await;
    let owner = Keypair::new();
    let lost = Keypair::new();
    let stranger = Keypair::new();
    let who = owner.pubkey();
    let acc = acct(&pid, &who);
    let tx = |ixs: Vec<SolIx>, signers: Vec<&Keypair>, bh| {
        Transaction::new_signed_with_payer(&ixs, Some(&relay.pubkey()), &signers, bh)
    };

    let mut bh = bh;
    banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), who, &[acc], Instruction::OpenAccount)],
        vec![&relay, &owner], bh)).await.expect("open");

    // A stranger cannot write themselves in.
    let e = banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), stranger.pubkey(), &[acc],
                Instruction::AddAuthority { device: stranger.pubkey().to_bytes() })],
        vec![&relay, &stranger], bh)).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NotAuthorized as u32));

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), who, &[acc],
                Instruction::AddAuthority { device: lost.pubkey().to_bytes() })],
        vec![&relay, &owner], bh)).await.expect("add");

    // Adding the same machine twice is a mistake worth naming, not a silent no-op.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), who, &[acc],
                Instruction::AddAuthority { device: lost.pubkey().to_bytes() })],
        vec![&relay, &owner], bh)).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::AlreadyAuthorized as u32));

    // The lost laptop goes.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), who, &[acc],
                Instruction::RemoveAuthority { device: lost.pubkey().to_bytes() })],
        vec![&relay, &owner], bh)).await.expect("remove");

    // And it really is gone.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), lost.pubkey(), &[acc],
                Instruction::AddAuthority { device: stranger.pubkey().to_bytes() })],
        vec![&relay, &lost], bh)).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NotAuthorized as u32));

    // The last one stays, whatever the owner thinks they want.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(tx(
        vec![ix(pid, relay.pubkey(), who, &[acc],
                Instruction::RemoveAuthority { device: who.to_bytes() })],
        vec![&relay, &owner], bh)).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::LastAuthority as u32),
               "removing it would leave a record nobody can ever claim against");
}

/// A stranger must not be able to append into somebody else's anchoring tree.
///
/// This is the failure the PDA change exists to close, and it needed no collision to reach: the
/// tree address came from `tree_id` alone, `tree_id` is the slot the server booted in, and the
/// server prints it on its own status line. Anyone who read that number could name the tree and
/// add a leaf to it — the root moves, and the operator's tree now contains records they cannot
/// account for. Deriving the address from the funder makes a foreign tree unreachable rather than
/// merely unlikely to be hit by accident.
#[tokio::test]
async fn a_stranger_cannot_append_to_my_tree() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, mine, bh) = pt.start().await;
    let me = mine.pubkey();

    // My tree, with one honest leaf in it.
    let (tree, _, _) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);
    let mut tx = Transaction::new_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me),
    );
    tx.sign(&[&mine], bh);
    banks.process_transaction(tx).await.expect("open");
    let bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let (ch, cs) = committed(&mut banks, pid, &mine, &mine, bh).await;
    let mut r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    r.commitment = ch;
    r.committed_slot = cs;
    let bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let mut tx = Transaction::new_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me),
    );
    tx.sign(&[&mine], bh);
    banks.process_transaction(tx).await.expect("my own anchor");

    // Somebody else, funded, who knows the tree id — which is all they ever needed to know.
    let stranger = Keypair::new();
    let them = stranger.pubkey();
    let fund = system_instruction::transfer(&me, &them, 10_000_000_000);
    let mut tx = Transaction::new_with_payer(&[fund], Some(&me));
    tx.sign(&[&mine], bh);
    banks.process_transaction(tx).await.expect("fund the stranger");

    let their_acc = acct(&pid, &them);
    let mut tx = Transaction::new_with_payer(
        &[ix(pid, them, them, &[their_acc], Instruction::OpenAccount)],
        Some(&them),
    );
    tx.sign(&[&stranger], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.expect("the stranger's own account");
    // The stranger commits properly too. Without this their attempt would be refused for the
    // missing commitment, and the test would prove nothing about tree ownership.
    let bh3 = banks.get_latest_blockhash().await.unwrap();
    let (sch, scs) = committed(&mut banks, pid, &stranger, &stranger, bh3).await;
    let mut theirs = rec(&them, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    theirs.commitment = sch;
    theirs.committed_slot = scs;
    let mut tx = Transaction::new_with_payer(
        // They name *my* tree account, with my tree id — with their own commitment open.
        &[ix(pid, them, them, &[their_acc, tree, cpda(&pid, &them)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&theirs) })],
        Some(&them),
    );
    tx.sign(&[&stranger], banks.get_latest_blockhash().await.unwrap());
    assert!(
        banks.process_transaction(tx).await.is_err(),
        "a stranger appended a leaf into my tree"
    );
}

/// Substituting one account for another is the attack this program refuses most often, and until
/// now the refusal had no test at all.
///
/// `WrongPda` is raised at six places — twice for the account, twice for the tree, once each for
/// the claim buffer and the progress record — and every one of them guards the same thing: an
/// instruction that names a *real* account of the *right shape* which simply is not the one it is
/// entitled to touch. It is also the class the tree bug belonged to, where the check was present
/// and derived from the wrong seed. A present check with no test is how that survives.
#[tokio::test]
async fn naming_an_account_that_is_not_yours_is_refused_everywhere() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, claim, _prog) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);

    // A second player whose account genuinely exists. That detail is the test: an account that
    // was never created is caught earlier and by a different name — `NoAccount` — so substituting
    // a non-existent one never reaches the check being exercised here.
    let other_kp = Keypair::new();
    let other = other_kp.pubkey();
    let (other_tree, other_claim, other_prog) = pdas(&pid, &other, &other);
    let other_acc = acct(&pid, &other);

    banks.process_transaction(Transaction::new_signed_with_payer(
        &[
            ix(pid, me, me, &[acc], Instruction::OpenAccount),
            ix(pid, me, other, &[other_acc], Instruction::OpenAccount),
        ],
        Some(&me), &[&payer, &other_kp], bh,
    )).await.expect("open both");

    let mut bh = bh;
    // A commitment first — the gate sits before most of the refusals under test, and each failed
    // substitution rolls back, so one open declaration serves every doomed attempt below and is
    // finally consumed by the honest anchor.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let (ch, cs) = committed(&mut banks, pid, &payer, &payer, bh).await;
    let mut r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    r.commitment = ch;
    r.committed_slot = cs;

    // Someone else's account record, in place of mine.
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[other_acc, tree, cpda(&pid, &other)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    // NotAuthorized, not WrongPda, and the difference is the design: the PDA check in
    // `authorised` compares the account's *stored* id against the address it lives at, so it
    // catches an account whose contents do not match its own location. A legitimate account
    // belonging to someone else passes that check honestly and is stopped one line later, by not
    // listing this device. Both are refusals; naming the right one is what makes the test worth
    // having.
    assert_eq!(custom(&e), Some(VitalsError::NotAuthorized as u32), "a foreign account record");

    // Someone else's tree, in place of mine.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, other_tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::WrongPda as u32), "a foreign tree");

    // Anchor honestly, so the later substitutions fail on the account they name rather than on
    // there being nothing to prove.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&payer], bh,
    )).await.expect("anchor");

    // Someone else's claim buffer, in place of mine.
    let path = merkle::prove(&[r.leaf()], 0).unwrap();
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, other_claim], Instruction::ProveAttempt {
            tree_id: TREE, record: wire(&r), index: 0, path: path.to_vec(),
                commitment: r.commitment, committed_slot: r.committed_slot })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::WrongPda as u32), "a foreign claim buffer");

    // Prove honestly, then aim the claim at someone else's progress record.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
            tree_id: TREE, record: wire(&r), index: 0, path: path.to_vec(),
                commitment: r.commitment, committed_slot: r.committed_slot })],
        Some(&me), &[&payer], bh,
    )).await.expect("prove");

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, claim, other_prog], Instruction::ClaimProgress {
            tree_id: TREE, specialty: 1, claimed: Dreyfus::Novice as u8 })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::WrongPda as u32), "a foreign progress record");
}

/// Acting before opening an account is refused by name rather than by accident.
#[tokio::test]
async fn a_device_with_no_account_cannot_anchor() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, _, _) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);
    let r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);

    // The account PDA is correct — it simply has never been created. The commitment account is
    // named too, so the refusal is the account's absence and not a short account list.
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NoAccount as u32));
}

/// The device list is capped, and the cap is enforced rather than silently truncating.
///
/// A truncating write would be the dangerous version: a learner adds a ninth machine, is told
/// nothing, and finds the record unreachable from it.
#[tokio::test]
async fn the_device_list_refuses_a_ninth_machine() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let acc = acct(&pid, &me);

    let mut bh = bh;
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&payer], bh,
    )).await.expect("open");

    // Opening writes the first authority, so MAX_AUTHORITIES - 1 more are allowed.
    for i in 1..MAX_AUTHORITIES {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        let device = Pubkey::new_unique();
        banks.process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc],
                 Instruction::AddAuthority { device: device.to_bytes() })],
            Some(&me), &[&payer], bh,
        )).await.unwrap_or_else(|e| panic!("adding device {i}: {e:?}"));
    }

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc],
             Instruction::AddAuthority { device: Pubkey::new_unique().to_bytes() })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::AuthoritiesFull as u32));
}

/// A record whose enums are out of range is refused, not coerced.
///
/// `difficulty` and `outcome` arrive as raw bytes over the wire, so a client — buggy, or a second
/// implementation of this protocol reading the spec differently — can send a value no variant
/// answers to. Clamping it to the nearest valid one would anchor a record that says something the
/// player never did, permanently, in a structure whose entire purpose is to be trusted later.
#[tokio::test]
async fn a_record_with_an_impossible_difficulty_or_outcome_is_refused() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, _, _) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);

    let mut bh = bh;
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&payer], bh,
    )).await.expect("open");

    // One commitment covers both bad attempts: each transaction fails at decode and rolls back,
    // which un-consumes it — the same rollback that stops a failed anchor eating a declaration.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    committed(&mut banks, pid, &payer, &payer, bh).await;

    for (field, w) in [
        ("difficulty", {
            let mut w = wire(&rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student));
            w.difficulty = 9;
            w
        }),
        ("outcome", {
            let mut w = wire(&rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student));
            w.outcome = 200;
            w
        }),
    ] {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        let e = banks.process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
                 Instruction::AnchorReplay { tree_id: TREE, record: w })],
            Some(&me), &[&payer], bh,
        )).await.unwrap_err().unwrap();
        assert_eq!(custom(&e), Some(VitalsError::BadRecord as u32), "{field} out of range");
    }
}

/// The gate itself: a run that was never declared cannot be anchored.
#[tokio::test]
async fn anchoring_without_a_commitment_is_refused() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, _, _) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);

    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&payer], bh,
    )).await.expect("open");

    let r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    let bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NoCommitment as u32));
}

/// One declaration covers one anchor. The second run needs its own.
#[tokio::test]
async fn a_commitment_is_consumed_by_the_anchor_it_covers() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, _, _) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);

    let mut bh = bh;
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&payer], bh,
    )).await.expect("open");

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let (ch, cs) = committed(&mut banks, pid, &payer, &payer, bh).await;
    let mut r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    r.commitment = ch;
    r.committed_slot = cs;

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&payer], bh,
    )).await.expect("the committed anchor");

    // The same declaration must not stretch to a second run.
    let r2 = rec(&me, 2, Outcome::WinDischarge, 0, Difficulty::Student);
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r2) })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::NoCommitment as u32), "a spent commitment was reused");
}

/// The trap the whole mechanism exists to close: closing commitments must not erase the count.
///
/// Without this, a learner commits five times, plays five, anchors the flattering run and closes
/// the rest — and the chain says they attempted once. Rent is refundable; the fact is not.
#[tokio::test]
async fn closing_a_commitment_keeps_the_count() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let acc = acct(&pid, &me);
    let commit = cpda(&pid, &me);

    let mut bh = bh;
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&payer], bh,
    )).await.expect("open");

    // Three declarations — the middle one overwritten, which still counts, because every
    // statement of intent is on the record whether or not it became a run.
    for h in [[1u8; 32], [2u8; 32], [3u8; 32]] {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks.process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, commit], Instruction::Commit { hash: h })],
            Some(&me), &[&payer], bh,
        )).await.expect("commit");
    }
    let c: Commitment = borsh::from_slice(
        &banks.get_account(commit).await.unwrap().unwrap().data).unwrap();
    assert_eq!(c.started, 3);
    let lamports_before = banks.get_account(commit).await.unwrap().unwrap().lamports;

    // Close it. The open slot clears; the count and the account survive.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, commit], Instruction::CloseCommitment)],
        Some(&me), &[&payer], bh,
    )).await.expect("close");

    let after = banks.get_account(commit).await.unwrap().expect("the account must survive a close");
    let c: Commitment = borsh::from_slice(&after.data).unwrap();
    assert!(!c.open, "closing must clear the open slot");
    assert_eq!(c.started, 3, "closing erased the count — the whole mechanism is decoration");
    assert_eq!(after.lamports, lamports_before, "lamports moved on close; the counter is buyable");

    // And the record keeps growing from where it left off.
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, commit], Instruction::Commit { hash: [4u8; 32] })],
        Some(&me), &[&payer], bh,
    )).await.expect("commit after close");
    let c: Commitment = borsh::from_slice(
        &banks.get_account(commit).await.unwrap().unwrap().data).unwrap();
    assert_eq!(c.started, 4);
}

/// All-zero is the sentinel for "nothing open" and must not be committable as a real value.
#[tokio::test]
async fn a_zero_commitment_is_refused() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let acc = acct(&pid, &me);

    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&payer], bh,
    )).await.expect("open");

    let bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, cpda(&pid, &me)], Instruction::Commit { hash: [0u8; 32] })],
        Some(&me), &[&payer], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::BadRecord as u32));
}

/// Fund a keypair we control so it can be the funder/signer, since ProgramTest's own payer is a
/// random key and the tree PDA is derived from whoever funds. Seeding our own operator lets the
/// pre-seeded fixtures below be derived from a key we hold.
fn fund(pt: &mut ProgramTest, who: &Pubkey) {
    pt.add_account(*who, solana_sdk::account::Account {
        lamports: 100_000_000_000,
        data: vec![],
        owner: system_program::id(),
        executable: false,
        rent_epoch: 0,
    });
}

/// Pre-seed an account the program will read, so a full/foreign state can be reached without
/// paying for thousands of real operations to build it.
fn seed(pt: &mut ProgramTest, key: Pubkey, owner: Pubkey, mut data: Vec<u8>, len: usize) {
    data.resize(len, 0);
    pt.add_account(
        key,
        solana_sdk::account::Account {
            lamports: 1_000_000_000,
            data,
            owner,
            executable: false,
            rent_epoch: 0,
        },
    );
}

/// A tree at capacity refuses the next leaf rather than wrapping or overwriting.
///
/// MAX_LEAVES is 4,096; anchoring that many to reach the edge honestly is not a test, it is a
/// denial-of-service on the suite. The edge state is pre-seeded instead — a tree whose next index
/// is already MAX_LEAVES — and the one thing under test is that the next append is refused.
#[tokio::test]
async fn a_full_tree_refuses_another_leaf() {
    let pid = Pubkey::new_unique();
    let mut pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let op = Keypair::new();
    let me = op.pubkey();
    fund(&mut pt, &me);

    let (tree, _, _) = pdas(&pid, &me, &me);
    let full = TreeAccount {
        root: [0; 32],
        next_index: vitals_progress::merkle::MAX_LEAVES,
        filled: [[0; 32]; vitals_progress::merkle::DEPTH],
    };
    seed(&mut pt, tree, pid, borsh::to_vec(&full).unwrap(), TREE_LEN);

    let (mut banks, _p, bh) = pt.start().await;

    let mut bh = bh;
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acct(&pid, &me)], Instruction::OpenAccount)],
        Some(&me), &[&op], bh,
    )).await.expect("open");

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let (ch, cs) = committed(&mut banks, pid, &op, &op, bh).await;
    let mut r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    r.commitment = ch;
    r.committed_slot = cs;

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acct(&pid, &me), tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&op], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::TreeFull as u32));
}

/// A claim buffer at capacity refuses the seventeenth proof.
///
/// Same reasoning: sixteen honest anchor-and-prove cycles to fill it is cost without insight, so
/// the full buffer is pre-seeded with sixteen distinct proven attempts and the test anchors and
/// proves one more real run. The proof itself must pass — the leaf really is in the tree — so
/// what is under test is precisely the capacity refusal and not some earlier failure.
#[tokio::test]
async fn a_full_claim_buffer_refuses_another_proof() {
    let pid = Pubkey::new_unique();
    let mut pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let op = Keypair::new();
    let me = op.pubkey();
    fund(&mut pt, &me);

    let (_, claim, _) = pdas(&pid, &me, &me);
    let attempts: Vec<ProvenAttempt> = (0..CLAIM_CAPACITY as u8)
        .map(|i| ProvenAttempt {
            leaf: [100 + i; 32], // distinct, and none equal to the real leaf below
            case: [0; 32], score: 100, max: 100, det_score: 0, det_max: 0, difficulty: 0, exam_mode: false,
        })
        .collect();
    let full = ClaimAccount { player: me.to_bytes(), count: CLAIM_CAPACITY as u8, attempts };
    seed(&mut pt, claim, pid, borsh::to_vec(&full).unwrap(), CLAIM_LEN);

    let (mut banks, _p, bh) = pt.start().await;
    let (tree, _, _) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);

    let mut bh = bh;
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
        Some(&me), &[&op], bh,
    )).await.expect("open");

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let (ch, cs) = committed(&mut banks, pid, &op, &op, bh).await;
    let mut r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    r.commitment = ch;
    r.committed_slot = cs;

    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
             Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) })],
        Some(&me), &[&op], bh,
    )).await.expect("anchor");

    let path = merkle::prove(&[r.leaf()], 0).unwrap();
    bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
            tree_id: TREE, record: wire(&r), index: 0, path: path.to_vec(),
            commitment: r.commitment, committed_slot: r.committed_slot })],
        Some(&me), &[&op], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::ClaimFull as u32));
}

/// An account the program is handed but does not own is refused.
///
/// The address is the right one — it is the account PDA for this player — but it is owned by the
/// system program, not by us. A program that read it anyway would trust bytes an attacker could
/// have written. `authorised` checks ownership before it reads.
#[tokio::test]
async fn an_account_owned_by_another_program_is_refused() {
    let pid = Pubkey::new_unique();
    let mut pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let op = Keypair::new();
    let me = op.pubkey();
    fund(&mut pt, &me);

    // A well-formed Account, at the right PDA, but owned by the system program.
    let acc = acct(&pid, &me);
    let plausible = Account { id: me.to_bytes(), authorities: vec![me.to_bytes()] };
    seed(&mut pt, acc, system_program::id(), borsh::to_vec(&plausible).unwrap(), ACCOUNT_LEN);

    let (banks, _p, bh) = pt.start().await;

    let e = banks.process_transaction(Transaction::new_signed_with_payer(
        &[ix(pid, me, me, &[acc],
             Instruction::AddAuthority { device: Pubkey::new_unique().to_bytes() })],
        Some(&me), &[&op], bh,
    )).await.unwrap_err().unwrap();
    assert_eq!(custom(&e), Some(VitalsError::WrongOwner as u32));
}

/// An `AttemptRecord` on `case`, scored `det_score`/`det_max` on its rubric, exam or practice.
fn exam_rec(player: &Pubkey, case: u8, det_score: u16, det_max: u16, exam: bool) -> AttemptRecord {
    let mut r = rec(player, case, Outcome::WinDischarge, 0, Difficulty::Student);
    r.exam_mode = exam;
    r.det_score = det_score;
    r.det_max = det_max;
    r
}

/// The star gate's core claim, end to end on chain: an exam run's deterministic score is persisted
/// in the *claim buffer* (not only the leaf), and a star is measured on that det score — so a
/// cleared exam earns one, a sub-bar exam does not, and a practice run never can whatever its
/// outcome. This is the on-chain half of the fix that stopped stars being measured on the outcome.
#[tokio::test]
async fn an_exam_run_persists_its_det_score_and_earns_a_star() {
    let pid = Pubkey::new_unique();
    let pt = ProgramTest::new("vitals_program", pid, processor!(process_instruction));
    let (mut banks, payer, bh) = pt.start().await;
    let me = payer.pubkey();
    let (tree, claim, _prog) = pdas(&pid, &me, &me);
    let acc = acct(&pid, &me);
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc], Instruction::OpenAccount)],
            Some(&me),
            &[&payer],
            bh,
        ))
        .await
        .expect("opening an account");

    // Three distinct cases: an exam cleared at det 40/40, an exam below the 70% bar at 24/40, and a
    // practice run with a perfect outcome. Only the first is a star.
    let mut runs: Vec<AttemptRecord> = vec![
        exam_rec(&me, 1, 40, 40, true),
        exam_rec(&me, 2, 24, 40, true),
        exam_rec(&me, 3, 40, 40, false),
    ];

    let mut bh = bh;
    let mut leaves = Vec::new();
    for r in &mut runs {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        let (ch, cs) = committed(&mut banks, pid, &payer, &payer, bh).await;
        r.commitment = ch;
        r.committed_slot = cs;
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, me, me, &[acc, tree, cpda(&pid, &me)],
                     Instruction::AnchorReplay { tree_id: TREE, record: wire(r) })],
                Some(&me),
                &[&payer],
                bh,
            ))
            .await
            .expect("anchoring an exam run");
        leaves.push(r.leaf());
    }
    for (i, r) in runs.iter().enumerate() {
        let path = merkle::prove(&leaves, i as u64).unwrap();
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
                    tree_id: TREE,
                    record: wire(r),
                    index: i as u64,
                    path: path.to_vec(),
                    commitment: r.commitment,
                    committed_slot: r.committed_slot,
                })],
                Some(&me),
                &[&payer],
                bh,
            ))
            .await
            .expect("proving an exam run");
    }

    // Read the claim buffer back: det must be persisted per attempt, and the star measured on it.
    let data = banks.get_account(claim).await.unwrap().expect("claim account").data;
    // deserialize, not from_slice: the account is CLAIM_LEN-padded, so the Vec is followed by zero
    // bytes the struct does not read — from_slice would reject them as "not all bytes read".
    let claim_acct: ClaimAccount =
        borsh::BorshDeserialize::deserialize(&mut &data[..]).expect("claim layout");
    assert_eq!(claim_acct.attempts.len(), 3);
    let cleared = claim_acct.attempts.iter().find(|a| a.case[0] == 1).unwrap();
    assert_eq!(
        (cleared.det_score, cleared.det_max),
        (40, 40),
        "the det score must survive into the claim buffer, not only the leaf"
    );

    let attempts: Vec<vitals_progress::Attempt> = claim_acct
        .attempts
        .iter()
        .map(|a| vitals_progress::Attempt {
            case: a.case,
            score: a.score,
            max: a.max,
            det_score: a.det_score,
            det_max: a.det_max,
            difficulty: Difficulty::Student,
            exam_mode: a.exam_mode,
        })
        .collect();
    assert_eq!(
        vitals_progress::stars(&attempts, vitals_progress::STAR_PASS_BPS),
        1,
        "only the cleared exam case is a star — not the sub-bar exam, not the practice run"
    );
}
