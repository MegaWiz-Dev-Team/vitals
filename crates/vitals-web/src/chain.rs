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
    commitment_pda, Account, ClaimAccount, Commitment, Instruction, Progress, RecordWire,
    TreeAccount, SEED_ACCOUNT, SEED_CLAIM, SEED_PROGRESS,
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

    /// Where this relay's tree lives.
    ///
    /// Scoped to the relay's own key, not to `tree_id` alone. The id is the slot the server
    /// started in, which is a global number two operators can pick in the same instant — and,
    /// worse, one anybody can simply read off another server and reuse. Deriving from the relay
    /// makes another operator's tree unaddressable from here rather than merely unlikely to be hit.
    fn tree_pda(&self, tree_id: u64) -> Pubkey {
        vitals_program::tree_pda(&self.program_id, &self.relay.pubkey(), tree_id).0
    }
    /// The person, keyed by the first device they ever played on.
    pub fn account_pda(&self, id: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[SEED_ACCOUNT, &id.to_bytes()], &self.program_id).0
    }

    /// Seeded on the *person*, so the same record is reachable from every machine they play on —
    /// and two people on one server never share a buffer.
    pub fn claim_pda(&self, id: &Pubkey, tree_id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[SEED_CLAIM, id.as_ref(), &tree_id.to_le_bytes()],
            &self.program_id,
        )
        .0
    }
    pub fn progress_pda(&self, id: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[SEED_PROGRESS, id.as_ref(), &[SPECIALTY]],
            &self.program_id,
        )
        .0
    }

    /// Who this account lets play. `None` when it has never been opened.
    pub fn account(&self, id: &Pubkey) -> Option<Account> {
        self.fetch(&self.account_pda(id))
    }

    /// The level as it stands on chain. Needs no key at all — reading somebody's record is not a
    /// privileged act, and that is what makes a score checkable from a machine you do not own.
    pub fn progress(&self, id: &Pubkey) -> Option<Progress> {
        self.fetch(&self.progress_pda(id))
    }

    /// This deployment's identity for storage: relay, program, cluster.
    ///
    /// The three things that make one server's leaf list a different list from another's. See
    /// `store::tree_key` for why the list cannot simply live at a fixed name.
    pub fn deployment(&self) -> (String, String, String) {
        (self.relay.pubkey().to_string(), self.program_id.to_string(), self.rpc.url())
    }

    pub fn relay_pubkey(&self) -> String {
        self.relay.pubkey().to_string()
    }

    fn ix(&self, device: &Pubkey, ix: Instruction, extra: &[Pubkey]) -> Result<SolInstruction, String> {
        let mut metas = vec![
            AccountMeta::new(self.relay.pubkey(), true),
            AccountMeta::new_readonly(*device, true),
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
    /// Build the declaration a player signs before a run starts.
    ///
    /// Bundles OpenAccount for a first-time player, because `Commit` requires the account to
    /// exist — a commitment has to belong to somebody. The transaction lands before any play
    /// happens; that ordering is the entire meaning of the word.
    pub fn prepare_commit(&self, device: &Pubkey, id: &Pubkey, hash: [u8; 32]) -> Result<Pending, String> {
        let acc = self.account_pda(id);
        let commit = commitment_pda(&self.program_id, &id.to_bytes()).0;
        let mut ixs = Vec::new();
        if self.account(id).is_none() {
            ixs.push(self.ix(device, Instruction::OpenAccount, &[acc])?);
        }
        ixs.push(self.ix(device, Instruction::Commit { hash }, &[acc, commit])?);
        self.prepare(device, ixs)
    }

    /// What the chain says this player has open, and how many times they have ever started.
    ///
    /// Read back after a commit lands, because the slot is assigned on chain — the client cannot
    /// know it in advance, and the record it builds later must carry the same slot the program
    /// will stamp into the leaf, or the two sides compute different hashes.
    pub fn commitment(&self, id: &Pubkey) -> Option<Commitment> {
        self.fetch(&commitment_pda(&self.program_id, &id.to_bytes()).0)
    }

    /// The anchor and its proof, as two transactions the player signs together.
    ///
    /// They used to be one. The vt02 record made the wire 80 bytes longer, and it rides in both
    /// instructions — with the 384-byte Merkle path on top, the combined transaction reached
    /// 1,656 bytes against a 1,232-byte packet limit. Two transactions fit with room to spare,
    /// and the in-page key signs both in the same breath, so the player notices nothing.
    ///
    /// If the first lands and the second fails, the run is anchored but not yet proven — an
    /// intact state, not a corrupt one: the leaf is in the tree and the proof can be re-sent.
    pub fn prepare_anchor(
        &self,
        device: &Pubkey,
        id: &Pubkey,
        tree_id: u64,
        rec: &AttemptRecord,
        leaves: &[[u8; 32]],
    ) -> Result<(Pending, Pending), String> {
        let index = leaves.len() as u64 - 1;
        let path = merkle::prove(leaves, index).ok_or("could not build the proof")?;
        let acc = self.account_pda(id);
        let mut ixs = Vec::new();
        // First run from a new machine opens the person. Idempotent on chain, so a second tab
        // doing the same thing is not an error.
        if self.account(id).is_none() {
            ixs.push(self.ix(device, Instruction::OpenAccount, &[acc])?);
        }
        ixs.push(self.ix(device, Instruction::AnchorReplay { tree_id, record: wire(rec) },
                         &[acc, self.tree_pda(tree_id),
                           commitment_pda(&self.program_id, &id.to_bytes()).0])?);
        let anchor = self.prepare(device, ixs)?;
        let prove = self.prepare(device, vec![self.ix(device,
            Instruction::ProveAttempt { tree_id, record: wire(rec), index, path: path.to_vec(),
                                        commitment: rec.commitment, committed_slot: rec.committed_slot },
            &[acc, self.tree_pda(tree_id), self.claim_pda(id, tree_id)])?])?;
        Ok((anchor, prove))
    }

    /// Let another machine act as this person. Signed by one that already can.
    pub fn prepare_link(&self, device: &Pubkey, id: &Pubkey, add: &Pubkey, on: bool) -> Result<Pending, String> {
        let acc = self.account_pda(id);
        let mut ixs = Vec::new();
        // The account only exists once something has been anchored against it, and someone can
        // reasonably want a second machine before they have finished their first case. Opening it
        // here rather than refusing means the button does what it says — the earlier version built
        // the open and dropped the device on the floor.
        if self.account(id).is_none() {
            if device != id {
                return Err("that account does not exist yet".into());
            }
            ixs.push(self.ix(device, Instruction::OpenAccount, &[acc])?);
        }
        let ix = if on {
            Instruction::AddAuthority { device: add.to_bytes() }
        } else {
            Instruction::RemoveAuthority { device: add.to_bytes() }
        };
        ixs.push(self.ix(device, ix, &[acc])?);
        self.prepare(device, ixs)
    }

    /// What the tree looks like now. Read after the transaction confirms.
    pub fn anchored(&self, id: &Pubkey, tree_id: u64, index: u64) -> Result<Anchored, String> {
        let tree: TreeAccount = self.fetch(&self.tree_pda(tree_id)).ok_or("tree missing")?;
        Ok(Anchored {
            index,
            root: hex32(&tree.root),
            leaves: tree.next_index,
            proven: self.proven_count(id, tree_id) > 0,
        })
    }

    /// Claim a level. `Ok(msg)` when the program grants it, `Err(msg)` when it recomputes and
    /// refuses — and the refusal carries the program's own log line, because that is the point.
    pub fn prepare_claim(&self, device: &Pubkey, id: &Pubkey, tree_id: u64, level: u8) -> Result<Pending, String> {
        self.prepare(
            device,
            vec![self.ix(
                device,
                Instruction::ClaimProgress { tree_id, specialty: SPECIALTY, claimed: level },
                &[self.account_pda(id), self.claim_pda(id, tree_id), self.progress_pda(id)],
            )?],
        )
    }

    /// Read the level back off chain after a claim lands.
    ///
    /// The level reported is the one stored in the account, which is the one the *program*
    /// computed — not the one the player asked for. Echoing the request back would make the UI
    /// agree with every claim, including the ones the chain refused.
    pub fn claimed(&self, id: &Pubkey) -> Result<String, String> {
        let p: Progress = self
            .fetch(&self.progress_pda(id))
            .ok_or("the claim landed but no progress account was written")?;
        Ok(format!(
            "granted · level {} · {} attempts · {} distinct · xp {}",
            level_name(p.level), p.attempts_counted, p.distinct_cases, p.xp
        ))
    }

    pub fn proven_count(&self, id: &Pubkey, tree_id: u64) -> usize {
        self.fetch::<ClaimAccount>(&self.claim_pda(id, tree_id))
            .map(|c| c.attempts.len())
            .unwrap_or(0)
    }

    /// Stars on this tree — distinct exam-mode cases the player has cleared at or above `pass_bps`.
    /// Reads the same claim buffer as `proven_count`, maps each proven attempt into the scorer's
    /// shape, and defers to `vitals_progress::stars`. Additive: the level path is unchanged.
    pub fn star_count(&self, id: &Pubkey, tree_id: u64, pass_bps: u32) -> u32 {
        self.fetch::<ClaimAccount>(&self.claim_pda(id, tree_id))
            .map(|c| {
                let attempts: Vec<vitals_progress::Attempt> = c
                    .attempts
                    .iter()
                    .map(|a| vitals_progress::Attempt {
                        case: a.case,
                        score: a.score,
                        max: a.max,
                        difficulty: match a.difficulty {
                            1 => Difficulty::Intern,
                            2 => Difficulty::Resident,
                            _ => Difficulty::Student,
                        },
                        exam_mode: a.exam_mode,
                    })
                    .collect();
                vitals_progress::stars(&attempts, pass_bps)
            })
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
        // Mirrors of what the record already holds. Deliberately no commitment: the program reads
        // that from the account, so there is nothing here for a caller to assert.
        rubric_hash: r.rubric_hash,
        det_score: r.det_score,
        det_max: r.det_max,
        judged_score: r.judged_score,
        judged_max: r.judged_max,
    }
}

/// Pull the program's own message out of the RPC error — a refusal should read as the program
/// speaking, not as a transport failure.
fn program_log(err: &str) -> String {
    // The *last* log line, not the first. A transaction carries several instructions now, and the
    // earlier ones succeeded and said so — reporting their success as the reason for the failure
    // is worse than reporting nothing.
    if let Some(line) = err
        .lines()
        .map(str::trim).rfind(|l| l.starts_with("Program log:") && !l.contains("invoke"))
    {
        return line.trim_start_matches("Program log:").trim().to_string();
    }
    // A refusal the program made without saying anything arrives as a bare hex code. "0xa" is not
    // a message; it is the absence of one.
    if let Some(code) = err
        .split("custom program error: 0x")
        .nth(1)
        .and_then(|t| u32::from_str_radix(t.trim().split(|c: char| !c.is_ascii_hexdigit()).next()?, 16).ok())
    {
        return match code {
            0 => "the arithmetic does not support that claim",
            1 => "the record does not decode",
            2 => "wrong account address",
            3 => "nothing proven yet",
            4 => "that run is not in the tree",
            5 => "this claim buffer is full",
            6 => "that run has already been proven",
            7 => "the tree is full",
            8 => "that run belongs to someone else",
            9 => "that account is not this program's",
            10 => "this machine is not linked to that account",
            11 => "no room for another device",
            12 => "that device is already linked",
            13 => "that is the last device — removing it would strand the record",
            14 => "no account yet — play a case first",
            _ => "the program refused it",
        }
        .to_string();
    }
    err.lines().next().unwrap_or("failed").to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this pins: a transaction carries several instructions, and the earlier ones log
    /// their success. Taking the *first* log line reported that success as the reason for the
    /// failure — "anchored leaf at index 1" shown to someone whose claim was rejected.
    #[test]
    fn the_reason_is_the_last_thing_the_program_said() {
        let err = "\
RPC response error -32002: Transaction simulation failed: Error processing Instruction 1
    Program log: Instruction: AnchorReplay
    Program log: anchored leaf at index 1 — score 100 of 100
    Program log: claim rejected: claimed Expert, computed Competent
    Program abc invoke [1]";
        assert_eq!(program_log(err), "claim rejected: claimed Expert, computed Competent");
    }

    /// A refusal the program made without saying anything arrives as a bare hex code.
    #[test]
    fn a_bare_error_code_becomes_a_sentence() {
        let err = "RPC response error -32002: Transaction simulation failed: \
                   Error processing Instruction 0: custom program error: 0xa; 3 log messages:";
        assert_eq!(program_log(err), "this machine is not linked to that account");
    }

    #[test]
    fn every_error_the_program_can_return_has_words() {
        // 0..=14 are the VitalsError variants. None may fall through to the generic text, or a
        // refusal arrives at a person as a hex number.
        for code in 0..=14u32 {
            let err = format!("custom program error: 0x{code:x};");
            let got = program_log(&err);
            assert_ne!(got, "the program refused it", "error {code} has no message of its own");
            assert!(!got.contains("0x"), "error {code} leaked its code: {got}");
        }
    }

    #[test]
    fn an_unknown_code_still_says_something_human() {
        assert_eq!(program_log("custom program error: 0xff;"), "the program refused it");
    }

    #[test]
    fn a_transport_failure_is_not_dressed_up_as_a_refusal() {
        let err = "error sending request for url (http://127.0.0.1:8899/)\nCaused by: connection refused";
        assert!(program_log(err).starts_with("error sending request"));
    }

    #[test]
    fn hex32_is_lowercase_and_sixty_four_characters() {
        let h = hex32(&[0xab; 32]);
        assert_eq!(h.len(), 64);
        assert_eq!(h, "ab".repeat(32));
        assert_eq!(hex32(&[0u8; 32]), "0".repeat(64));
    }

    #[test]
    fn levels_are_named_in_order_and_nothing_beyond_them_is() {
        assert_eq!(
            (0..5).map(level_name).collect::<Vec<_>>(),
            ["Novice", "Advanced beginner", "Competent", "Proficient", "Expert"]
        );
        assert_eq!(level_name(5), "?", "an out-of-range level must not read as a real one");
        assert_eq!(level_name(255), "?");
    }

    #[test]
    fn difficulty_survives_the_trip_to_the_wire_and_back() {
        use vitals_progress::record::Outcome;
        for (d, n) in [(Difficulty::Student, 0u8), (Difficulty::Intern, 1), (Difficulty::Resident, 2)] {
            let r = AttemptRecord {
                player: [1; 32], sce_hash: [2; 32], case: [3; 32], run_hash: [4; 32],
                difficulty: d, exam_mode: false, outcome: Outcome::WinDischarge, harm_count: 0,
                commitment: [0u8; 32], committed_slot: 0, rubric_hash: [0u8; 32],
                det_score: 0, det_max: 0, judged_score: 0, judged_max: 0,
            };
            assert_eq!(wire(&r).difficulty, n, "{d:?} must encode as {n}");
        }
    }
}
