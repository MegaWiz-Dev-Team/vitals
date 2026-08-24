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
    let theirs = rec(&Pubkey::new_unique(), 1, Outcome::WinDischarge, 0, Difficulty::Student);
    let e = banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, me, me, &[acc, tree], Instruction::AnchorReplay { tree_id: TREE, record: wire(&theirs) })],
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
                &[ix(pid, me, me, &[acc, tree], Instruction::AnchorReplay { tree_id: TREE, record: wire(r) })],
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
            &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
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
                &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
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
            &[ix(pid, me, me, &[acc, tree, claim], Instruction::ProveAttempt {
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
            &[ix(pid, me, me, &[acc, claim, prog], Instruction::ClaimProgress {
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
            &[ix(pid, me, me, &[acc, claim, prog], Instruction::ClaimProgress {
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

    let (tree, claim, prog) = pdas(&pid, &relay.pubkey(), &who);
    let acc = acct(&pid, &who);
    banks
        .process_transaction(Transaction::new_signed_with_payer(
            &[ix(pid, relay.pubkey(), who, &[acc], Instruction::OpenAccount)],
            Some(&relay.pubkey()), &[&relay, &player], bh,
        ))
        .await
        .expect("the relay opens the account, the player owns it");
    let records: Vec<AttemptRecord> = (0..3)
        .map(|i| rec(&who, i as u8 + 1, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();

    let mut bh = bh;
    for r in &records {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks
            .process_transaction(Transaction::new_signed_with_payer(
                &[ix(pid, relay.pubkey(), who, &[acc, tree], Instruction::AnchorReplay {
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
                &[ix(pid, relay.pubkey(), who, &[acc, tree, claim], Instruction::ProveAttempt {
                    tree_id: TREE, record: wire(r), index: i as u64, path: path.to_vec() })],
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
    let (mut banks, relay, bh) = pt.start().await;
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
    let mut go = |ixs: Vec<SolIx>, signers: Vec<&Keypair>, bh| {
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
    let records: Vec<AttemptRecord> = (0..2)
        .map(|i| rec(&who, i as u8 + 1, Outcome::WinDischarge, 0, Difficulty::Student))
        .collect();
    for r in &records {
        bh = banks.get_new_latest_blockhash(&bh).await.unwrap();
        banks.process_transaction(go(
            vec![ix(pid, relay.pubkey(), who, &[acc, tree],
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
                                                index: i as u64, path: path.to_vec() })],
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
    let r = rec(&me, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    let mut tx = Transaction::new_with_payer(
        &[
            ix(pid, me, me, &[acc], Instruction::OpenAccount),
            ix(pid, me, me, &[acc, tree], Instruction::AnchorReplay { tree_id: TREE, record: wire(&r) }),
        ],
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
    let theirs = rec(&them, 1, Outcome::WinDischarge, 0, Difficulty::Student);
    let mut tx = Transaction::new_with_payer(
        &[
            ix(pid, them, them, &[their_acc], Instruction::OpenAccount),
            // They name *my* tree account, with my tree id.
            ix(pid, them, them, &[their_acc, tree], Instruction::AnchorReplay { tree_id: TREE, record: wire(&theirs) }),
        ],
        Some(&them),
    );
    tx.sign(&[&stranger], banks.get_latest_blockhash().await.unwrap());
    assert!(
        banks.process_transaction(tx).await.is_err(),
        "a stranger appended a leaf into my tree"
    );
}
