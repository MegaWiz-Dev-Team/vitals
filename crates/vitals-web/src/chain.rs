//! The chain half of the app: anchor a finished run, prove it, claim a level.
//!
//! Same three instructions the CLI driver sends, called from the server so a player never sees a
//! wallet. That is the intended shape in production too — a fee-payer relay signs, and a
//! nineteen-year-old learning medicine never learns key management.

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
use vitals_progress::Difficulty;
use vitals_program::{
    ClaimAccount, Instruction, Progress, RecordWire, TreeAccount, SEED_CLAIM, SEED_PROGRESS,
    SEED_TREE,
};

pub const SPECIALTY: u8 = 1;

pub struct Chain {
    rpc: RpcClient,
    program_id: Pubkey,
    payer: Keypair,
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
        let payer = read_keypair_file(path).ok()?;
        let rpc = RpcClient::new_with_commitment(url, CommitmentConfig::confirmed());
        rpc.get_slot().ok()?;
        Some(Chain { rpc, program_id, payer })
    }

    pub fn slot(&self) -> u64 {
        self.rpc.get_slot().unwrap_or(0)
    }

    fn tree_pda(&self, tree_id: u64) -> Pubkey {
        Pubkey::find_program_address(&[SEED_TREE, &tree_id.to_le_bytes()], &self.program_id).0
    }
    fn claim_pda(&self, tree_id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[SEED_CLAIM, self.payer.pubkey().as_ref(), &tree_id.to_le_bytes()],
            &self.program_id,
        )
        .0
    }
    fn progress_pda(&self) -> Pubkey {
        Pubkey::find_program_address(
            &[SEED_PROGRESS, self.payer.pubkey().as_ref(), &[SPECIALTY]],
            &self.program_id,
        )
        .0
    }

    pub fn player(&self) -> String {
        self.payer.pubkey().to_string()
    }

    fn send(&self, ix: Instruction, extra: &[Pubkey]) -> Result<(), String> {
        let mut metas = vec![AccountMeta::new(self.payer.pubkey(), true)];
        metas.extend(extra.iter().map(|k| AccountMeta::new(*k, false)));
        metas.push(AccountMeta::new_readonly(system_program::id(), false));
        let ix = SolInstruction {
            program_id: self.program_id,
            accounts: metas,
            data: borsh::to_vec(&ix).map_err(|e| e.to_string())?,
        };
        let bh = self.rpc.get_latest_blockhash().map_err(|e| e.to_string())?;
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&self.payer.pubkey()), &[&self.payer], bh);
        self.rpc
            .send_and_confirm_transaction(&tx)
            .map(|_| ())
            .map_err(|e| program_log(&e.to_string()))
    }

    /// Anchor one finished run, then immediately prove it belongs to this player.
    pub fn anchor(
        &self,
        tree_id: u64,
        rec: &AttemptRecord,
        leaves: &[[u8; 32]],
    ) -> Result<Anchored, String> {
        self.send(
            Instruction::AnchorReplay { tree_id, record: wire(rec) },
            &[self.tree_pda(tree_id)],
        )?;

        let index = leaves.len() as u64 - 1;
        let path = merkle::prove(leaves, index).ok_or("could not build the proof")?;
        let proven = self
            .send(
                Instruction::ProveAttempt { tree_id, record: wire(rec), index, path: path.to_vec() },
                &[self.tree_pda(tree_id), self.claim_pda(tree_id)],
            )
            .is_ok();

        let tree: TreeAccount = self.fetch(&self.tree_pda(tree_id)).ok_or("tree missing")?;
        Ok(Anchored {
            index,
            root: hex32(&tree.root),
            leaves: tree.next_index,
            proven,
        })
    }

    /// Claim a level. `Ok(msg)` when the program grants it, `Err(msg)` when it recomputes and
    /// refuses — and the refusal carries the program's own log line, because that is the point.
    pub fn claim(&self, tree_id: u64, level: u8) -> Result<String, String> {
        match self.send(
            Instruction::ClaimProgress { tree_id, specialty: SPECIALTY, claimed: level },
            &[self.claim_pda(tree_id), self.progress_pda()],
        ) {
            Ok(()) => {
                let p: Option<Progress> = self.fetch(&self.progress_pda());
                Ok(match p {
                    Some(p) => format!(
                        "granted · level {} · {} attempts · {} distinct · xp {}",
                        level_name(p.level), p.attempts_counted, p.distinct_cases, p.xp
                    ),
                    None => "granted".into(),
                })
            }
            Err(e) => Err(e),
        }
    }

    pub fn proven_count(&self, tree_id: u64) -> usize {
        self.fetch::<ClaimAccount>(&self.claim_pda(tree_id))
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
