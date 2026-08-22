//! The chain half of the app: anchor a finished run, prove it, claim a level.
//!
//! Same three instructions the CLI driver sends. The server is a **fee-payer relay**: it holds
//! SOL and pays, and it holds no player key at all. The player's key lives in their browser and
//! signs there, so a nineteen-year-old learning medicine never buys SOL and never installs a
//! wallet — and never hands their credential's key to a server either.
//!
//! It used to sign both halves with one keypair. That did not just centralise custody, it
//! collapsed identity: PDAs are seeded on the player, so every player on a server shared one
//! claim buffer and one progress account, and each person's level was the whole server's level.

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction as SolInstruction},
    pubkey::Pubkey,
    message::Message,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;
use vitals_progress::merkle;
use vitals_progress::record::AttemptRecord;
use vitals_progress::Difficulty;
use vitals_program::{
    ClaimAccount, Instruction, Progress, RecordWire, TreeAccount, SEED_CLAIM, SEED_PROGRESS,
    SEED_TREE,
};

pub const SPECIALTY: u8 = 1;

pub struct Chain {
    rpc: RpcClient,
    program_id: Pubkey,
    /// Pays. Never plays.
    relay: Keypair,
}

/// A transaction the relay has signed and the player has not.
///
/// The bytes in `message` are exactly what the player's key must sign — the same bytes the relay
/// signed. The server cannot produce that signature, which is the entire point.
pub struct Pending {
    tx: Transaction,
    slot: usize,
}

impl Pending {
    pub fn message(&self) -> Vec<u8> {
        self.tx.message_data()
    }

    /// Drop the player's signature into its slot and hand back a transaction that will verify.
    pub fn signed(mut self, sig: &[u8; 64]) -> Result<Transaction, String> {
        self.tx.signatures[self.slot] = Signature::from(*sig);
        self.tx
            .verify()
            .map_err(|_| "that signature does not match this transaction".to_string())?;
        Ok(self.tx)
    }
}

pub struct Anchored {
    pub index: u64,
    pub root: String,
    pub leaves: u64,
    pub proven: bool,
}

impl Chain {
    /// `None` when the validator or the program id is not configured — the app still plays, it
    /// just cannot anchor, and the UI says so rather than pretending.
    pub fn connect() -> Option<Chain> {
        let url = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
        let program_id = Pubkey::from_str(&std::env::var("VITALS_PROGRAM_ID").ok()?).ok()?;
        let path = std::env::var("VITALS_KEYPAIR")
            .unwrap_or_else(|_| format!("{}/.config/solana/id.json", std::env::var("HOME").ok().unwrap_or_default()));
        let relay = read_keypair_file(path).ok()?;
        let rpc = RpcClient::new_with_commitment(url, CommitmentConfig::confirmed());
        rpc.get_slot().ok()?;
        Some(Chain { rpc, program_id, relay })
    }

    pub fn slot(&self) -> u64 {
        self.rpc.get_slot().unwrap_or(0)
    }

