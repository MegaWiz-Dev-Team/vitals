//! **The bytes this server signs did not change when the Solana crates did.**
//!
//! On 7 ต.ค. the client crates moved from Solana 2 to 3 to drop ed25519-dalek 1 and
//! curve25519-dalek 3 from the production binary (RUSTSEC-2022-0093, RUSTSEC-2024-0344). A ward
//! transaction is a claim on a public chain, so the move had to leave every signed byte as it was.
//! These five transactions, with fixed keys and a fixed blockhash, were printed on Solana 2 and on
//! Solana 3 and came out identical; the hash below is of the Solana 2 output. A bump that changes a
//! byte fails here, before it signs anything.
use solana_sdk::{hash::Hash, message::Message, pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction};

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

#[test]
fn the_ward_s_signed_transactions_are_byte_for_byte_what_solana_2_signed() {
    let mut out = String::new();
    let relay = Keypair::new_from_array([7u8; 32]);
    let player = Keypair::new_from_array([9u8; 32]);
    let program = Pubkey::new_from_array([3u8; 32]);
    let bh = Hash::new_from_array([5u8; 32]);
    let op = relay.pubkey();
    let ixs = vec![
        ("open_account", vitals_web::ward_chain::open_account_ix(&program, &op, &player.pubkey())),
        ("commit", vitals_web::ward_chain::commit_ix(&program, &op, &player.pubkey(), [1u8; 32])),
        ("take_shift", vitals_web::ward_chain::take_shift_ix(&program, &op, &player.pubkey(), 1_790_219_940)),
        ("free_shift", vitals_web::ward_chain::free_shift_ix(&program, &op, 1_790_219_940)),
        ("release_shift", vitals_web::ward_chain::release_shift_ix(&program, &op, &player.pubkey(), 1_790_219_940)),
    ];
    for (name, ix) in ixs {
        let msg = Message::new_with_blockhash(&[ix], Some(&op), &bh);
        let mut tx = Transaction::new_unsigned(msg.clone());
        let signers: Vec<&Keypair> = if tx.message.header.num_required_signatures == 2 { vec![&relay, &player] } else { vec![&relay] };
        tx.try_sign(&signers, bh).unwrap();
        out.push_str(&format!("TX {name} msg={} sigs={}\n", hex(&msg.serialize()), tx.signatures.iter().map(|s| hex(s.as_ref())).collect::<Vec<_>>().join(",")));
    }
    use sha2::Digest;
    let got = hex(&sha2::Sha256::digest(out.as_bytes()));
    assert_eq!(got, "dcaa6583169ff74fdf9c302cf6f6e4c18416670dffb40c940c5fea5511cce537", "a signed byte changed:\n{out}");
}
