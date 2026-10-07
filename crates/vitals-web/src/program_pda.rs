//! The program's PDA functions, in the client's key type.
//!
//! `vitals-program` is on-chain code built on solana-program 2, and stays there: moving it would
//! mean a redeploy. The off-chain clients are on solana-sdk 3, because sdk 2's keypair pulls
//! ed25519-dalek 1 / curve25519-dalek 3 (RUSTSEC-2022-0093, RUSTSEC-2024-0344) into the server
//! binary. The two `Pubkey`s are the same 32 bytes under two type names, so the program's own
//! derivations are called here and the key crosses the boundary as a `[u8; 32]` — the address is
//! still computed by the program's code, never re-derived by hand on this side.
//!
//! `vitals-cli` includes this file by path rather than keeping a second copy.

use solana_sdk::pubkey::Pubkey;

/// Into the program's key type. Generic so this crate never has to name solana-program 2.
fn program_key<K: From<[u8; 32]>>(k: &Pubkey) -> K {
    K::from(k.to_bytes())
}

pub fn tree_pda(program_id: &Pubkey, operator: &Pubkey, tree_id: u64) -> (Pubkey, u8) {
    let (k, bump) =
        vitals_program::tree_pda(&program_key(program_id), &program_key(operator), tree_id);
    (Pubkey::new_from_array(k.to_bytes()), bump)
}

pub fn commitment_pda(program_id: &Pubkey, player: &[u8; 32]) -> (Pubkey, u8) {
    let (k, bump) = vitals_program::commitment_pda(&program_key(program_id), player);
    (Pubkey::new_from_array(k.to_bytes()), bump)
}

pub fn patient_pda(program_id: &Pubkey, operator: &Pubkey, patient_id: u64) -> (Pubkey, u8) {
    let (k, bump) =
        vitals_program::patient_pda(&program_key(program_id), &program_key(operator), patient_id);
    (Pubkey::new_from_array(k.to_bytes()), bump)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vitals_program::{SEED_COMMIT, SEED_PATIENT, SEED_TREE};

    // The program's derivation, crossed through bytes, must land on the address the client's own
    // sdk derives from the same seeds. A lossy conversion would send a transaction to an account
    // the program then refuses — or worse, reads.
    #[test]
    fn the_boundary_moves_no_address() {
        let pid = Pubkey::new_from_array([7; 32]);
        let op = Pubkey::new_from_array([9; 32]);
        let player = [3u8; 32];

        let want =
            Pubkey::find_program_address(&[SEED_TREE, op.as_ref(), &5u64.to_le_bytes()], &pid);
        assert_eq!(tree_pda(&pid, &op, 5), want);

        let want = Pubkey::find_program_address(&[SEED_COMMIT, &player], &pid);
        assert_eq!(commitment_pda(&pid, &player), want);

        let want =
            Pubkey::find_program_address(&[SEED_PATIENT, op.as_ref(), &42u64.to_le_bytes()], &pid);
        assert_eq!(patient_pda(&pid, &op, 42), want);
    }
}