    fn tree_pda(&self, tree_id: u64) -> Pubkey {
        Pubkey::find_program_address(&[SEED_TREE, &tree_id.to_le_bytes()], &self.program_id).0
    }
    /// Seeded on the *player*, so two players on one server never share a buffer.
    pub fn claim_pda(&self, player: &Pubkey, tree_id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[SEED_CLAIM, player.as_ref(), &tree_id.to_le_bytes()],
            &self.program_id,
        )
        .0
    }
    pub fn progress_pda(&self, player: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[SEED_PROGRESS, player.as_ref(), &[SPECIALTY]],
            &self.program_id,
        )
        .0
    }

    pub fn relay_pubkey(&self) -> String {
        self.relay.pubkey().to_string()
    }

    fn ix(&self, player: &Pubkey, ix: Instruction, extra: &[Pubkey]) -> Result<SolInstruction, String> {
        let mut metas = vec![
            AccountMeta::new(self.relay.pubkey(), true),
            AccountMeta::new_readonly(*player, true),
        ];
        metas.extend(extra.iter().map(|k| AccountMeta::new(*k, false)));
        metas.push(AccountMeta::new_readonly(system_program::id(), false));
        Ok(SolInstruction {
            program_id: self.program_id,
            accounts: metas,
            data: borsh::to_vec(&ix).map_err(|e| e.to_string())?,
        })
    }

    /// Assemble, sign the relay's half, and leave the player's slot empty.
    fn prepare(&self, player: &Pubkey, ixs: Vec<SolInstruction>) -> Result<Pending, String> {
        let bh = self.rpc.get_latest_blockhash().map_err(|e| e.to_string())?;
        let msg = Message::new_with_blockhash(&ixs, Some(&self.relay.pubkey()), &bh);
        let slot = msg
            .account_keys
            .iter()
            .position(|k| k == player)
            .ok_or("the player is not a signer on this transaction")?;
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_partial_sign(&[&self.relay], bh).map_err(|e| e.to_string())?;
        Ok(Pending { tx, slot })
    }

    pub fn submit(&self, tx: &Transaction) -> Result<(), String> {
        self.rpc
            .send_and_confirm_transaction(tx)
            .map(|_| ())
            .map_err(|e| program_log(&e.to_string()))
    }

    /// Anchor one finished run and prove it in the same transaction.
    ///
    /// One transaction, so one signature from the player rather than two round trips through the
    /// browser — and atomically: a leaf that lands in the tree but never gets proven is a leaf the
    /// player paid for and cannot use.
    pub fn prepare_anchor(
        &self,
        player: &Pubkey,
        tree_id: u64,
        rec: &AttemptRecord,
        leaves: &[[u8; 32]],
    ) -> Result<Pending, String> {
        let index = leaves.len() as u64 - 1;
        let path = merkle::prove(leaves, index).ok_or("could not build the proof")?;
        self.prepare(
            player,
            vec![
                self.ix(player, Instruction::AnchorReplay { tree_id, record: wire(rec) },
                        &[self.tree_pda(tree_id)])?,
                self.ix(player,
                        Instruction::ProveAttempt { tree_id, record: wire(rec), index, path: path.to_vec() },
                        &[self.tree_pda(tree_id), self.claim_pda(player, tree_id)])?,
            ],
        )
    }

    /// What the tree looks like now. Read after the transaction confirms.
    pub fn anchored(&self, player: &Pubkey, tree_id: u64, index: u64) -> Result<Anchored, String> {
        let tree: TreeAccount = self.fetch(&self.tree_pda(tree_id)).ok_or("tree missing")?;
        Ok(Anchored {
            index,
            root: hex32(&tree.root),
            leaves: tree.next_index,
            proven: self.proven_count(player, tree_id) > 0,
        })
    }

    /// Claim a level. `Ok(msg)` when the program grants it, `Err(msg)` when it recomputes and
    /// refuses — and the refusal carries the program's own log line, because that is the point.
    pub fn prepare_claim(&self, player: &Pubkey, tree_id: u64, level: u8) -> Result<Pending, String> {
        self.prepare(
            player,
            vec![self.ix(
                player,
                Instruction::ClaimProgress { tree_id, specialty: SPECIALTY, claimed: level },
                &[self.claim_pda(player, tree_id), self.progress_pda(player)],
            )?],
        )
    }

    /// Read the level back off chain after a claim lands.
    ///
    /// The level reported is the one stored in the account, which is the one the *program*
    /// computed — not the one the player asked for. Echoing the request back would make the UI
    /// agree with every claim, including the ones the chain refused.
    pub fn claimed(&self, player: &Pubkey) -> Result<String, String> {
        let p: Progress = self
            .fetch(&self.progress_pda(player))
            .ok_or("the claim landed but no progress account was written")?;
        Ok(format!(
            "granted · level {} · {} attempts · {} distinct · xp {}",
            level_name(p.level), p.attempts_counted, p.distinct_cases, p.xp
        ))
    }

    pub fn proven_count(&self, player: &Pubkey, tree_id: u64) -> usize {
        self.fetch::<ClaimAccount>(&self.claim_pda(player, tree_id))
            .map(|c| c.attempts.len())
            .unwrap_or(0)
    }

    fn fetch<T: borsh::BorshDeserialize>(&self, key: &Pubkey) -> Option<T> {
        let data = self.rpc.get_account_data(key).ok()?;
        T::deserialize(&mut &data[..]).ok()
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

/// Pull the program's own message out of the RPC error — a refusal should read as the program
/// speaking, not as a transport failure.
fn program_log(err: &str) -> String {
    err.lines()
        .map(str::trim)
        .find(|l| l.starts_with("Program log:") && !l.contains("invoke"))
        .map(|l| l.trim_start_matches("Program log:").trim().to_string())
        .unwrap_or_else(|| err.lines().next().unwrap_or("failed").to_string())
}

fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn level_name(v: u8) -> &'static str {
    match v {
        0 => "Novice", 1 => "Advanced beginner", 2 => "Competent",
        3 => "Proficient", 4 => "Expert", _ => "?",
    }
}
